/**
 * Unified Gyp Chat Hook
 *
 * Manages Gyp chat sessions across all contexts (board, run, draft, general).
 * Uses the unified backend GypContextBuilder for consistent prompt and context handling.
 */
import { invoke } from '@tauri-apps/api/core';
import { type UnlistenFn, listen } from '@tauri-apps/api/event';
import { createSignal, onCleanup } from 'solid-js';
import { createStore, produce } from 'solid-js/store';
import type {
  ChatEvent,
  ChatMessage,
  ChatToolCall,
  GypScope,
  PendingPermission,
  StartGypSessionRequest,
  StartGypSessionResponse,
  TaskFocus,
} from '../lib/types';

export type GypContextType = 'general' | 'board' | 'run' | 'draft';

export interface GypChatContext {
  type: GypContextType;
  projectId?: number;
  projectName?: string;
  runName?: string;
  focusNodeId?: string;
  focusNodeName?: string;
}

interface UseGypChatOptions {
  /** History depth (default: 20) */
  historyDepth?: number;
  /** Callback when Gyp finishes editing board files */
  onEditComplete?: () => void;
}

interface UseGypChatReturn {
  // Connection
  connected: () => boolean;
  connecting: () => boolean;
  sessionId: () => string | null;
  connect: () => Promise<void>;
  disconnect: () => Promise<void>;

  // Messages
  messages: ChatMessage[];
  currentMessage: () => Partial<ChatMessage> | null;
  sendMessage: (content: string) => Promise<void>;

  // Context
  context: () => GypChatContext;
  setFocusNode: (id: string | null, name: string | null) => void;

  // Status
  gypEditing: () => boolean;
  editingIslands: () => Set<string>;

  // Permissions
  pendingPermission: () => PendingPermission | null;
  respondToPermission: (optionId: string) => Promise<void>;

  // History
  clearHistory: () => Promise<void>;

  // Reset (disconnect, clear history, reconnect)
  reset: () => Promise<void>;
}

const WELCOME_MESSAGE = `Hello! I'm Gyp, your AI assistant for Hirsel. I can help you manage runs, tasks, and workers.

What would you like to do today?`;

