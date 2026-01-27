/**
 * GypMessenger - Unified chat widget that lives in the status bar
 *
 * Replaces DirectChat sidebar and GypChatDrawer with a single messenger
 * that adapts its context based on where the user is in the app.
 */
import {
  type Component,
  For,
  Show,
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
} from 'solid-js';
import type { ChatMessage, ChatToolCall, PendingPermission } from '../../lib/types';
import { useApp, useProject, useRuns } from '../../stores';
import { useGypChat, type GypChatContext } from '../../hooks/use-gyp-chat';
import { initLucideIcons } from '../../lib/icons';

const PANEL_WIDTH = 420;
const PANEL_HEIGHT = 480;

export const GypMessenger: Component = () => {
  const app = useApp();
  const project = useProject();
  const runs = useRuns();

  let messagesRef: HTMLDivElement | undefined;
  let inputRef: HTMLTextAreaElement | undefined;

  const [inputText, setInputText] = createSignal('');
  const [sending, setSending] = createSignal(false);
  const [expandedTools, setExpandedTools] = createSignal<Set<string>>(new Set());
  const [menuOpen, setMenuOpen] = createSignal(false);

  // Build context based on current app state
  const buildContext = (): GypChatContext => {
    const projectId = project.selectedProjectId();
    const projectName = project.selectedProject()?.name;
    const runName = runs.selectedRun();
    const runDetail = runs.runDetail();
    const activeView = project.activeProjectView();

    // On board view with a project selected
    if (projectId && activeView === 'board' && !runName) {
      return { type: 'board', projectId, projectName };
    }

    // Run selected - check if draft or active run
    if (runName && runDetail) {
      if (runDetail.status === 'draft') {
        return { type: 'draft', runName, projectId: projectId ?? undefined, projectName };
      }
      return { type: 'run', runName, projectId: projectId ?? undefined, projectName };
    }

    // General context
    return { type: 'general' };
  };

  // Chat hook
  const chat = useGypChat(buildContext, {
    historyDepth: 20,
    onEditComplete: () => {
      // Refresh board when Gyp finishes editing
      const projectId = project.selectedProjectId();
      if (projectId) {
        window.dispatchEvent(new CustomEvent('board-refresh', { detail: projectId }));
      }
    },
  });

  // Auto-connect when expanded
  createEffect(() => {
    if (app.aiChatOpen() && !chat.connected() && !chat.connecting()) {
      chat.connect();
    }
  });

  // Scroll to bottom when messages change
  createEffect(() => {
    chat.messages.length;
    chat.currentMessage();
    if (messagesRef) {
      messagesRef.scrollTop = messagesRef.scrollHeight;
    }
  });

  // Focus input when expanded
  createEffect(() => {
    if (app.aiChatOpen()) {
      setTimeout(() => inputRef?.focus(), 100);
    }
  });

  // Initialize icons when expanded or menu opens
  createEffect(() => {
    if (app.aiChatOpen()) {
      queueMicrotask(() => initLucideIcons());
    }
  });
  createEffect(() => {
    if (menuOpen()) {
      queueMicrotask(() => initLucideIcons());
    }
  });

  // Keyboard shortcut handler for Cmd+Shift+A
  createEffect(() => {
    const handler = (e: KeyboardEvent) => {
      // Cmd+Shift+A to toggle
      if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === 'a') {
        e.preventDefault();
        app.toggleAiChat();
      }
    };

    document.addEventListener('keydown', handler);
    onCleanup(() => document.removeEventListener('keydown', handler));
  });

  // Listen for gyp-focus-node events from SpecflowBoard
  createEffect(() => {
    const handler = (e: Event) => {
      const customEvent = e as CustomEvent<{ id: string; name: string }>;
      chat.setFocusNode(customEvent.detail.id, customEvent.detail.name);
    };
    window.addEventListener('gyp-focus-node', handler);
    onCleanup(() => window.removeEventListener('gyp-focus-node', handler));
  });

  // Dispatch gyp-editing-islands events when editing state changes
  createEffect(() => {
    const islands = chat.editingIslands();
    window.dispatchEvent(new CustomEvent('gyp-editing-islands', { detail: islands }));
  });

  // Get context label for display
  const contextLabel = createMemo(() => {
    const ctx = chat.context();
    switch (ctx.type) {
      case 'board':
        return ctx.projectName || 'Board';
      case 'run':
        return `Run: ${ctx.runName}`;
      case 'draft':
        return `Draft: ${ctx.runName}`;
      default:
        return 'General';
    }
  });

  // Handle send
  const handleSend = async () => {
    const text = inputText().trim();
    if (!text || sending()) return;

    setSending(true);
    setInputText('');
    try {
      await chat.sendMessage(text);
    } finally {
      setSending(false);
    }
  };

  // Handle key press
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Escape') {
      app.setAiChatOpen(false);
      return;
    }
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
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

  // Recent messages (last 50)
  const recentMessages = () => chat.messages.slice(-50);

  return (
    <>
      {/* Chat Panel (shown when expanded) */}
      <Show when={app.aiChatOpen()}>
        <div
          class="fixed right-0 bottom-[36px] rounded-t-xl overflow-hidden flex flex-col bg-pasture-800/98 border border-pasture-600 border-b-0 shadow-xl backdrop-blur-xl z-[1000]"
          classList={{
            'ring-2 ring-amber-500/50 border-amber-500/50': chat.gypEditing(),
          }}
          style={{
            width: `${PANEL_WIDTH}px`,
            height: `${PANEL_HEIGHT}px`,
          }}
        >
          {/* Header */}
          <div class="px-4 py-3 flex items-center justify-between shrink-0 border-b border-pasture-600">
            <div class="flex items-center gap-3">
              <img src="/gyp.svg" class="w-8 h-8" alt="Gyp" />
              <div class="flex flex-col">
                <div class="flex items-center gap-2">
                  <span class="text-sm font-medium text-wool-200">Gyp</span>
                  <Show when={chat.connected()}>
                    <span class="w-2 h-2 rounded-full bg-sage" />
                  </Show>
                  <Show when={!chat.connected() && !chat.connecting()}>
                    <span class="w-2 h-2 rounded-full bg-wool-700" />
                  </Show>
                  <Show when={chat.connecting()}>
                    <span class="w-2 h-2 rounded-full bg-amber-500 animate-pulse" />
                  </Show>
                </div>
                <span class="text-[10px] text-wool-500">{contextLabel()}</span>
              </div>
            </div>
            <div class="flex items-center gap-1">
              <Show when={chat.context().focusNodeName}>
                <div class="flex items-center gap-1.5 text-xs text-amber-400/70 max-w-[140px] mr-2">
                  <i data-lucide="crosshair" class="w-3 h-3" />
                  <span class="truncate">{chat.context().focusNodeName}</span>
                  <button
                    onClick={() => chat.setFocusNode(null, null)}
                    class="text-wool-600 hover:text-wool-400"
                  >
                    <i data-lucide="x" class="w-3 h-3" />
                  </button>
                </div>
              </Show>

              {/* Options menu */}
              <div class="relative">
                <button
                  onClick={() => setMenuOpen(!menuOpen())}
                  class="p-1.5 rounded hover:bg-pasture-700 text-wool-500 hover:text-wool-300"
                  title="Options"
                >
                  <i data-lucide="more-vertical" class="w-4 h-4" />
                </button>

                <Show when={menuOpen()}>
                  {/* Backdrop to close menu */}
                  <div
                    class="fixed inset-0 z-40"
                    onClick={() => setMenuOpen(false)}
                  />

                  {/* Dropdown menu */}
                  <div class="absolute right-0 top-full mt-1 w-40 bg-pasture-800 border border-pasture-600 rounded-lg shadow-xl overflow-hidden z-50">
                    <button
                      onClick={async () => {
                        setMenuOpen(false);
                        await chat.reset();
                        window.toast?.success('Chat reset');
                      }}
                      class="w-full flex items-center gap-2 px-3 py-2 text-sm text-wool-300 hover:bg-pasture-700 hover:text-wool-100 transition-colors"
                    >
                      <i data-lucide="refresh-cw" class="w-3.5 h-3.5" />
                      <span>Reset chat</span>
                    </button>
                  </div>
                </Show>
              </div>

              <button
                onClick={() => app.setAiChatOpen(false)}
                class="p-1.5 rounded hover:bg-pasture-700 text-wool-500 hover:text-wool-300"
              >
                <i data-lucide="chevron-down" class="w-4 h-4" />
              </button>
            </div>
          </div>

          {/* Messages */}
          <div
            ref={messagesRef}
            class="flex-1 overflow-y-auto px-4 py-3 space-y-3 bg-pasture-900/50"
          >
            <Show when={!chat.connected() && !chat.connecting()}>
              <div class="text-center text-xs text-wool-600 py-8 italic">
                Click to connect...
              </div>
            </Show>

            <Show when={chat.connecting()}>
              <div class="text-center text-xs text-wool-500 py-8 flex items-center justify-center gap-2">
                <div class="w-4 h-4 border-2 border-wool-600 border-t-amber-500 rounded-full animate-spin" />
                Connecting...
              </div>
            </Show>

            <Show when={chat.connected()}>
              <For each={recentMessages()}>
                {(msg) => (
                  <MessageBubble
                    message={msg}
                    expandedTools={expandedTools()}
                    onToggleTool={toggleTool}
                  />
                )}
              </For>

              {/* Streaming message */}
              <Show when={chat.currentMessage()}>
                <div class="text-sm rounded-lg px-3 py-2.5 bg-pasture-800/80 text-wool-300 mr-8 border border-pasture-600/50">
                  <Show when={chat.gypEditing()}>
                    <div class="flex items-center gap-2 text-[10px] text-amber-400 mb-1.5">
                      <div class="w-2.5 h-2.5 border-2 border-amber-400 border-t-transparent rounded-full animate-spin" />
                      Editing board...
                    </div>
                  </Show>
                  <Show when={chat.currentMessage()?.toolCalls?.length && !chat.gypEditing()}>
                    <div class="flex items-center gap-2 text-[10px] text-wool-500 mb-1.5">
                      <div class="w-2.5 h-2.5 border-2 border-wool-500 border-t-transparent rounded-full animate-spin" />
                      Working...
                    </div>
                  </Show>
                  <div class="whitespace-pre-wrap break-words">
                    {chat.currentMessage()?.content || (
                      <span class="text-wool-600 italic">Thinking...</span>
                    )}
                  </div>
                  <span class="inline-block w-2 h-4 bg-wool-400 animate-pulse ml-1" />
                </div>
              </Show>
            </Show>
          </div>

          {/* Permission modal */}
          <Show when={chat.pendingPermission()}>
            <PermissionModal
              permission={chat.pendingPermission()!}
              onRespond={chat.respondToPermission}
            />
          </Show>

          {/* Input */}
          <div class="px-3 pb-3 pt-2 shrink-0 bg-pasture-800 border-t border-pasture-600">
            <div class="relative rounded-lg overflow-hidden bg-pasture-900/60 border border-pasture-600">
              <textarea
                ref={inputRef}
                value={inputText()}
                onInput={(e) => setInputText(e.currentTarget.value)}
                onKeyDown={handleKeyDown}
                placeholder={
                  chat.context().focusNodeName
                    ? `Ask about "${chat.context().focusNodeName}"...`
                    : 'Ask Gyp anything...'
                }
                disabled={!chat.connected() || chat.gypEditing()}
                class="w-full h-14 resize-none text-sm text-wool-100 placeholder-wool-600 focus:outline-none p-3 pr-12 bg-transparent disabled:opacity-50"
              />
              <button
                onClick={handleSend}
                disabled={!inputText().trim() || !chat.connected() || chat.gypEditing() || sending()}
                class="absolute right-2.5 bottom-2.5 p-2 rounded-lg transition-all disabled:opacity-30 hover:scale-105 bg-amber-600/40 hover:bg-amber-600/60 border border-amber-500/50"
              >
                <Show when={sending()}>
                  <div class="w-4 h-4 border-2 border-amber-200 border-t-transparent rounded-full animate-spin" />
                </Show>
                <Show when={!sending()}>
                  <i data-lucide="send" class="w-4 h-4 text-amber-200" />
                </Show>
              </button>
            </div>
            <div class="flex items-center justify-between mt-2 text-[10px] text-wool-600">
              <span>Enter to send, Shift+Enter for newline</span>
              <span>Esc to close</span>
            </div>
          </div>
        </div>
      </Show>
    </>
  );
};

