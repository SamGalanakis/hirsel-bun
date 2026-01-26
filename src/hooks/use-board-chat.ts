/**
 * Board Chat Hook
 *
 * Manages Gyp chat sessions for the SpecFlow board.
 * Handles connection, message streaming, and tracking which islands are being edited.
 */
import { invoke } from '@tauri-apps/api/core';
import { type UnlistenFn, listen } from '@tauri-apps/api/event';
import { createEffect, createSignal, onCleanup } from 'solid-js';
import { createStore, produce } from 'solid-js/store';
import type { ChatEvent, ChatMessage, ChatToolCall } from '../lib/types';

interface UseBoardChatOptions {
  /** Number of messages to keep in history (default: 10) */
  historyDepth?: number;
  /** Callback when Gyp finishes editing (for triggering board refresh) */
  onEditComplete?: () => void;
}

interface UseBoardChatReturn {
  // State
  connected: () => boolean;
  connecting: () => boolean;
  sessionId: () => string | null;
  messages: ChatMessage[];
  currentMessage: () => Partial<ChatMessage> | null;
  gypEditing: () => boolean;
  editingIslands: () => Set<string>;

  // Actions
  connect: () => Promise<void>;
  disconnect: () => Promise<void>;
  sendMessage: (content: string, focusIslandId?: string, focusIslandName?: string) => Promise<void>;
  clearHistory: () => Promise<void>;
}

