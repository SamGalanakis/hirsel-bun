/**
 * Direct AI chat sidebar with streaming messages and permission handling
 */
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  type Component,
  For,
  Show,
  createEffect,
  createSignal,
  onCleanup,
} from 'solid-js';
import { createStore, produce } from 'solid-js/store';
import type {
  ChatEvent,
  ChatMessage,
  ChatToolCall,
  PendingPermission,
  UIContext,
} from '../../lib/types';
import { useApp, useRuns } from '../../stores';

const WELCOME_MESSAGE = `Hello! I'm Gyp, your AI assistant for Hirsel. I can help you manage runs, tasks, and workers using the hirsel tools.

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

export const DirectChat: Component = () => {
  const app = useApp();
  const runs = useRuns();

  const [sessionId, setSessionId] = createSignal<string | null>(null);
  const [connected, setConnected] = createSignal(false);
  const [connecting, setConnecting] = createSignal(false);
  const [messages, setMessages] = createStore<ChatMessage[]>([]);
  const [currentMessage, setCurrentMessage] = createSignal<Partial<ChatMessage> | null>(null);
  const [pendingPermission, setPendingPermission] = createSignal<PendingPermission | null>(null);
  const [inputText, setInputText] = createSignal('');
  const [sending, setSending] = createSignal(false);
  const [useRunContext, setUseRunContext] = createSignal(true);
  const [expandedTools, setExpandedTools] = createSignal<Set<string>>(new Set());

  // Track current run name for context
  let currentRunName: string | null = null;
  let messagesContainerRef: HTMLDivElement | undefined;
  let unlisten: UnlistenFn | undefined;
  let lastChunkType: 'text' | 'thinking' | null = null;
  const toolsById = new Map<string, ChatToolCall>();

  // Check if run has a workspace (projectPath)
  const canUseRunContext = () => {
    const detail = runs.runDetail();
    return Boolean(runs.selectedRun() && detail?.projectPath);
  };

  // Get effective run name based on toggle and availability
  const getEffectiveRunName = () => {
    if (!runs.selectedRun() || !useRunContext() || !canUseRunContext()) {
      return null;
    }
    return runs.selectedRun();
  };

  // Build rich UI context for the AI
  const getUIContext = (): UIContext => {
    const detail = runs.runDetail();

    const context: UIContext = {
      selectedRun: runs.selectedRun(),
      selectedWorker: null,
      uiSection: 'chat',
    };

    // Include file paths when a run with projectPath is selected
    if (detail?.projectPath) {
      context.extra = {
        projectPath: detail.projectPath,
        specFile: `${detail.projectPath}/spec.md`,
        evalFile: `${detail.projectPath}/eval.md`,
        runStatus: detail.status,
      };
      if (detail.request) {
        context.extra.currentSpec = detail.request;
      }
    }

    return context;
  };

  // Auto-scroll to bottom
  const scrollToBottom = () => {
    setTimeout(() => {
      messagesContainerRef?.scrollTo({ top: messagesContainerRef.scrollHeight });
    }, 50);
  };

  // Connect on mount when chat opens
  createEffect(() => {
    if (app.aiChatOpen()) {
      connect();
    }
  });

  // Cleanup on unmount
  onCleanup(() => {
    disconnect();
  });

  // Auto-scroll on new messages
  createEffect(() => {
    const _ = messages.length;
    const __ = currentMessage();
    scrollToBottom();
  });

  const connect = async () => {
    if (connecting() || connected()) return;

    setConnecting(true);

    try {
      // Determine context
      const runName = getEffectiveRunName();
      currentRunName = runName;

      // Load chat history for this context
      let loadedHistory = false;
      try {
        const history = await invoke<Array<{ role: string; chunks: string }>>('get_gyp_chat_history', {
          runName: runName || undefined,
        });

        if (history && history.length > 0) {
          const loadedMessages: ChatMessage[] = history.map((msg, idx) => ({
            id: `history-${idx}`,
            role: msg.role as 'user' | 'assistant',
            content: parseChunksContent(msg.chunks),
            thinking: parseChunksThinking(msg.chunks),
            toolCalls: parseChunksToolCalls(msg.chunks),
            timestamp: new Date(),
          }));
          setMessages(loadedMessages);
          loadedHistory = true;
        }
      } catch (e) {
        console.warn('[DirectChat] Failed to load chat history:', e);
      }

      // Add welcome message if no history
      if (!loadedHistory) {
        setMessages([
          {
            id: 'welcome',
            role: 'assistant',
            content: WELCOME_MESSAGE,
            timestamp: new Date(),
          },
        ]);
      }

      // Subscribe to chat events
      unlisten = await listen<ChatEvent>('chat-event', (event) => {
        handleChatEvent(event.payload);
      });

      // Get project path for working directory
      const detail = runs.runDetail();
      const projectPath = detail?.projectPath || undefined;

      // Start chat session - use hirsel __acp-bridge which bridges to Claude CLI
      const sid = await invoke<string>('start_chat_session', {
        agentCommand: ['hirsel', '__acp-bridge'],
        workingDir: projectPath,
        runName: runs.selectedRun() || undefined,
        systemPrompt: SYSTEM_PROMPT,
        profile: null,
      });

      setSessionId(sid);
      setConnected(true);
      scrollToBottom();
    } catch (e) {
      console.error('Failed to connect chat:', e);
      window.toast?.error(`Failed to connect: ${e}`);
    } finally {
      setConnecting(false);
    }
  };

  const disconnect = async () => {
    const sid = sessionId();
    if (sid) {
      try {
        await invoke('stop_chat_session', { sessionId: sid, profile: null });
      } catch {
        // Ignore errors
      }
    }

    if (unlisten) {
      unlisten();
      unlisten = undefined;
    }

    setSessionId(null);
    setConnected(false);
    currentRunName = null;
    lastChunkType = null;
    toolsById.clear();
  };

  // Toggle between run-specific and general chat
  const toggleContext = async () => {
    setUseRunContext(!useRunContext());
    // Reconnect with new context
    await disconnect();
    setMessages([]);
    await connect();
  };

  const handleChatEvent = (event: ChatEvent) => {
    // Ignore events for other sessions
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
        finalizeCurrentMessage();
        checkAndRefreshDraft();
        break;

      case 'error':
        console.error('Chat error:', event.message);
        window.toast?.error(`Chat error: ${event.message}`);
        finalizeCurrentMessage();
        break;

      case 'sessionEnded':
        setConnected(false);
        finalizeCurrentMessage();
        setTimeout(() => connect(), 1000);
        break;
    }
  };

  const handleTextDelta = (text: string) => {
    setCurrentMessage((prev) => {
      const newContent = (prev?.content || '') + text;
      lastChunkType = 'text';
      return {
        ...prev,
        id: prev?.id || `msg-${Date.now()}`,
        role: 'assistant',
        content: newContent,
        streaming: true,
        timestamp: prev?.timestamp || new Date(),
      };
    });
    scrollToBottom();
  };

  const handleThinkingDelta = (text: string) => {
    setCurrentMessage((prev) => {
      lastChunkType = 'thinking';
      return {
        ...prev,
        id: prev?.id || `msg-${Date.now()}`,
        role: 'assistant',
        thinking: (prev?.thinking || '') + text,
        streaming: true,
        timestamp: prev?.timestamp || new Date(),
      };
    });
  };

  const handleToolCallStart = (id: string, title: string, kind: string | null, input: string | null) => {
    lastChunkType = null;

    // Skip duplicates
    if (toolsById.has(id)) {
      const existing = toolsById.get(id)!;
      if (title) existing.title = title;
      if (input) existing.input = input;
      setCurrentMessage((prev) => ({ ...prev })); // Trigger re-render
      return;
    }

    const toolCall: ChatToolCall = {
      id,
      title,
      kind,
      status: 'in_progress',
      input,
      output: null,
    };
    toolsById.set(id, toolCall);

    setCurrentMessage((prev) => {
      const toolCalls = prev?.toolCalls || [];
      return {
        ...prev,
        id: prev?.id || `msg-${Date.now()}`,
        role: 'assistant',
        toolCalls: [...toolCalls, toolCall],
        streaming: true,
        timestamp: prev?.timestamp || new Date(),
      };
    });
    scrollToBottom();
  };

  const handleToolCallUpdate = (id: string, status: string, title: string | null, output: string | null) => {
    const tool = toolsById.get(id);
    if (tool) {
      tool.status = status;
      if (title) tool.title = title;
      if (output) tool.output = output;
    }

    setCurrentMessage((prev) => {
      if (!prev?.toolCalls) return prev;
      const toolCalls = prev.toolCalls.map((tc) =>
        tc.id === id
          ? { ...tc, status, title: title || tc.title, output: output || tc.output }
          : tc
      );
      return { ...prev, toolCalls };
    });
  };

  const finalizeCurrentMessage = () => {
    const msg = currentMessage();
    if (msg && (msg.content || msg.toolCalls?.length)) {
      const finalMsg: ChatMessage = {
        id: msg.id || `msg-${Date.now()}`,
        role: 'assistant',
        content: msg.content || '',
        thinking: msg.thinking,
        toolCalls: msg.toolCalls,
        timestamp: msg.timestamp || new Date(),
        streaming: false,
      };
      setMessages(produce((draft) => {
        draft.push(finalMsg);
      }));

      // Save to history
      saveMessage('assistant', finalMsg);
    }

    setCurrentMessage(null);
    lastChunkType = null;
    toolsById.clear();
  };

  // Refresh draft if we edited spec/eval files
  const checkAndRefreshDraft = () => {
    const detail = runs.runDetail();
    if (detail?.status === 'draft' && runs.selectedRun()) {
      window.dispatchEvent(
        new CustomEvent('draft-refresh', {
          detail: runs.selectedRun(),
        })
      );
    }
  };

  const sendMessage = async () => {
    const text = inputText().trim();
    const sid = sessionId();
    if (!text || !sid || sending()) return;

    setSending(true);

    // Add user message to list
    const userMsg: ChatMessage = {
      id: `msg-${Date.now()}`,
      role: 'user',
      content: text,
      timestamp: new Date(),
    };
    setMessages(produce((draft) => {
      draft.push(userMsg);
    }));
    setInputText('');
    scrollToBottom();

    // Save to history
    saveMessage('user', userMsg);

    try {
      await invoke('send_chat_message', {
        sessionId: sid,
        content: text,
        context: getUIContext(),
        profile: null,
      });
    } catch (e) {
      console.error('Failed to send message:', e);
      window.toast?.error(`Failed to send: ${e}`);
      setInputText(text); // Restore on error
    } finally {
      setSending(false);
    }
  };

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
      console.error('Failed to respond to permission:', e);
      window.toast?.error(`Failed to respond: ${e}`);
    } finally {
      setPendingPermission(null);
    }
  };

  const saveMessage = async (role: string, msg: ChatMessage) => {
    try {
      const chunks = buildChunksJson(msg);
      await invoke('save_gyp_message', {
        runName: currentRunName || undefined,
        role,
        chunksJson: chunks,
      });
    } catch (e) {
      console.error('Failed to save message:', e);
    }
  };

  const buildChunksJson = (msg: ChatMessage): string => {
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
  };

  const parseChunksContent = (chunksJson: string): string => {
    try {
      const chunks = JSON.parse(chunksJson);
      return chunks
        .filter((c: { type: string }) => c.type === 'text')
        .map((c: { content?: string }) => c.content || '')
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
        .map((c: { content?: string }) => c.content || '')
        .join('');
      return thinking || undefined;
    } catch {
      return undefined;
    }
  };

  const parseChunksToolCalls = (chunksJson: string): ChatToolCall[] | undefined => {
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
  };

  const toggleTool = (toolId: string) => {
    setExpandedTools((prev) => {
      const next = new Set(prev);
      if (next.has(toolId)) {
        next.delete(toolId);
      } else {
        next.add(toolId);
      }
      return next;
    });
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  };

  return (
    <aside class="ai-chat-panel w-96 flex-shrink-0 border-l border-pasture-600 flex flex-col bg-pasture-800">
      {/* Header */}
      <div class="p-3 border-b border-pasture-600 flex items-center justify-between shrink-0">
        <div class="flex items-center gap-2">
          <img src="/gyp.svg" class="size-10 shrink-0" alt="Gyp" />
          <div class="flex flex-col">
            <div class="flex items-center gap-2">
              <h2 class="text-sm font-medium text-wool-300">Gyp</h2>
              <Show when={connected()}>
                <span class="w-2 h-2 rounded-full bg-sage" data-tooltip="Connected" />
              </Show>
              <Show when={!connected() && !connecting()}>
                <span class="w-2 h-2 rounded-full bg-wool-600" data-tooltip="Disconnected" />
              </Show>
              <Show when={connecting()}>
                <span class="w-2 h-2 rounded-full bg-amber-500 animate-pulse" data-tooltip="Connecting..." />
              </Show>
            </div>
            <Show when={canUseRunContext()}>
              <span class="text-[10px] text-wool-500">
                {useRunContext() ? `Context: ${runs.selectedRun()}` : 'Context: General'}
              </span>
            </Show>
          </div>
        </div>
        <div class="flex items-center gap-1">
          <Show when={canUseRunContext()}>
            <button
              class={`p-1.5 rounded text-xs ${
                useRunContext()
                  ? 'bg-amber-500/20 text-amber-400'
                  : 'hover:bg-pasture-700 text-wool-500'
              }`}
              onClick={toggleContext}
              data-tooltip={useRunContext() ? 'Using run context' : 'Using general context'}
            >
              <i data-lucide="link" class="w-3.5 h-3.5" />
            </button>
          </Show>
          <button
            onClick={() => app.setAiChatOpen(false)}
            class="p-1.5 rounded hover:bg-pasture-700 text-wool-500 hover:text-wool-300"
          >
            <i data-lucide="x" class="w-4 h-4" />
          </button>
        </div>
      </div>

      {/* Loading state */}
      <Show when={connecting()}>
        <div class="flex-1 flex flex-col items-center justify-center p-6">
          <div class="spinner w-8 h-8 mb-4" />
          <p class="text-sm text-wool-400">Connecting to AI...</p>
        </div>
      </Show>

      {/* Messages area */}
      <Show when={!connecting()}>
        <div class="flex-1 flex flex-col overflow-hidden">
          <div
            ref={messagesContainerRef}
            class="messages-container flex-1 overflow-y-auto p-4 space-y-4"
          >
            <For each={messages}>
              {(msg) => (
                <MessageBubble
                  message={msg}
                  expandedTools={expandedTools()}
                  onToggleTool={toggleTool}
                />
              )}
            </For>

            {/* Streaming message */}
            <Show when={currentMessage()}>
              <MessageBubble
                message={currentMessage() as ChatMessage}
                expandedTools={expandedTools()}
                onToggleTool={toggleTool}
              />
            </Show>
          </div>

          {/* Permission modal */}
          <Show when={pendingPermission()}>
            <PermissionModal
              permission={pendingPermission()!}
              onRespond={respondToPermission}
            />
          </Show>

          {/* Input area */}
          <div class="p-3 border-t border-pasture-600 shrink-0">
            <div class="flex gap-2">
              <textarea
                value={inputText()}
                onInput={(e) => setInputText(e.currentTarget.value)}
                onKeyDown={handleKeyDown}
                placeholder={connected() ? 'Ask anything...' : 'Connecting...'}
                disabled={!connected() || sending()}
                rows={1}
                class="ai-chat-input flex-1 px-4 py-2.5 text-sm bg-pasture-900 border border-pasture-600 rounded-lg focus:outline-none focus:border-amber-500 text-wool-200 placeholder-wool-600 disabled:opacity-50 resize-none"
              />
              <button
                onClick={sendMessage}
                disabled={!inputText().trim() || !connected() || sending()}
                class="px-4 py-2.5 bg-amber-600 hover:bg-amber-500 disabled:opacity-50 disabled:cursor-not-allowed rounded-lg text-sm font-medium transition-colors"
              >
                <Show when={sending()}>
                  <span class="spinner w-4 h-4" />
                </Show>
                <Show when={!sending()}>
                  <i data-lucide="send" class="w-4 h-4" />
                </Show>
              </button>
            </div>
          </div>
        </div>
      </Show>
    </aside>
  );
};

// Message bubble component
const MessageBubble: Component<{
  message: ChatMessage;
  expandedTools: Set<string>;
  onToggleTool: (id: string) => void;
}> = (props) => {
  const isUser = () => props.message.role === 'user';

  return (
    <div class={`flex ${isUser() ? 'justify-end' : 'justify-start'}`}>
      <div
        class={`max-w-[85%] rounded-lg px-3 py-2 ${
          isUser()
            ? 'bg-amber-500/20 text-wool-200'
            : 'bg-pasture-700 text-wool-300'
        }`}
      >
        {/* Thinking */}
        <Show when={props.message.thinking}>
          <div class="mb-2 border-l-2 border-amber-500/30 pl-2">
            <div class="flex items-center gap-1 text-amber-500/70 text-xs mb-0.5">
              <i data-lucide="brain" class="w-3 h-3" />
              <span>Thinking</span>
            </div>
            <p class="text-xs text-wool-500 whitespace-pre-wrap line-clamp-3">
              {props.message.thinking}
            </p>
          </div>
        </Show>

        {/* Content */}
        <Show when={props.message.content}>
          <p class="text-sm whitespace-pre-wrap">{props.message.content}</p>
        </Show>

        {/* Tool calls */}
        <Show when={props.message.toolCalls?.length}>
          <div class="mt-2 space-y-1">
            <For each={props.message.toolCalls}>
              {(tc) => (
                <div class="border border-pasture-600 rounded text-xs overflow-hidden">
                  <button
                    class="w-full px-2 py-1.5 flex items-center gap-2 bg-pasture-800 hover:bg-pasture-700 text-left"
                    onClick={() => props.onToggleTool(tc.id)}
                  >
                    <i data-lucide="wrench" class="w-3 h-3 text-wool-500" />
                    <span class="flex-1 truncate text-wool-300">{tc.title}</span>
                    <Show when={tc.status === 'completed'}>
                      <i data-lucide="check" class="w-3 h-3 text-sage" />
                    </Show>
                    <Show when={tc.status === 'failed'}>
                      <i data-lucide="x" class="w-3 h-3 text-terra" />
                    </Show>
                    <Show when={tc.status === 'pending' || tc.status === 'in_progress'}>
                      <span class="spinner w-3 h-3" />
                    </Show>
                    <i
                      data-lucide={props.expandedTools.has(tc.id) ? 'chevron-up' : 'chevron-down'}
                      class="w-3 h-3 text-wool-500"
                    />
                  </button>
                  <Show when={props.expandedTools.has(tc.id)}>
                    <div class="p-2 bg-pasture-900 border-t border-pasture-600 space-y-2">
                      <Show when={tc.input}>
                        <div>
                          <p class="text-wool-500 mb-0.5">Input</p>
                          <pre class="text-wool-400 bg-pasture-800 p-1 rounded overflow-x-auto">
                            {tc.input}
                          </pre>
                        </div>
                      </Show>
                      <Show when={tc.output}>
                        <div>
                          <p class="text-wool-500 mb-0.5">Output</p>
                          <pre class="text-wool-400 bg-pasture-800 p-1 rounded overflow-x-auto max-h-32 overflow-y-auto">
                            {tc.output}
                          </pre>
                        </div>
                      </Show>
                    </div>
                  </Show>
                </div>
              )}
            </For>
          </div>
        </Show>

        {/* Streaming indicator */}
        <Show when={props.message.streaming}>
          <span class="inline-block w-2 h-4 bg-wool-400 animate-pulse ml-1" />
        </Show>
      </div>
    </div>
  );
};

// Permission modal component
const PermissionModal: Component<{
  permission: PendingPermission;
  onRespond: (optionId: string) => void;
}> = (props) => {
  return (
    <div class="absolute inset-0 bg-black/60 flex items-center justify-center p-4">
      <div class="bg-pasture-800 border border-pasture-600 rounded-lg shadow-xl w-full max-w-sm">
        <div class="p-4 border-b border-pasture-600">
          <h3 class="text-sm font-medium text-wool-100">{props.permission.title}</h3>
          <Show when={props.permission.description}>
            <p class="text-xs text-wool-500 mt-1">{props.permission.description}</p>
          </Show>
        </div>
        <div class="p-4 space-y-2">
          <For each={props.permission.options}>
            {(option) => (
              <button
                class="w-full px-3 py-2 text-left text-sm rounded hover:bg-pasture-700 border border-pasture-600"
                onClick={() => props.onRespond(option.optionId)}
              >
                {option.label}
              </button>
            )}
          </For>
        </div>
      </div>
    </div>
  );
};