/**
 * GypMessengerBar - The part that lives in the StatusBar
 */
export const GypMessengerBar: Component = () => {
  const app = useApp();
  const project = useProject();
  const runs = useRuns();

  // Build context label
  const contextLabel = createMemo(() => {
    const projectId = project.selectedProjectId();
    const projectName = project.selectedProject()?.name;
    const runName = runs.selectedRun();
    const runDetail = runs.runDetail();
    const activeView = project.activeProjectView();

    if (projectId && activeView === 'board' && !runName) {
      return projectName || 'Board';
    }
    if (runName && runDetail) {
      if (runDetail.status === 'draft') {
        return `Draft: ${runName}`;
      }
      return `Run: ${runName}`;
    }
    return 'General';
  });

  return (
    <button
      onClick={() => app.toggleAiChat()}
      class="flex items-center gap-3 px-4 h-full min-w-[180px] transition-all border-l border-pasture-600 hover:bg-pasture-700"
      classList={{
        'bg-pasture-700': app.aiChatOpen(),
      }}
    >
      <div class="flex items-center gap-2.5">
        <img
          src="/gyp.svg"
          alt="Gyp"
          class="w-5 h-5"
          classList={{
            'opacity-60': !app.aiChatOpen(),
          }}
        />
        <span class="text-xs text-wool-400">Gyp</span>
      </div>

      {/* Context indicator */}
      <div class="flex items-center gap-1.5 text-[10px] text-wool-600 truncate max-w-[100px]">
        <span class="text-wool-700">·</span>
        <span class="truncate">{contextLabel()}</span>
      </div>

      {/* Expand/collapse chevron */}
      <div class="ml-auto flex items-center gap-2">
        <i
          data-lucide={app.aiChatOpen() ? 'chevron-down' : 'chevron-up'}
          class="w-3.5 h-3.5 text-wool-600"
        />
      </div>
    </button>
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
            ? 'bg-amber-500/15 text-wool-200 ml-8 border border-amber-500/25'
            : 'bg-pasture-800/80 text-wool-300 mr-8 border border-pasture-600/50'
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
          <p class="text-sm whitespace-pre-wrap break-words">{props.message.content}</p>
        </Show>

        {/* Tool calls - compact inline display */}
        <Show when={props.message.toolCalls?.length}>
          <div class="mt-2 flex flex-wrap gap-1.5">
            <For each={props.message.toolCalls}>
              {(tc) => (
                <ToolCallChip
                  toolCall={tc}
                  expanded={props.expandedTools.has(tc.id)}
                  onToggle={() => props.onToggleTool(tc.id)}
                />
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

// Tool call chip - compact inline display
const ToolCallChip: Component<{
  toolCall: ChatToolCall;
  expanded: boolean;
  onToggle: () => void;
}> = (props) => {
  // Get icon based on tool kind
  const getIcon = () => {
    switch (props.toolCall.kind) {
      case 'read':
        return 'file-text';
      case 'write':
      case 'edit':
        return 'pencil';
      case 'execute':
        return 'terminal';
      case 'search':
        return 'search';
      default:
        return 'wrench';
    }
  };

  // Get short label from title
  const shortLabel = () => {
    const title = props.toolCall.title;
    // Extract just the tool name, e.g., "Read" from "Read file.txt"
    const firstWord = title.split(/[\s:(]/)[0];
    return firstWord || title;
  };

  const isWorking = () =>
    props.toolCall.status === 'pending' || props.toolCall.status === 'in_progress';

  return (
    <div class="relative">
      {/* Chip */}
      <button
        onClick={props.onToggle}
        class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-medium transition-all hover:scale-105"
        classList={{
          'bg-wool-700/30 text-wool-400': props.toolCall.status === 'completed',
          'bg-sage/20 text-sage': props.toolCall.status === 'completed' && props.toolCall.kind === 'write',
          'bg-terra/20 text-terra': props.toolCall.status === 'failed',
          'bg-amber-500/20 text-amber-400': isWorking(),
        }}
      >
        <Show when={isWorking()}>
          <span class="w-2.5 h-2.5 border border-current border-t-transparent rounded-full animate-spin" />
        </Show>
        <Show when={!isWorking()}>
          <i data-lucide={getIcon()} class="w-2.5 h-2.5" />
        </Show>
        <span>{shortLabel()}</span>
        <Show when={props.toolCall.status === 'completed' && props.toolCall.kind !== 'write'}>
          <i data-lucide="check" class="w-2.5 h-2.5 text-sage" />
        </Show>
        <Show when={props.toolCall.status === 'failed'}>
          <i data-lucide="x" class="w-2.5 h-2.5" />
        </Show>
      </button>

      {/* Expanded details popover */}
      <Show when={props.expanded}>
        <div class="absolute left-0 top-full mt-1 z-50 w-72 bg-pasture-800 border border-pasture-600 rounded-lg shadow-xl overflow-hidden">
          {/* Header */}
          <div class="px-2.5 py-1.5 bg-pasture-700 border-b border-pasture-600 flex items-center justify-between">
            <span class="text-[11px] font-medium text-wool-200 truncate flex-1">
              {props.toolCall.title}
            </span>
            <button
              onClick={props.onToggle}
              class="p-0.5 rounded hover:bg-pasture-600 text-wool-500"
            >
              <i data-lucide="x" class="w-3 h-3" />
            </button>
          </div>

          {/* Content */}
          <div class="p-2 space-y-2 max-h-48 overflow-y-auto">
            <Show when={props.toolCall.input}>
              <div>
                <p class="text-[10px] text-wool-500 mb-0.5 uppercase tracking-wide">Input</p>
                <pre class="text-[10px] text-wool-300 bg-pasture-900 p-1.5 rounded overflow-x-auto whitespace-pre-wrap break-all">
                  {props.toolCall.input}
                </pre>
              </div>
            </Show>
            <Show when={props.toolCall.output}>
              <div>
                <p class="text-[10px] text-wool-500 mb-0.5 uppercase tracking-wide">Output</p>
                <pre class="text-[10px] text-wool-300 bg-pasture-900 p-1.5 rounded overflow-x-auto whitespace-pre-wrap break-all max-h-24 overflow-y-auto">
                  {props.toolCall.output}
                </pre>
              </div>
            </Show>
            <Show when={!props.toolCall.input && !props.toolCall.output}>
              <p class="text-[10px] text-wool-600 italic">No details available</p>
            </Show>
          </div>
        </div>
      </Show>
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
                class="w-full px-3 py-2 text-left text-sm text-wool-200 rounded hover:bg-pasture-700 border border-pasture-600"
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