export function useBoardChat(
  projectId: () => number | null,
  options: UseBoardChatOptions = {},
): UseBoardChatReturn {
  const { historyDepth = 10, onEditComplete } = options;

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

  // Track tool calls during streaming
  const toolsById = new Map<string, ChatToolCall>();
  let lastChunkType: 'text' | 'thinking' | null = null;
  let unlisten: UnlistenFn | undefined;

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
      return updated;
    });
  };

  const handleToolCallStart = (
    toolCallId: string,
    title: string,
    kind: string | null,
    input: string | null,
  ) => {
    const toolCall: ChatToolCall = {
      id: toolCallId,
      title,
      kind,
      status: 'in_progress',
      input,
      output: null,
    };
    toolsById.set(toolCallId, toolCall);

    // Track if this is a board file edit
    if (kind === 'write' || kind === 'edit') {
      const inputStr = input || '';
      // Check if editing a board markdown file
      if (inputStr.includes('/board/') && inputStr.includes('.md')) {
        setGypEditing(true);
        // Try to extract island name from the file path
        const match = inputStr.match(/\/board\/([^/]+)\.md/);
        if (match) {
          setEditingIslands((prev) => new Set([...prev, match[1]]));
        }
      }
    }

    setCurrentMessage((prev) => ({
      ...(prev || { role: 'assistant' as const, content: '' }),
      toolCalls: Array.from(toolsById.values()),
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
    if (msg) {
      const finalMessage: ChatMessage = {
        id: crypto.randomUUID(),
        role: msg.role || 'assistant',
        content: msg.content || '',
        thinking: msg.thinking,
        toolCalls: msg.toolCalls,
        timestamp: new Date(),
      };

      setMessages(produce((draft) => draft.push(finalMessage)));

      // Save to history
      const pid = projectId();
      if (pid) {
        const chunks = [{ type: 'text', content: finalMessage.content }];
        if (finalMessage.thinking) {
          chunks.unshift({ type: 'thinking', content: finalMessage.thinking });
        }
        invoke('save_board_chat_message', {
          projectId: pid,
          role: 'assistant',
          chunksJson: JSON.stringify(chunks),
        }).catch(console.error);
      }
    }

    // Reset state
    setCurrentMessage(null);
    toolsById.clear();
    lastChunkType = null;

    // Clear editing state and trigger refresh
    if (gypEditing()) {
      setGypEditing(false);
      setEditingIslands(new Set<string>());
      onEditComplete?.();
    }
  };

  const handleError = (message: string) => {
    console.error('[board-chat] Error:', message);
    window.toast?.error(`Gyp error: ${message}`);
    setGypEditing(false);
    setEditingIslands(new Set<string>());
  };

  const handleSessionEnded = () => {
    setConnected(false);
    setSessionId(null);
    setGypEditing(false);
    setEditingIslands(new Set<string>());
  };

  // Connect to board chat session
  const connect = async () => {
    const pid = projectId();
    if (!pid || connecting() || connected()) return;

    setConnecting(true);
    try {
      // Subscribe to events first
      unlisten = await listen<ChatEvent>('board-chat-event', (event) => {
        handleChatEvent(event.payload);
      });

      // Start the session
      const id = await invoke<string>('start_board_chat_session', {
        projectId: pid,
      });

      setSessionId(id);
      setConnected(true);

      // Load history
      const history = await invoke<Array<{ role: string; chunksJson: string }>>(
        'get_board_chat_history',
        {
          projectId: pid,
          limit: historyDepth,
        },
      );

      if (history.length > 0) {
        const loadedMessages: ChatMessage[] = history.map((msg) => {
          const chunks = JSON.parse(msg.chunksJson);
          const textChunk = chunks.find((c: { type: string }) => c.type === 'text');
          const thinkingChunk = chunks.find((c: { type: string }) => c.type === 'thinking');
          return {
            id: crypto.randomUUID(),
            role: msg.role as 'user' | 'assistant',
            content: textChunk?.content || '',
            thinking: thinkingChunk?.content,
            timestamp: new Date(),
          };
        });
        setMessages(loadedMessages);
      }
    } catch (e) {
      console.error('[board-chat] Failed to connect:', e);
      window.toast?.error('Failed to connect to Gyp');
      unlisten?.();
      unlisten = undefined;
    } finally {
      setConnecting(false);
    }
  };

  // Disconnect from board chat session
  const disconnect = async () => {
    if (unlisten) {
      unlisten();
      unlisten = undefined;
    }

    const sid = sessionId();
    if (sid) {
      try {
        await invoke('stop_chat_session', { sessionId: sid });
      } catch (e) {
        console.error('[board-chat] Failed to stop session:', e);
      }
    }

    setConnected(false);
    setSessionId(null);
    setCurrentMessage(null);
    setGypEditing(false);
    setEditingIslands(new Set<string>());
    toolsById.clear();
  };

  // Send a message
  const sendMessage = async (content: string, focusIslandId?: string, focusIslandName?: string) => {
    const pid = projectId();
    const sid = sessionId();
    console.log('[board-chat] sendMessage called:', {
      pid,
      sid,
      connected: connected(),
      content: content.slice(0, 30),
    });
    if (!pid || !sid || !connected()) {
      console.log('[board-chat] Not connected, aborting send');
      window.toast?.error('Not connected to Gyp');
      return;
    }

    // Add user message to UI
    const userMessage: ChatMessage = {
      id: crypto.randomUUID(),
      role: 'user',
      content,
      timestamp: new Date(),
    };
    setMessages(produce((draft) => draft.push(userMessage)));

    // Start assistant message placeholder
    setCurrentMessage({ role: 'assistant', content: '', streaming: true });

    try {
      console.log('[board-chat] Invoking send_board_chat_message...');
      await invoke('send_board_chat_message', {
        projectId: pid,
        sessionId: sid,
        content,
        focusIslandId,
        focusIslandName,
      });
      console.log('[board-chat] Invoke completed');
    } catch (e) {
      console.error('[board-chat] Failed to send message:', e);
      window.toast?.error('Failed to send message');
      setCurrentMessage(null);
    }
  };

  // Clear chat history
  const clearHistory = async () => {
    const pid = projectId();
    if (!pid) return;

    try {
      await invoke('clear_board_chat_history', { projectId: pid });
      setMessages([]);
    } catch (e) {
      console.error('[board-chat] Failed to clear history:', e);
    }
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
    messages,
    currentMessage,
    gypEditing,
    editingIslands,
    connect,
    disconnect,
    sendMessage,
    clearHistory,
  };
}
