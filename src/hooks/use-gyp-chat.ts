/**
 * Unified Gyp Chat Hook
 *
 * Manages Gyp chat sessions across all contexts (board, run, draft, general).
 * Merges DirectChat and useBoardChat functionality into a single hook.
 */
import { invoke } from '@tauri-apps/api/core';
import { type UnlistenFn, listen } from '@tauri-apps/api/event';
import { createEffect, createSignal, onCleanup } from 'solid-js';
import { createStore, produce } from 'solid-js/store';
import type {
  ChatEvent,
  ChatMessage,
  ChatToolCall,
  PendingPermission,
  UIContext,
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

const SYSTEM_PROMPT = `You are Gyp, an AI assistant for Hirsel.

RULES:
1. NEVER use "hirsel" CLI commands - they will fail
2. Use hirsel MCP tools for tasks: task_list, task_add, task_done, msg_send, msg_read
3. To edit spec/eval, use Read/Edit/Write on the paths provided in <ui-context>

The <ui-context> block contains:
- specFile: exact path to spec.md (use this for Read/Edit)
- evalFile: exact path to eval.md
- currentSpec: current spec contents

Be concise.`;

export function useGypChat(
  getContext: () => GypChatContext,
  options: UseGypChatOptions = {},
): UseGypChatReturn {
  const { historyDepth = 20, onEditComplete } = options;

  // Connection state
  const [sessionId, setSessionId] = createSignal<string | null>(null);
  const [connected, setConnected] = createSignal(false);
  const [connecting, setConnecting] = createSignal(false);

  // Message state
  const [messages, setMessages] = createStore<ChatMessage[]>([]);
  const [currentMessage, setCurrentMessage] = createSignal<Partial<ChatMessage> | null>(null);

  // Editing state
  const [gypEditing, setGypEditing] = createSignal(false);
  const [editingIslands, setEditingIslands] = createSignal<Set<string>>(new Set());

  // Permission state
  const [pendingPermission, setPendingPermission] = createSignal<PendingPermission | null>(null);

  // Focus node (for board context)
  const [focusNodeId, setFocusNodeId] = createSignal<string | null>(null);
  const [focusNodeName, setFocusNodeName] = createSignal<string | null>(null);

  // Track tool calls during streaming
  const toolsById = new Map<string, ChatToolCall>();
  let lastChunkType: 'text' | 'thinking' | null = null;
  let unlisten: UnlistenFn | undefined;

  // Build context with focus node
  const context = () => {
    const baseContext = getContext();
    return {
      ...baseContext,
      focusNodeId: focusNodeId() ?? undefined,
      focusNodeName: focusNodeName() ?? undefined,
    };
  };

  const setFocusNode = (id: string | null, name: string | null) => {
    setFocusNodeId(id);
    setFocusNodeName(name);
  };

  // Handle chat events
  const handleChatEvent = (event: ChatEvent) => {
    if (event.sessionId !== sessionId()) return;

    switch (event.type) {
      case 'textDelta':
        handleTextDelta(event.text);
        break;
      case 'thinkingDelta':
        handleThinkingDelta(event.text);
        break;
      case 'toolCallStart':
        handleToolCallStart(event.toolCallId, event.title, event.kind, event.input);
        break;
      case 'toolCallUpdate':
        handleToolCallUpdate(event.toolCallId, event.status, event.title, event.output);
        break;
      case 'permissionRequest':
        setPendingPermission(event.request);
        break;
      case 'messageComplete':
        handleMessageComplete();
        break;
      case 'error':
        handleError(event.message);
        break;
      case 'sessionEnded':
        handleSessionEnded();
        break;
    }
  };

  const handleTextDelta = (text: string) => {
    setCurrentMessage((prev) => {
      const updated = prev ? { ...prev } : { role: 'assistant' as const, content: '' };
      if (lastChunkType !== 'text') {
        lastChunkType = 'text';
      }
      updated.content = (updated.content || '') + text;
      updated.streaming = true;
      return updated;
    });
  };

  const handleThinkingDelta = (text: string) => {
    setCurrentMessage((prev) => {
      const updated = prev ? { ...prev } : { role: 'assistant' as const, content: '' };
      if (lastChunkType !== 'thinking') {
        lastChunkType = 'thinking';
      }
      updated.thinking = (updated.thinking || '') + text;
      updated.streaming = true;
      return updated;
    });
  };

  const handleToolCallStart = (
    toolCallId: string,
    title: string,
    kind: string | null,
    input: string | null,
  ) => {
    lastChunkType = null;

    // Skip duplicates
    if (toolsById.has(toolCallId)) {
      const existing = toolsById.get(toolCallId)!;
      if (title) existing.title = title;
      if (input) existing.input = input;
      setCurrentMessage((prev) => ({ ...prev }));
      return;
    }

    const toolCall: ChatToolCall = {
      id: toolCallId,
      title,
      kind,
      status: 'in_progress',
      input,
      output: null,
    };
    toolsById.set(toolCallId, toolCall);

    // Track if this is a board file edit (board.json or any file in board directory)
    if (kind === 'write' || kind === 'edit') {
      const inputStr = input || '';
      if (inputStr.includes('/board/')) {
        setGypEditing(true);
        // Track specific files being edited
        const match = inputStr.match(/\/board\/([^/]+)\.(md|json)/);
        if (match) {
          setEditingIslands((prev) => new Set([...prev, match[1]]));
        }
      }
    }

    setCurrentMessage((prev) => ({
      ...(prev || { role: 'assistant' as const, content: '' }),
      toolCalls: Array.from(toolsById.values()),
      streaming: true,
    }));
  };

  const handleToolCallUpdate = (
    toolCallId: string,
    status: string,
    title: string | null,
    output: string | null,
  ) => {
    const existing = toolsById.get(toolCallId);
    if (existing) {
      existing.status = status;
      if (title) existing.title = title;
      if (output) existing.output = output;
      toolsById.set(toolCallId, existing);
    }

    setCurrentMessage((prev) => ({
      ...(prev || { role: 'assistant' as const, content: '' }),
      toolCalls: Array.from(toolsById.values()),
    }));
  };

  const handleMessageComplete = () => {
    const msg = currentMessage();
    if (msg && (msg.content || msg.toolCalls?.length)) {
      const finalMessage: ChatMessage = {
        id: msg.id || `msg-${Date.now()}`,
        role: msg.role || 'assistant',
        content: msg.content || '',
        thinking: msg.thinking,
        toolCalls: msg.toolCalls,
        timestamp: new Date(),
        streaming: false,
      };

      setMessages(produce((draft) => draft.push(finalMessage)));
      saveMessage('assistant', finalMessage);
    }

    setCurrentMessage(null);
    toolsById.clear();
    lastChunkType = null;

    // Clear editing state and trigger refresh
    if (gypEditing()) {
      setGypEditing(false);
      setEditingIslands(new Set<string>());
      onEditComplete?.();
    }

    // Refresh draft if we edited spec/eval files
    checkAndRefreshDraft();
  };

  const handleError = (message: string) => {
    console.error('[gyp-chat] Error:', message);
    window.toast?.error(`Gyp error: ${message}`);
    setGypEditing(false);
    setEditingIslands(new Set<string>());
    setCurrentMessage(null);
  };

  const handleSessionEnded = () => {
    setConnected(false);
    setSessionId(null);
    setGypEditing(false);
    setEditingIslands(new Set<string>());
    setCurrentMessage(null);
    // Auto-reconnect after a delay
    setTimeout(() => connect(), 1000);
  };

  // Refresh draft if we edited spec/eval files
  const checkAndRefreshDraft = () => {
    const ctx = context();
    if (ctx.type === 'run' || ctx.type === 'draft') {
      window.dispatchEvent(new CustomEvent('draft-refresh', { detail: ctx.runName }));
    }
  };

  // Connect to chat session
  const connect = async () => {
    if (connecting() || connected()) return;

    const ctx = context();
    setConnecting(true);

    try {
      // Subscribe to events
      const eventChannel = ctx.type === 'board' ? 'board-chat-event' : 'chat-event';
      unlisten = await listen<ChatEvent>(eventChannel, (event) => {
        handleChatEvent(event.payload);
      });

      // Load history
      await loadHistory(ctx);

      // Start session based on context type
      let sid: string;
      if (ctx.type === 'board' && ctx.projectId) {
        sid = await invoke<string>('start_board_chat_session', {
          projectId: ctx.projectId,
        });
      } else {
        const runName = ctx.type === 'run' || ctx.type === 'draft' ? ctx.runName : undefined;
        sid = await invoke<string>('start_chat_session', {
          agentCommand: ['hirsel', '__acp-bridge'],
          workingDir: undefined,
          runName,
          systemPrompt: SYSTEM_PROMPT,
          profile: null,
        });
      }

      setSessionId(sid);
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
      let history: Array<{ role: string; chunks?: string; chunksJson?: string }> = [];

      if (ctx.type === 'board' && ctx.projectId) {
        history = await invoke<Array<{ role: string; chunksJson: string }>>(
          'get_board_chat_history',
          { projectId: ctx.projectId, limit: historyDepth },
        );
      } else {
        const runName = ctx.type === 'run' || ctx.type === 'draft' ? ctx.runName : undefined;
        history = await invoke<Array<{ role: string; chunks: string }>>('get_gyp_chat_history', {
          runName,
        });
      }

      if (history && history.length > 0) {
        const loadedMessages: ChatMessage[] = history.map((msg, idx) => {
          const chunksStr = msg.chunks || msg.chunksJson || '[]';
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
      console.warn('[gyp-chat] Failed to load history:', e);
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

  // Disconnect from chat session
  const disconnect = async () => {
    if (unlisten) {
      unlisten();
      unlisten = undefined;
    }

    const sid = sessionId();
    if (sid) {
      try {
        await invoke('stop_chat_session', { sessionId: sid, profile: null });
      } catch {
        // Ignore errors
      }
    }

    setConnected(false);
    setSessionId(null);
    setCurrentMessage(null);
    setGypEditing(false);
    setEditingIslands(new Set<string>());
    toolsById.clear();
    lastChunkType = null;
  };

  // Send a message
  const sendMessage = async (content: string) => {
    const sid = sessionId();
    const ctx = context();

    if (!sid || !connected() || !content.trim()) {
      window.toast?.error('Not connected to Gyp');
      return;
    }

    // Add user message to UI
    const userMessage: ChatMessage = {
      id: `msg-${Date.now()}`,
      role: 'user',
      content,
      timestamp: new Date(),
    };
    setMessages(produce((draft) => draft.push(userMessage)));

    // Save user message
    saveMessage('user', userMessage);

    // Start assistant message placeholder
    setCurrentMessage({ role: 'assistant', content: '', streaming: true });

    try {
      if (ctx.type === 'board' && ctx.projectId) {
        await invoke('send_board_chat_message', {
          projectId: ctx.projectId,
          sessionId: sid,
          content,
          focusTaskId: focusNodeId(),
          focusTaskName: focusNodeName(),
        });
      } else {
        await invoke('send_chat_message', {
          sessionId: sid,
          content,
          context: buildUIContext(ctx),
          profile: null,
        });
      }
    } catch (e) {
      console.error('[gyp-chat] Failed to send message:', e);
      window.toast?.error('Failed to send message');
      setCurrentMessage(null);
    }
  };

  // Build UI context for regular chat
  const buildUIContext = (ctx: GypChatContext): UIContext => {
    return {
      selectedRun: ctx.runName ?? null,
      selectedWorker: null,
      uiSection: ctx.type,
      extra: ctx.projectId ? { projectId: String(ctx.projectId) } : undefined,
    };
  };

  // Save message to history
  const saveMessage = async (role: string, msg: ChatMessage) => {
    const ctx = context();
    try {
      const chunks = buildChunksJson(msg);

      if (ctx.type === 'board' && ctx.projectId) {
        await invoke('save_board_chat_message', {
          projectId: ctx.projectId,
          role,
          chunksJson: chunks,
        });
      } else {
        const runName = ctx.type === 'run' || ctx.type === 'draft' ? ctx.runName : undefined;
        await invoke('save_gyp_message', {
          runName,
          role,
          chunksJson: chunks,
        });
      }
    } catch (e) {
      console.error('[gyp-chat] Failed to save message:', e);
    }
  };

  // Respond to permission request
  const respondToPermission = async (optionId: string) => {
    const permission = pendingPermission();
    const sid = sessionId();
    if (!permission || !sid) return;

    try {
      await invoke('respond_chat_permission', {
        sessionId: sid,
        requestId: permission.requestId,
        optionId,
        profile: null,
      });
    } catch (e) {
      console.error('[gyp-chat] Failed to respond to permission:', e);
      window.toast?.error(`Failed to respond: ${e}`);
    } finally {
      setPendingPermission(null);
    }
  };

  // Clear chat history
  const clearHistory = async () => {
    const ctx = context();
    try {
      if (ctx.type === 'board' && ctx.projectId) {
        await invoke('clear_board_chat_history', { projectId: ctx.projectId });
      }
      // For non-board contexts, history is file-based and we just clear locally
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

  // Reset: disconnect, clear history, and reconnect for a fresh session
  const reset = async () => {
    await disconnect();
    await clearHistory();
    // Small delay to ensure clean state
    await new Promise((resolve) => setTimeout(resolve, 100));
    await connect();
  };

  // Cleanup on unmount
  createEffect(() => {
    onCleanup(() => {
      disconnect();
    });
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

// Helper functions for parsing message chunks
function buildChunksJson(msg: ChatMessage): string {
  const chunks: Array<{ type: string; content?: string; tool?: ChatToolCall }> = [];

  if (msg.thinking) {
    chunks.push({ type: 'thinking', content: msg.thinking });
  }
  if (msg.content) {
    chunks.push({ type: 'text', content: msg.content });
  }
  if (msg.toolCalls) {
    for (const tc of msg.toolCalls) {
      chunks.push({ type: 'tool', tool: tc });
    }
  }

  return JSON.stringify(chunks);
}

function parseChunksContent(chunksJson: string): string {
  try {
    const chunks = JSON.parse(chunksJson);
    return chunks
      .filter((c: { type: string }) => c.type === 'text')
      .map((c: { content?: string }) => c.content || '')
      .join('');
  } catch {
    return '';
  }
}

function parseChunksThinking(chunksJson: string): string | undefined {
  try {
    const chunks = JSON.parse(chunksJson);
    const thinking = chunks
      .filter((c: { type: string }) => c.type === 'thinking')
      .map((c: { content?: string }) => c.content || '')
      .join('');
    return thinking || undefined;
  } catch {
    return undefined;
  }
}

function parseChunksToolCalls(chunksJson: string): ChatToolCall[] | undefined {
  try {
    const chunks = JSON.parse(chunksJson);
    const toolCalls = chunks
      .filter((c: { type: string }) => c.type === 'tool')
      .map((c: { tool?: ChatToolCall }) => c.tool)
      .filter(Boolean);
    return toolCalls.length > 0 ? toolCalls : undefined;
  } catch {
    return undefined;
  }
}
