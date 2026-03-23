import { type UnlistenFn, listen } from '@tauri-apps/api/event';
import { createSignal, onCleanup } from 'solid-js';
import { createStore, produce } from 'solid-js/store';
/**
 * Unified Shepherd Chat Hook
 *
 * Manages Shepherd chat sessions for the docked Shepherd surface.
 * The desktop UI now uses only project or general Shepherd contexts.
 */
import { invoke } from '../lib/invoke';
import type {
  ChatEvent,
  ChatImage,
  ChatMessage,
  ChatToolCall,
  ShepherdImageInput,
  ShepherdMessageChunk,
  ShepherdScope,
  StartShepherdSessionRequest,
  StartShepherdSessionResponse,
} from '../lib/types';

export type ShepherdContextType = 'general' | 'project';

export interface ShepherdChatContext {
  type: ShepherdContextType;
  projectId?: number;
  focusNodeId?: string;
  focusNodeName?: string;
}

interface UseShepherdChatOptions {
  /** History depth (default: 20) */
  historyDepth?: number;
  /** Callback when Shepherd finishes editing board files */
  onEditComplete?: () => void;
}

interface UseShepherdChatReturn {
  // Connection
  connected: () => boolean;
  connecting: () => boolean;
  connect: () => Promise<void>;
  disconnect: () => Promise<void>;

  // Messages
  messages: ChatMessage[];
  currentMessage: () => Partial<ChatMessage> | null;
  sendMessage: (content: string, images?: ShepherdImageInput[]) => Promise<void>;

  // Context
  context: () => ShepherdChatContext;
  setFocusNode: (id: string | null, name: string | null) => void;

  // Status
  shepherdEditing: () => boolean;
  editingIslands: () => Set<string>;

  // History
  clearHistory: () => Promise<void>;

  // Reset (disconnect, clear history, reconnect)
  reset: () => Promise<void>;
}

const WELCOME_MESSAGE = `Hello! I'm Shepherd, your AI assistant for Hirsel. I can help you steer the project surface, routes, work items, and workers.

What would you like to do today?`;

