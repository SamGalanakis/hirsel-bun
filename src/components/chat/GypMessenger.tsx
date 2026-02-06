/**
 * GypMessenger - "The Shepherd's Hearth" chat widget
 *
 * A warm, intimate conversation space that feels like sitting by the fire
 * in a Scottish croft, speaking with your trusted sheepdog companion.
 */
import {
  type Component,
  For,
  Show,
  createEffect,
  createMemo,
  createSignal,
  on,
  onCleanup,
} from 'solid-js';
import { Portal } from 'solid-js/web';
import type { ChatMessage, ChatToolCall, PendingPermission } from '../../lib/types';
import { useApp, useProject, useRuns } from '../../stores';
import { useGypChat, type GypChatContext } from '../../hooks/use-gyp-chat';
import { Icon, Markdown, ThinkingBlock, ToolCard, ToolCluster, type ToolInfo } from '../shared';

const PANEL_WIDTH = 440;
const PANEL_HEIGHT = 520;

export const GypMessenger: Component = () => {
  const app = useApp();
  const project = useProject();
  const runs = useRuns();

  let messagesRef: HTMLDivElement | undefined;
  let inputRef: HTMLTextAreaElement | undefined;
  let menuBtnRef: HTMLButtonElement | undefined;

  const [inputText, setInputText] = createSignal('');
  const [sending, setSending] = createSignal(false);
  const [expandedTools, setExpandedTools] = createSignal<Set<string>>(new Set());
  const [menuOpen, setMenuOpen] = createSignal(false);

  // Compute fixed position for menu dropdown (escapes overflow-hidden panel)
  const menuPosition = () => {
    if (!menuOpen() || !menuBtnRef) return {};
    const rect = menuBtnRef.getBoundingClientRect();
    return {
      top: `${rect.bottom + 4}px`,
      right: `${window.innerWidth - rect.right}px`,
    };
  };

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

  // Track if we're in the middle of a project switch to prevent auto-connect race
  let projectSwitchInProgress = false;

  // Auto-connect when expanded (only if not mid-project-switch)
  createEffect(() => {
    if (app.aiChatOpen() && !chat.connected() && !chat.connecting() && !projectSwitchInProgress) {
      chat.connect();
    }
  });

  // Reconnect chat when project changes (each project has its own conversation in database)
  createEffect(
    on(
      () => project.selectedProjectId(),
      (projectId, prevProjectId) => {
        // Skip initial run and when both are null
        if (prevProjectId === undefined) return;
        if (projectId === prevProjectId) return;

        // Mark that we're switching projects to prevent auto-connect from racing
        projectSwitchInProgress = true;

        // Disconnect and reconnect with new project context
        chat.disconnect().then(() => {
          // Only reconnect if switching to another project (not during deletion)
          if (projectId !== null) {
            chat.connect().finally(() => {
              projectSwitchInProgress = false;
            });
          } else {
            projectSwitchInProgress = false;
          }
        });
      }
    )
  );

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

  // Get context label for display (pastoral/poetic)
  const contextLabel = createMemo(() => {
    const ctx = chat.context();
    switch (ctx.type) {
      case 'board':
        return 'Tending the board...';
      case 'run':
        return `Watching ${ctx.runName}...`;
      case 'draft':
        return `Shaping the draft...`;
      default:
        return 'At your service';
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
          class="gyp-chat-panel gyp-panel-enter fixed right-0 bottom-[36px] rounded-t-xl overflow-hidden flex flex-col border border-amber-500/20 border-b-0 shadow-2xl z-[1000]"
          classList={{
            'gyp-editing': chat.gypEditing(),
          }}
          style={{
            width: `${PANEL_WIDTH}px`,
            height: `${PANEL_HEIGHT}px`,
          }}
        >
          {/* Header - "Gyp's Corner" */}
          <div class="px-4 pt-4 pb-2 shrink-0">
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-3">
                {/* Larger avatar with connection ring */}
                <div
                  class="gyp-avatar-ring"
                  classList={{
                    connected: chat.connected(),
                    connecting: chat.connecting(),
                  }}
                >
                  <img
                    src="/gyp.svg"
                    class="w-10 h-10 drop-shadow-md"
                    alt="Gyp"
                  />
                </div>
                <div class="flex flex-col">
                  <span class="text-base font-semibold text-wool-200" style="font-family: 'ET Book', serif;">
                    Gyp
                  </span>
                  <span class="text-[11px] italic text-wool-500" style="font-family: 'ET Book', serif;">
                    {contextLabel()}
                  </span>
                </div>
              </div>

              <div class="flex items-center gap-1">
                {/* Focus node indicator */}
                <Show when={chat.context().focusNodeName}>
                  <div class="gyp-focus-indicator max-w-[140px] mr-2">
                    <Icon name="crosshair" class="w-3 h-3" />
                    <span class="truncate">{chat.context().focusNodeName}</span>
                    <button
                      onClick={() => chat.setFocusNode(null, null)}
                      class="text-wool-600 hover:text-wool-400 ml-1"
                    >
                      <Icon name="x" class="w-3 h-3" />
                    </button>
                  </div>
                </Show>

                {/* Options menu */}
                <div>
                  <button
                    ref={menuBtnRef}
                    type="button"
                    onClick={() => setMenuOpen(!menuOpen())}
                    class="gyp-options-btn"
                    title="Options"
                    aria-haspopup="listbox"
                    aria-expanded={menuOpen()}
                  >
                    <Icon name="more-vertical" class="w-4 h-4" />
                  </button>

                  <Show when={menuOpen()}>
                    <Portal>
                      <div
                        class="fixed inset-0 z-[1001]"
                        onClick={() => setMenuOpen(false)}
                      />
                      <div
                        class="fixed z-[1002] w-44 bg-popover border border-border rounded-md shadow-md py-1"
                        style={menuPosition()}
                      >
                        <div role="listbox">
                          <div
                            role="option"
                            class="px-3 py-2 text-sm cursor-pointer hover:bg-accent flex items-center gap-2"
                            onClick={async () => {
                              setMenuOpen(false);
                              await chat.reset();
                              window.toast?.success('Chat reset');
                            }}
                          >
                            <Icon name="refresh-cw" class="w-3.5 h-3.5" />
                            <span>Reset chat</span>
                          </div>
                        </div>
                      </div>
                    </Portal>
                  </Show>
                </div>

                <button
                  onClick={() => app.setAiChatOpen(false)}
                  class="gyp-options-btn"
                >
                  <Icon name="chevron-down" class="w-4 h-4" />
                </button>
              </div>
            </div>

            {/* Decorative divider */}
            <div class="gyp-header-divider" />
          </div>

          {/* Messages Area */}
          <div
            ref={messagesRef}
            class="gyp-messages-area flex-1 overflow-y-auto px-4 py-3 space-y-3"
          >
            {/* Empty state - redesigned */}
            <Show when={chat.connected() && recentMessages().length === 0 && !chat.currentMessage()}>
              <div class="gyp-empty-state h-full">
                <img
                  src="/gyp.svg"
                  class="w-16 h-16 gyp-avatar-breathe opacity-80"
                  alt="Gyp"
                />
                <p class="text-sm italic text-wool-500" style="font-family: 'ET Book', serif;">
                  Gyp is ready to help
                </p>
                <div class="gyp-decorative-dots">
                  <span />
                  <span />
                  <span />
                </div>
              </div>
            </Show>

            {/* Connecting state */}
            <Show when={chat.connecting()}>
              <div class="gyp-empty-state h-full">
                <img
                  src="/gyp.svg"
                  class="w-14 h-14 opacity-60"
                  alt="Gyp"
                />
                <div class="flex items-center gap-2 text-sm text-wool-500">
                  <div class="w-4 h-4 border-2 border-wool-600 border-t-amber-500 rounded-full animate-spin" />
                  <span class="italic" style="font-family: 'ET Book', serif;">Connecting...</span>
                </div>
              </div>
            </Show>

            {/* Messages */}
            <Show when={chat.connected()}>
              <For each={recentMessages()}>
                {(msg, index) => (
                  <MessageBubble
                    message={msg}
                    expandedTools={expandedTools()}
                    onToggleTool={toggleTool}
                    isLatest={index() === recentMessages().length - 1}
                  />
                )}
              </For>

              {/* Streaming message */}
              <Show when={chat.currentMessage()}>
                <div class="gyp-message-assistant gyp-message-enter text-sm px-3 py-2.5 mr-8">
                  <Show when={chat.gypEditing()}>
                    <div class="flex items-center gap-2 text-[11px] text-amber-400 mb-2">
                      <div class="w-3 h-3 border-2 border-amber-400 border-t-transparent rounded-full animate-spin" />
                      <span class="italic" style="font-family: 'ET Book', serif;">Working on it...</span>
                    </div>
                  </Show>
                  <Show when={chat.currentMessage()?.toolCalls?.length && !chat.gypEditing()}>
                    <div class="flex items-center gap-2 text-[11px] text-wool-500 mb-2">
                      <div class="w-3 h-3 border-2 border-wool-500 border-t-transparent rounded-full animate-spin" />
                      <span class="italic" style="font-family: 'ET Book', serif;">Working...</span>
                    </div>
                  </Show>
                  <Show when={chat.currentMessage()?.content}>
                    <Markdown content={chat.currentMessage()?.content || ''} class="text-sm text-wool-300" />
                  </Show>
                  <Show when={!chat.currentMessage()?.content}>
                    <div class="flex items-center gap-2">
                      <span class="italic text-wool-500" style="font-family: 'ET Book', serif;">Gyp is thinking</span>
                      <div class="gyp-thinking-dots">
                        <span />
                        <span />
                        <span />
                      </div>
                    </div>
                  </Show>
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

          {/* Input Area - "The Fireside" */}
          <div class="gyp-input-area px-3 pb-3 pt-2 shrink-0 border-t border-pasture-600/50">
            <div
              class="gyp-input-wrapper relative overflow-hidden"
              classList={{ disabled: !chat.connected() || chat.gypEditing() }}
            >
              <textarea
                ref={inputRef}
                value={inputText()}
                onInput={(e) => setInputText(e.currentTarget.value)}
                onKeyDown={handleKeyDown}
                placeholder={
                  chat.context().focusNodeName
                    ? `Ask about "${chat.context().focusNodeName}"...`
                    : 'Speak with Gyp...'
                }
                disabled={!chat.connected() || chat.gypEditing()}
                class="w-full h-14 resize-none text-sm text-wool-100 placeholder-wool-600 placeholder:italic focus:outline-none p-3 pr-12 bg-transparent disabled:opacity-50"
                style="font-family: 'ET Book', serif;"
              />
              <button
                onClick={handleSend}
                disabled={!inputText().trim() || !chat.connected() || chat.gypEditing() || sending()}
                class="gyp-send-btn absolute right-2 bottom-2 w-8 h-8 flex items-center justify-center"
              >
                <Show when={sending()}>
                  <div class="w-4 h-4 border-2 border-amber-200 border-t-transparent rounded-full animate-spin" />
                </Show>
                <Show when={!sending()}>
                  <Icon name="send" class="w-4 h-4 text-white" />
                </Show>
              </button>
            </div>
            <div class="gyp-keyboard-hints flex items-center justify-between mt-2 px-1">
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
        <Icon
          name={app.aiChatOpen() ? 'chevron-down' : 'chevron-up'}
          class="w-3.5 h-3.5 text-wool-600"
        />
      </div>
    </button>
  );
};

// Message bubble component - redesigned
const MessageBubble: Component<{
  message: ChatMessage;
  expandedTools: Set<string>;
  onToggleTool: (id: string) => void;
  isLatest: boolean;
}> = (props) => {
  const isUser = () => props.message.role === 'user';

  return (
    <div
      class={`flex gyp-message-enter ${isUser() ? 'justify-end' : 'justify-start'}`}
    >
      <div
        class={`max-w-[85%] px-3 py-2.5 ${
          isUser()
            ? 'gyp-message-user text-wool-200 ml-8'
            : 'gyp-message-assistant text-wool-300 mr-8'
        }`}
      >
        {/* Thinking block */}
        <Show when={props.message.thinking}>
          <div class="mb-2">
            <ThinkingBlock content={props.message.thinking!} />
          </div>
        </Show>

        {/* Content */}
        <Show when={props.message.content}>
          <Markdown content={props.message.content} class="text-sm" />
        </Show>

        {/* Tool calls - cluster if 2+, single card otherwise */}
        <Show when={props.message.toolCalls?.length}>
          <div class="mt-2">
            <Show when={(props.message.toolCalls?.length || 0) >= 2}>
              {/* Cluster multiple tools */}
              <ToolCluster
                tools={props.message.toolCalls!.map((tc): ToolInfo => ({
                  id: tc.id,
                  title: tc.title,
                  kind: tc.kind,
                  status: tc.status,
                  input: tc.input,
                  output: tc.output,
                }))}
              />
            </Show>
            <Show when={(props.message.toolCalls?.length || 0) === 1}>
              {/* Single tool - show as card */}
              <ToolCard
                title={props.message.toolCalls![0].title}
                kind={props.message.toolCalls![0].kind}
                status={props.message.toolCalls![0].status}
                input={props.message.toolCalls![0].input}
                output={props.message.toolCalls![0].output}
                expanded={props.expandedTools.has(props.message.toolCalls![0].id)}
                onToggle={() => props.onToggleTool(props.message.toolCalls![0].id)}
              />
            </Show>
          </div>
        </Show>
      </div>
    </div>
  );
};

// Permission modal - parchment-style request card
const PermissionModal: Component<{
  permission: PendingPermission;
  onRespond: (optionId: string) => void;
}> = (props) => {
  return (
    <div class="gyp-permission-overlay absolute inset-0 flex items-center justify-center p-4">
      <div class="gyp-permission-modal w-full max-w-sm">
        <div class="p-4 border-b border-pasture-600">
          <h3
            class="text-sm font-semibold text-wool-100"
            style="font-family: 'ET Book', serif;"
          >
            {props.permission.title}
          </h3>
          <Show when={props.permission.description}>
            <p
              class="text-xs text-wool-500 mt-1 italic"
              style="font-family: 'ET Book', serif;"
            >
              {props.permission.description}
            </p>
          </Show>
        </div>
        <div class="p-4 space-y-2">
          <For each={props.permission.options}>
            {(option, index) => (
              <button
                class={`gyp-permission-option text-sm text-wool-200 ${index() === 0 ? 'primary' : ''}`}
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