export function useGypChat(
  getContext: () => GypChatContext,
  options: UseGypChatOptions = {},
): UseGypChatReturn {
  const { historyDepth = 20, onEditComplete } = options;

  // Connection state
  const [sessionId, setSessionId] = createSignal<string | null>(null);
  const [connected, setConnected] = createSignal(false);
  const [connecting, setConnecting] = createSignal(false);

  // Current scope (from backend)
  const [currentScope, setCurrentScope] = createSignal<GypScope | null>(null);

  // Message state
  const [messages, setMessages] = createStore<ChatMessage[]>([]);
  const [currentMessage, setCurrentMessage] = createSignal<Partial<ChatMessage> | null>(null);

  // Editing state
  const [gypEditing, setGypEditing] = createSignal(false);
  const [editingIslands, setEditingIslands] = createSignal<Set<string>>(new Set());

  // Permission state
  const [pendingPermission, setPendingPermission] = createSignal<PendingPermission | null>(null);

  // Focus node (for board context)
  const [focusNodeId, setFocusNodeIdState] = createSignal<string | null>(null);
  const [focusNodeName, setFocusNodeNameState] = createSignal<string | null>(null);

  // Track tool calls during streaming
  const toolsById = new Map<string, ChatToolCall>();
  let lastChunkType: 'text' | 'thinking' | null = null;
  let unlisten: UnlistenFn | undefined;

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
  const contextToRequest = (ctx: GypChatContext): StartGypSessionRequest => {
    if (ctx.type === 'board' && ctx.projectId) {
      if (ctx.focusNodeId && ctx.focusNodeName) {
        return {
          type: 'boardFocused',
          projectId: ctx.projectId,
          taskId: ctx.focusNodeId,
          taskName: ctx.focusNodeName,
        };
      }
      return { type: 'board', projectId: ctx.projectId };
    }
    if ((ctx.type === 'run' || ctx.type === 'draft') && ctx.runName) {
      return { type: 'run', runName: ctx.runName };
    }
    return { type: 'general' };
  };

  // Convert context to scope for sending messages
  const contextToScope = (ctx: GypChatContext): GypScope => {
    if (ctx.type === 'board' && ctx.projectId) {
      const focus: TaskFocus | undefined =
        ctx.focusNodeId && ctx.focusNodeName
          ? { taskId: ctx.focusNodeId, taskName: ctx.focusNodeName }
          : undefined;
      return { type: 'board', projectId: ctx.projectId, focus };
    }
    if ((ctx.type === 'run' || ctx.type === 'draft') && ctx.runName) {
      return { type: 'run', runName: ctx.runName, workspacePath: '' };
    }
    return { type: 'general' };
  };

  // Parse chunks helpers
  const parseChunksContent = (chunksJson: string): string => {
    try {
      const chunks = JSON.parse(chunksJson);
      return chunks
        .filter((c: { type: string }) => c.type === 'text')
        .map((c: { content: string }) => c.content)
        .join('');
    } catch {
      return '';
    }
  };

  const parseChunksThinking = (chunksJson: string): string | undefined => {
    try {
      const chunks = JSON.parse(chunksJson);
      const thinking = chunks
        .filter((c: { type: string }) => c.type === 'thinking')
        .map((c: { content: string }) => c.content)
        .join('');
      return thinking || undefined;
    } catch {
      return undefined;
    }
  };

  const parseChunksToolCalls = (chunksJson: string): ChatToolCall[] | undefined => {
    try {
      const chunks = JSON.parse(chunksJson);
      const tools = chunks.filter((c: { type: string }) => c.type === 'tool');
      return tools.length > 0 ? tools : undefined;
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
          content: (prev?.content || '') + event.text,
          streaming: true,
        }));
        break;
      }

      case 'thinkingDelta': {
        if (lastChunkType !== 'thinking') {
          lastChunkType = 'thinking';
        }
        setCurrentMessage((prev) => ({
          ...prev,
          id: prev?.id || `msg-${Date.now()}`,
          role: 'assistant',
          thinking: (prev?.thinking || '') + event.text,
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
        if (event.title === 'Edit' || event.title === 'Write') {
          setGypEditing(true);
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
            (tool.title === 'Edit' || tool.title === 'Write') &&
            (event.status === 'completed' || event.status === 'failed')
          ) {
            // Check if any edit tools still running
            const stillEditing = Array.from(toolsById.values()).some(
              (t) => (t.title === 'Edit' || t.title === 'Write') && t.status === 'in_progress',
            );
            if (!stillEditing) {
              setGypEditing(false);
              onEditComplete?.();
            }
          }
        }
        break;
      }

      case 'permissionRequest': {
        setPendingPermission(event.request);
        break;
      }

      case 'messageComplete': {
        // Finalize message
        const current = currentMessage();
        if (current) {
          const finalMessage: ChatMessage = {
            id: current.id || `msg-${Date.now()}`,
            role: 'assistant',
            content: current.content || '',
            thinking: current.thinking,
            toolCalls: current.toolCalls,
            timestamp: new Date(),
          };
          setMessages(produce((msgs) => msgs.push(finalMessage)));

          // Save to history
          saveAssistantMessage(finalMessage);
        }
        setCurrentMessage(null);
        toolsById.clear();
        lastChunkType = null;
        setGypEditing(false);
        setEditingIslands(new Set<string>());
        break;
      }

      case 'error': {
        console.error('[gyp-chat] Error:', event.message);
        window.toast?.error(event.message);
        setCurrentMessage(null);
        toolsById.clear();
        lastChunkType = null;
        setGypEditing(false);
        break;
      }

      case 'sessionEnded': {
        console.log('[gyp-chat] Session ended');
        setConnected(false);
        setSessionId(null);
        break;
      }
    }
  };

  // Save assistant message to history
  const saveAssistantMessage = async (msg: ChatMessage) => {
    const ctx = context();
    const chunks = [
      ...(msg.thinking ? [{ type: 'thinking', content: msg.thinking }] : []),
      { type: 'text', content: msg.content },
      ...(msg.toolCalls || []).map((t) => ({ type: 'tool', ...t })),
    ];
    const chunksJson = JSON.stringify(chunks);

    try {
      await invoke('save_gyp_message', {
        scope: contextToScope(ctx),
        role: 'assistant',
        chunksJson,
      });
    } catch (e) {
      console.error('[gyp-chat] Failed to save message:', e);
    }
  };

  // Connect to Gyp
  const connect = async () => {
    if (connected() || connecting()) return;

    setConnecting(true);
    const ctx = context();

    try {
      // Subscribe to unified event channel
      unlisten = await listen<[string, ChatEvent]>('gyp-event', (event) => {
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
      const response = await invoke<StartGypSessionResponse>('start_gyp_session', { request });

      setSessionId(response.sessionId);
      setCurrentScope(response.scope);
      setConnected(true);
    } catch (e) {
      console.error('[gyp-chat] Failed to connect:', e);
      window.toast?.error('Failed to connect to Gyp');
      unlisten?.();
      unlisten = undefined;
    } finally {
      setConnecting(false);
    }
  };

  // Load chat history
  const loadHistory = async (ctx: GypChatContext) => {
    try {
      const scope = contextToScope(ctx);
      const history = await invoke<Array<{ role: string; chunksJson: string }>>('get_gyp_history', {
        scope,
        limit: historyDepth,
      });

      if (history && history.length > 0) {
        const loadedMessages: ChatMessage[] = history.map((msg, idx) => {
          const chunksStr = msg.chunksJson || '[]';
          return {
            id: `history-${idx}`,
            role: msg.role as 'user' | 'assistant',
            content: parseChunksContent(chunksStr),
            thinking: parseChunksThinking(chunksStr),
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
      console.error('[gyp-chat] Failed to load history:', e);
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
        await invoke('stop_gyp_session', { sessionId: sid });
      } catch (e) {
        console.error('[gyp-chat] Failed to stop session:', e);
      }
    }
    unlisten?.();
    unlisten = undefined;
    setConnected(false);
    setSessionId(null);
    setCurrentMessage(null);
    setGypEditing(false);
    setEditingIslands(new Set<string>());
    toolsById.clear();
    lastChunkType = null;
    // Clear messages to prevent stale UI during project transitions
    setMessages([]);
  };

  // Send message
  const sendMessage = async (content: string) => {
    const sid = sessionId();
    const ctx = context();

    if (!sid || !connected() || !content.trim()) {
      window.toast?.error('Not connected to Gyp');
      return;
    }

    // Add user message to UI
    const userMessage: ChatMessage = {
      id: `user-${Date.now()}`,
      role: 'user',
      content: content.trim(),
      timestamp: new Date(),
    };
    setMessages(produce((msgs) => msgs.push(userMessage)));

    // Initialize streaming state
    setCurrentMessage({ id: `msg-${Date.now()}`, role: 'assistant', streaming: true });

    try {
      const scope = contextToScope(ctx);
      const focus: TaskFocus | undefined =
        ctx.focusNodeId && ctx.focusNodeName
          ? { taskId: ctx.focusNodeId, taskName: ctx.focusNodeName }
          : undefined;

      await invoke('send_gyp_message', {
        sessionId: sid,
        content: content.trim(),
        scope,
        focus,
      });
    } catch (e) {
      console.error('[gyp-chat] Failed to send message:', e);
      window.toast?.error('Failed to send message');
      setCurrentMessage(null);
    }
  };

  // Respond to permission request
  const respondToPermission = async (optionId: string) => {
    const sid = sessionId();
    const permission = pendingPermission();
    if (!sid || !permission) return;

    try {
      await invoke('respond_chat_permission', {
        sessionId: sid,
        requestId: permission.requestId,
        optionId,
      });
      setPendingPermission(null);
    } catch (e) {
      console.error('[gyp-chat] Failed to respond to permission:', e);
    }
  };

  // Clear history
  const clearHistory = async () => {
    const ctx = context();
    try {
      const scope = contextToScope(ctx);
      await invoke('clear_gyp_history', { scope });
      setMessages([
        {
          id: 'welcome',
          role: 'assistant',
          content: WELCOME_MESSAGE,
          timestamp: new Date(),
        },
      ]);
    } catch (e) {
      console.error('[gyp-chat] Failed to clear history:', e);
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
    sessionId,
    connect,
    disconnect,
    messages,
    currentMessage,
    sendMessage,
    context,
    setFocusNode,
    gypEditing,
    editingIslands,
    pendingPermission,
    respondToPermission,
    clearHistory,
    reset,
  };
}