export function useShepherdChat(
  getContext: () => ShepherdChatContext,
  options: UseShepherdChatOptions = {},
): UseShepherdChatReturn {
  const { historyDepth = 20, onEditComplete } = options;

  // Connection state
  const [sessionId, setSessionId] = createSignal<string | null>(null);
  const [connected, setConnected] = createSignal(false);
  const [connecting, setConnecting] = createSignal(false);

  const [messages, setMessages] = createStore<ChatMessage[]>([]);
  const [currentMessage, setCurrentMessage] = createSignal<Partial<ChatMessage> | null>(null);

  // Editing state
  const [shepherdEditing, setShepherdEditing] = createSignal(false);
  const [editingIslands, setEditingIslands] = createSignal<Set<string>>(new Set());

  // Focused work item (for project context)
  const [focusNodeId, setFocusNodeIdState] = createSignal<string | null>(null);
  const [focusNodeName, setFocusNodeNameState] = createSignal<string | null>(null);

  // Track tool calls during streaming
  const toolsById = new Map<string, ChatToolCall>();
  let lastChunkType: 'text' | null = null;
  let unlisten: UnlistenFn | undefined;

  const sanitizeAssistantText = (text: string): string => {
    // Lash can surface internal repl delimiters in streamed text in edge cases.
    // Keep UI clean by stripping repl markup/fragments from assistant-visible text.
    const out = text.replace(/<\/?repl>/gi, '');
    const trimmed = out.trim().toLowerCase();
    if (trimmed.includes('<') && /^[<>/repl\s]+$/.test(trimmed)) {
      return '';
    }
    return out;
  };

  const errorMessage = (error: unknown): string => {
    if (typeof error === 'string') return error;
    if (error instanceof Error) return error.message;
    try {
      return JSON.stringify(error);
    } catch {
      return String(error);
    }
  };

  // Context accessor
  const context = () => {
    const ctx = getContext();
    return {
      ...ctx,
      focusNodeId: focusNodeId() ?? ctx.focusNodeId,
      focusNodeName: focusNodeName() ?? ctx.focusNodeName,
    };
  };

  const setFocusNode = (id: string | null, name: string | null) => {
    setFocusNodeIdState(id);
    setFocusNodeNameState(name);
  };

  // Convert context to backend request
  const contextToRequest = (ctx: ShepherdChatContext): StartShepherdSessionRequest => {
    if (ctx.type === 'project' && ctx.projectId) {
      if (ctx.focusNodeId && ctx.focusNodeName) {
        return {
          type: 'projectFocused',
          projectId: ctx.projectId,
          taskId: ctx.focusNodeId,
          taskName: ctx.focusNodeName,
        };
      }
      return { type: 'project', projectId: ctx.projectId };
    }
    return { type: 'general' };
  };

  // Convert context to scope for sending messages
  const contextToScope = (ctx: ShepherdChatContext): ShepherdScope => {
    if (ctx.type === 'project' && ctx.projectId) {
      const focus =
        ctx.focusNodeId && ctx.focusNodeName
          ? { taskId: ctx.focusNodeId, taskName: ctx.focusNodeName }
          : undefined;
      return { type: 'project', projectId: ctx.projectId, focus };
    }
    return { type: 'general' };
  };

  // Parse chunks helpers
  const parseChunksContent = (chunksJson: string): string => {
    try {
      const chunks = JSON.parse(chunksJson);
      return sanitizeAssistantText(
        chunks
          .filter((c: { type: string }) => c.type === 'text')
          .map((c: { content: string }) => c.content)
          .join(''),
      );
    } catch {
      return '';
    }
  };

  const parseChunksToolCalls = (chunksJson: string): ChatToolCall[] | undefined => {
    try {
      const chunks = JSON.parse(chunksJson);
      const tools = chunks
        .filter((c: { type: string }) => c.type === 'tool')
        .map(
          (c: {
            id: string;
            title: string;
            kind?: string | null;
            status: string;
            input?: string | null;
            output?: string | null;
          }) => ({
            id: c.id,
            title: c.title,
            kind: c.kind ?? null,
            status: c.status,
            input: c.input ?? null,
            output: c.output ?? null,
          }),
        );
      return tools.length > 0 ? tools : undefined;
    } catch {
      return undefined;
    }
  };

  const parseChunksImages = (chunksJson: string): ChatImage[] | undefined => {
    try {
      const chunks = JSON.parse(chunksJson);
      const images = chunks
        .filter((c: { type: string }) => c.type === 'image')
        .map((c: { mimeType?: string; dataBase64?: string; name?: string; src?: string }) => {
          const mimeType = c.mimeType || 'image/png';
          const dataBase64 = c.dataBase64 || '';
          const src = c.src || `data:${mimeType};base64,${dataBase64}`;
          return {
            src,
            mimeType,
            name: c.name,
            dataBase64: dataBase64 || undefined,
          };
        })
        .filter((img: ChatImage) => !!img.src);
      return images.length > 0 ? images : undefined;
    } catch {
      return undefined;
    }
  };

  // Handle chat events from backend
  const handleChatEvent = (event: ChatEvent) => {
    switch (event.type) {
      case 'textDelta': {
        // Start new message or append
        if (lastChunkType !== 'text') {
          lastChunkType = 'text';
        }
        setCurrentMessage((prev) => ({
          ...prev,
          id: prev?.id || `msg-${Date.now()}`,
          role: 'assistant',
          content: sanitizeAssistantText((prev?.content || '') + event.text),
          streaming: true,
        }));
        break;
      }

      case 'toolCallStart': {
        const tool: ChatToolCall = {
          id: event.toolCallId,
          title: event.title,
          kind: event.kind,
          status: 'in_progress',
          input: event.input,
          output: null,
        };
        toolsById.set(event.toolCallId, tool);
        setCurrentMessage((prev) => ({
          ...prev,
          id: prev?.id || `msg-${Date.now()}`,
          role: 'assistant',
          toolCalls: [...(prev?.toolCalls || []), tool],
          streaming: true,
        }));

        // Track file editing
        const isEditTool =
          event.kind === 'edit' ||
          event.kind === 'write' ||
          /\b(edit|write)\b/i.test(event.title || '');
        if (isEditTool) {
          setShepherdEditing(true);
          // Extract file path if available
          if (event.input) {
            try {
              const input = JSON.parse(event.input);
              if (input.file_path) {
                setEditingIslands((prev) => new Set<string>([...prev, input.file_path]));
              }
            } catch {
              // Ignore parse errors
            }
          }
        }
        break;
      }

      case 'toolCallUpdate': {
        const tool = toolsById.get(event.toolCallId);
        if (tool) {
          tool.status = event.status;
          if (event.output) tool.output = event.output;
          if (event.title) tool.title = event.title;

          setCurrentMessage((prev) => ({
            ...prev,
            toolCalls: prev?.toolCalls?.map((t) => (t.id === event.toolCallId ? { ...tool } : t)),
          }));

          // Track completion of edit tools
          if (
            (tool.kind === 'edit' ||
              tool.kind === 'write' ||
              /\b(edit|write)\b/i.test(tool.title || '')) &&
            (event.status === 'completed' || event.status === 'failed')
          ) {
            // Check if any edit tools still running
            const stillEditing = Array.from(toolsById.values()).some(
              (t) =>
                (t.kind === 'edit' ||
                  t.kind === 'write' ||
                  /\b(edit|write)\b/i.test(t.title || '')) &&
                t.status === 'in_progress',
            );
            if (!stillEditing) {
              setShepherdEditing(false);
              onEditComplete?.();
            }
          }
        }
        break;
      }

      case 'messageComplete': {
        // Finalize message
        const current = currentMessage();
        if (current) {
          const content = sanitizeAssistantText(current.content || '').trim();
          const hasTools = (current.toolCalls?.length || 0) > 0;
          const hasThinking = !!current.thinking?.trim();
          if (content || hasTools || hasThinking) {
            const finalMessage: ChatMessage = {
              id: current.id || `msg-${Date.now()}`,
              role: 'assistant',
              content,
              thinking: current.thinking,
              toolCalls: current.toolCalls,
              timestamp: new Date(),
            };
            setMessages(produce((msgs) => msgs.push(finalMessage)));
          }
        }
        setCurrentMessage(null);
        toolsById.clear();
        lastChunkType = null;
        setShepherdEditing(false);
        setEditingIslands(new Set<string>());
        break;
      }

      case 'error': {
        console.error('[shepherd-chat] Error:', event.message);
        window.toast?.error(event.message);
        setCurrentMessage(null);
        toolsById.clear();
        lastChunkType = null;
        setShepherdEditing(false);
        break;
      }

      case 'sessionEnded': {
        console.log('[shepherd-chat] Session ended');
        setConnected(false);
        setSessionId(null);
        break;
      }
    }
  };

  // Connect to Shepherd
  const connect = async () => {
    if (connected() || connecting()) return;

    setConnecting(true);
    const ctx = context();

    try {
      // Subscribe to unified event channel
      unlisten = await listen<[string, ChatEvent]>('shepherd-event', (event) => {
        const [eventSessionId, chatEvent] = event.payload;
        // Only handle events for our session
        if (eventSessionId === sessionId()) {
          handleChatEvent(chatEvent);
        }
      });

      // Load history
      await loadHistory(ctx);

      // Start unified session
      const request = contextToRequest(ctx);
      const response = await invoke<StartShepherdSessionResponse>('start_shepherd_session', {
        request,
      });

      setSessionId(response.sessionId);
      setConnected(true);
    } catch (e) {
      console.error('[shepherd-chat] Failed to connect:', e);
      window.toast?.error(`Failed to connect to Shepherd: ${errorMessage(e)}`);
      unlisten?.();
      unlisten = undefined;
    } finally {
      setConnecting(false);
    }
  };

  // Load chat history
  const loadHistory = async (ctx: ShepherdChatContext) => {
    try {
      const scope = contextToScope(ctx);
      const history = await invoke<Array<{ role: string; chunksJson: string }>>(
        'get_shepherd_history',
        {
          scope,
          limit: historyDepth,
        },
      );

      if (history && history.length > 0) {
        const loadedMessages: ChatMessage[] = history.map((msg, idx) => {
          const chunksStr = msg.chunksJson || '[]';
          return {
            id: `history-${idx}`,
            role: msg.role as 'user' | 'assistant',
            content: parseChunksContent(chunksStr),
            images: parseChunksImages(chunksStr),
            toolCalls: parseChunksToolCalls(chunksStr),
            timestamp: new Date(),
          };
        });
        setMessages(loadedMessages);
      } else {
        // Add welcome message if no history
        setMessages([
          {
            id: 'welcome',
            role: 'assistant',
            content: WELCOME_MESSAGE,
            timestamp: new Date(),
          },
        ]);
      }
    } catch (e) {
      console.error('[shepherd-chat] Failed to load history:', e);
      setMessages([
        {
          id: 'welcome',
          role: 'assistant',
          content: WELCOME_MESSAGE,
          timestamp: new Date(),
        },
      ]);
    }
  };

  // Disconnect
  const disconnect = async () => {
    const sid = sessionId();
    if (sid) {
      try {
        await invoke('stop_shepherd_session', { sessionId: sid });
      } catch (e) {
        console.error('[shepherd-chat] Failed to stop session:', e);
      }
    }
    unlisten?.();
    unlisten = undefined;
    setConnected(false);
    setSessionId(null);
    setCurrentMessage(null);
    setShepherdEditing(false);
    setEditingIslands(new Set<string>());
    toolsById.clear();
    lastChunkType = null;
    // Clear messages to prevent stale UI during project transitions
    setMessages([]);
  };

  // Send message
  const sendMessage = async (content: string, images: ShepherdImageInput[] = []) => {
    const sid = sessionId();
    const ctx = context();
    const text = content.trim();

    if (!sid || !connected() || (!text && images.length === 0)) {
      window.toast?.error('Not connected to Shepherd');
      return;
    }

    // Add user message to UI
    const userMessage: ChatMessage = {
      id: `user-${Date.now()}`,
      role: 'user',
      content: text,
      images: images.map((image) => ({
        src: `data:${image.mimeType};base64,${image.dataBase64}`,
        mimeType: image.mimeType,
        name: image.name,
        dataBase64: image.dataBase64,
      })),
      timestamp: new Date(),
    };
    setMessages(produce((msgs) => msgs.push(userMessage)));

    // Initialize streaming state
    setCurrentMessage({ id: `msg-${Date.now()}`, role: 'assistant', streaming: true });

    try {
      const focus =
        ctx.focusNodeId && ctx.focusNodeName
          ? { taskId: ctx.focusNodeId, taskName: ctx.focusNodeName }
          : null;

      const chunks: ShepherdMessageChunk[] = [
        ...(text ? [{ type: 'text' as const, content: text }] : []),
        ...images.map((image) => ({
          type: 'image' as const,
          mimeType: image.mimeType,
          dataBase64: image.dataBase64,
          name: image.name,
        })),
      ];

      await invoke('send_shepherd_message', {
        sessionId: sid,
        content: text || null,
        chunks,
        focus,
      });
    } catch (e) {
      console.error('[shepherd-chat] Failed to send message:', e);
      window.toast?.error(`Failed to send message: ${errorMessage(e)}`);
      setCurrentMessage(null);
    }
  };

  // Clear history
  const clearHistory = async () => {
    const ctx = context();
    try {
      const scope = contextToScope(ctx);
      await invoke('clear_shepherd_history', { scope });
      setMessages([
        {
          id: 'welcome',
          role: 'assistant',
          content: WELCOME_MESSAGE,
          timestamp: new Date(),
        },
      ]);
    } catch (e) {
      console.error('[shepherd-chat] Failed to clear history:', e);
    }
  };

  // Reset (disconnect, clear, reconnect)
  const reset = async () => {
    await disconnect();
    await clearHistory();
    await connect();
  };

  // Cleanup on unmount
  onCleanup(() => {
    disconnect();
  });

  return {
    connected,
    connecting,
    connect,
    disconnect,
    messages,
    currentMessage,
    sendMessage,
    context,
    setFocusNode,
    shepherdEditing,
    editingIslands,
    clearHistory,
    reset,
  };
}
