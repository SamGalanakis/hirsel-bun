/**
 * ShepherdConsole - "The Shepherd's Hearth" chat widget
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
import type {
  ChatMessage,
  ShepherdImageInput,
} from '../../lib/types';
import { emit, on as onEvent } from '../../lib/events';
import { useApp, useProject, useRuns } from '../../stores';
import { useShepherdChat, type ShepherdChatContext } from '../../hooks/use-shepherd-chat';
import { Icon, Markdown, ToolCard, ToolCluster, type ToolInfo } from '../shared';

const PANEL_WIDTH = 440;
const PANEL_HEIGHT = 520;
const MAX_PASTED_IMAGES = 8;

interface PendingImage extends ShepherdImageInput {
  id: string;
  previewUrl: string;
}

function readFileAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result || ''));
    reader.onerror = () => reject(reader.error || new Error('Failed to read image'));
    reader.readAsDataURL(file);
  });
}

export const ShepherdConsole: Component = () => {
  const app = useApp();
  const project = useProject();
  const runs = useRuns();

  let messagesRef: HTMLDivElement | undefined;
  let inputRef: HTMLTextAreaElement | undefined;
  let menuBtnRef: HTMLButtonElement | undefined;

  const [inputText, setInputText] = createSignal('');
  const [sending, setSending] = createSignal(false);
  const [pendingImages, setPendingImages] = createSignal<PendingImage[]>([]);
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
  const buildContext = (): ShepherdChatContext => {
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
        return {
          type: 'draft',
          runName,
          projectId: projectId ?? undefined,
          projectName,
          projectPath: runDetail.projectPath,
        };
      }
      return {
        type: 'run',
        runName,
        projectId: projectId ?? undefined,
        projectName,
        projectPath: runDetail.projectPath,
      };
    }

    // General context
    return { type: 'general' };
  };

  // Chat hook
  const chat = useShepherdChat(buildContext, {
    historyDepth: 20,
    onEditComplete: () => {
      // Refresh board when Shepherd finishes editing
      const projectId = project.selectedProjectId();
      if (projectId) {
        emit('board-refresh', projectId);
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
    } else if (pendingImages().length > 0) {
      setPendingImages([]);
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

  // Listen for shepherd-focus-node events from SpecflowBoard
  createEffect(() => {
    const cleanup = onEvent('shepherd-focus-node', (detail) => {
      chat.setFocusNode(detail.id, detail.name);
    });
    onCleanup(cleanup);
  });

  // Dispatch shepherd-editing-islands events when editing state changes
  createEffect(() => {
    const islands = chat.editingIslands();
    emit('shepherd-editing-islands', islands);
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
    const images = pendingImages();
    if ((!text && images.length === 0) || sending()) return;

    setSending(true);
    setInputText('');
    setPendingImages([]);
    try {
      await chat.sendMessage(
        text,
        images.map((img) => ({
          mimeType: img.mimeType,
          dataBase64: img.dataBase64,
          name: img.name,
        })),
      );
    } finally {
      setSending(false);
    }
  };

  const removePendingImage = (id: string) => {
    setPendingImages((prev) => prev.filter((img) => img.id !== id));
  };

  const handlePaste = (e: ClipboardEvent) => {
    const items = Array.from(e.clipboardData?.items ?? []);
    const imageItems = items.filter((item) => item.type.startsWith('image/'));
    if (imageItems.length === 0) return;
    e.preventDefault();

    void (async () => {
      for (const item of imageItems) {
        const file = item.getAsFile();
        if (!file) continue;
        let dataUrl = '';
        try {
          dataUrl = await readFileAsDataUrl(file);
        } catch {
          continue;
        }

        const match = dataUrl.match(/^data:([^;]+);base64,(.+)$/);
        if (!match) continue;
        const [, mimeType, dataBase64] = match;

        setPendingImages((prev) => {
          if (prev.length >= MAX_PASTED_IMAGES) return prev;
          return [
            ...prev,
            {
              id: `img-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
              mimeType,
              dataBase64,
              name: file.name || undefined,
              previewUrl: dataUrl,
            },
          ];
        });
      }
    })();
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
          class="shepherd-chat-panel shepherd-panel-enter fixed right-0 bottom-[36px] rounded-t-xl overflow-hidden flex flex-col border border-amber-500/20 border-b-0 shadow-2xl z-[1000]"
          classList={{
            'shepherd-editing': chat.shepherdEditing(),
          }}
          style={{
            width: `${PANEL_WIDTH}px`,
            height: `${PANEL_HEIGHT}px`,
          }}
        >
          {/* Header - "Shepherd's Corner" */}
          <div class="px-4 pt-4 pb-2 shrink-0">
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-3">
                {/* Larger avatar with connection ring */}
                <div
                  class="shepherd-avatar-ring"
                  classList={{
                    connected: chat.connected(),
                    connecting: chat.connecting(),
                  }}
                >
                  <img
                    src="/shepherd.svg"
                    class="w-10 h-10 drop-shadow-md"
                    alt="Shepherd"
                  />
                </div>
                <div class="flex flex-col">
                  <span class="text-base font-semibold text-wool-200" style="font-family: 'ET Book', serif;">
                    Shepherd
                  </span>
                  <span class="text-[11px] italic text-wool-500" style="font-family: 'ET Book', serif;">
                    {contextLabel()}
                  </span>
                </div>
              </div>

              <div class="flex items-center gap-1">
                {/* Focus node indicator */}
                <Show when={chat.context().focusNodeName}>
                  <div class="shepherd-focus-indicator max-w-[140px] mr-2">
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
                    class="shepherd-options-btn"
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
                  class="shepherd-options-btn"
                >
                  <Icon name="chevron-down" class="w-4 h-4" />
                </button>
              </div>
            </div>

            {/* Decorative divider */}
            <div class="shepherd-header-divider" />
          </div>

          {/* Messages Area */}
          <div
            ref={messagesRef}
            class="shepherd-messages-area flex-1 overflow-y-auto px-4 py-3 space-y-3"
          >
            {/* Empty state - redesigned */}
            <Show when={chat.connected() && recentMessages().length === 0 && !chat.currentMessage()}>
              <div class="shepherd-empty-state h-full">
                <img
                  src="/shepherd.svg"
                  class="w-16 h-16 shepherd-avatar-breathe opacity-80"
                  alt="Shepherd"
                />
                <p class="text-sm italic text-wool-500" style="font-family: 'ET Book', serif;">
                  Shepherd is ready to help
                </p>
                <div class="shepherd-decorative-dots">
                  <span />
                  <span />
                  <span />
                </div>
              </div>
            </Show>

            {/* Connecting state */}
            <Show when={chat.connecting()}>
              <div class="shepherd-empty-state h-full">
                <img
                  src="/shepherd.svg"
                  class="w-14 h-14 opacity-60"
                  alt="Shepherd"
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
                <div class="shepherd-message-assistant shepherd-message-enter text-sm px-3 py-2.5 mr-8">
                  <Show when={chat.shepherdEditing()}>
                    <div class="flex items-center gap-2 text-[11px] text-amber-400 mb-2">
                      <div class="w-3 h-3 border-2 border-amber-400 border-t-transparent rounded-full animate-spin" />
                      <span class="italic" style="font-family: 'ET Book', serif;">Working on it...</span>
                    </div>
                  </Show>
                  <Show when={chat.currentMessage()?.toolCalls?.length && !chat.shepherdEditing()}>
                    <div class="flex items-center gap-2 text-[11px] text-wool-500 mb-2">
                      <div class="w-3 h-3 border-2 border-wool-500 border-t-transparent rounded-full animate-spin" />
                      <span class="italic" style="font-family: 'ET Book', serif;">Working...</span>
                    </div>
                  </Show>
                  <Show when={chat.currentMessage()?.content}>
                    <Markdown content={chat.currentMessage()?.content || ''} class="text-sm text-wool-300" />
                  </Show>
                  <Show when={!chat.currentMessage()?.content && !(chat.currentMessage()?.toolCalls?.length)}>
                    <div class="flex items-center gap-2">
                      <span class="italic text-wool-500" style="font-family: 'ET Book', serif;">Shepherd is thinking</span>
                      <div class="shepherd-thinking-dots">
                        <span />
                        <span />
                        <span />
                      </div>
                    </div>
                  </Show>

                  <Show when={chat.currentMessage()?.toolCalls?.length}>
                    <div class="mt-2">
                      <Show when={(chat.currentMessage()?.toolCalls?.length || 0) >= 2}>
                        <ToolCluster
                          tools={(chat.currentMessage()?.toolCalls || []).map((tc): ToolInfo => ({
                            id: tc.id,
                            title: tc.title,
                            kind: tc.kind,
                            status: tc.status,
                            input: tc.input,
                            output: tc.output,
                          }))}
                        />
                      </Show>
                      <Show when={(chat.currentMessage()?.toolCalls?.length || 0) === 1}>
                        <ToolCard
                          title={chat.currentMessage()?.toolCalls?.[0]?.title || 'Tool'}
                          kind={chat.currentMessage()?.toolCalls?.[0]?.kind || null}
                          status={chat.currentMessage()?.toolCalls?.[0]?.status || 'in_progress'}
                          input={chat.currentMessage()?.toolCalls?.[0]?.input || null}
                          output={chat.currentMessage()?.toolCalls?.[0]?.output || null}
                          expanded={!!chat.currentMessage()?.toolCalls?.[0]?.id && expandedTools().has(chat.currentMessage()!.toolCalls![0].id)}
                          onToggle={() => {
                            const id = chat.currentMessage()?.toolCalls?.[0]?.id;
                            if (id) toggleTool(id);
                          }}
                        />
                      </Show>
                    </div>
                  </Show>
                </div>
              </Show>
            </Show>
          </div>

          {/* Input Area - "The Fireside" */}
          <div class="shepherd-input-area px-3 pb-3 pt-2 shrink-0 border-t border-pasture-600/50">
            <Show when={pendingImages().length > 0}>
              <div class="mb-2">
                <div class="flex items-center justify-between text-[11px] text-wool-500 mb-1 px-1">
                  <span>{pendingImages().length} image(s) ready</span>
                  <span>paste from clipboard</span>
                </div>
                <div class="flex gap-2 overflow-x-auto pb-1">
                  <For each={pendingImages()}>
                    {(img) => (
                      <div class="relative shrink-0">
                        <img
                          src={img.previewUrl}
                          alt={img.name || 'pasted image'}
                          class="w-16 h-16 object-cover rounded border border-pasture-600"
                        />
                        <button
                          type="button"
                          onClick={() => removePendingImage(img.id)}
                          class="absolute -top-1 -right-1 w-5 h-5 rounded-full bg-pasture-900/90 border border-pasture-600 text-wool-300 hover:text-white flex items-center justify-center"
                          title="Remove image"
                        >
                          <Icon name="x" class="w-3 h-3" />
                        </button>
                      </div>
                    )}
                  </For>
                </div>
              </div>
            </Show>

            <div
              class="shepherd-input-wrapper relative overflow-hidden"
              classList={{ disabled: !chat.connected() || chat.shepherdEditing() }}
            >
              <textarea
                ref={inputRef}
                value={inputText()}
                onInput={(e) => setInputText(e.currentTarget.value)}
                onKeyDown={handleKeyDown}
                onPaste={handlePaste}
                placeholder={
                  chat.context().focusNodeName
                    ? `Ask about "${chat.context().focusNodeName}"...`
                    : 'Speak with Shepherd...'
                }
                disabled={!chat.connected() || chat.shepherdEditing()}
                class="w-full h-14 resize-none text-sm text-wool-100 placeholder-wool-600 placeholder:italic focus:outline-none p-3 pr-12 bg-transparent disabled:opacity-50"
                style="font-family: 'ET Book', serif;"
              />
              <button
                onClick={handleSend}
                disabled={
                  (!inputText().trim() && pendingImages().length === 0) ||
                  !chat.connected() ||
                  chat.shepherdEditing() ||
                  sending()
                }
                class="shepherd-send-btn absolute right-2 bottom-2 w-8 h-8 flex items-center justify-center"
              >
                <Show when={sending()}>
                  <div class="w-4 h-4 border-2 border-amber-200 border-t-transparent rounded-full animate-spin" />
                </Show>
                <Show when={!sending()}>
                  <Icon name="send" class="w-4 h-4 text-white" />
                </Show>
              </button>
            </div>
            <div class="shepherd-keyboard-hints flex items-center justify-between mt-2 px-1">
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
 * ShepherdConsoleBar - The part that lives in the StatusBar
 */
export const ShepherdConsoleBar: Component = () => {
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
          src="/shepherd.svg"
          alt="Shepherd"
          class="w-5 h-5"
          classList={{
            'opacity-60': !app.aiChatOpen(),
          }}
        />
        <span class="text-xs text-wool-400">Shepherd</span>
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
      class={`flex shepherd-message-enter ${isUser() ? 'justify-end' : 'justify-start'}`}
    >
      <div
        class={`max-w-[85%] px-3 py-2.5 ${
          isUser()
            ? 'shepherd-message-user text-wool-200 ml-8'
            : 'shepherd-message-assistant text-wool-300 mr-8'
        }`}
      >
        {/* Content */}
        <Show when={props.message.content}>
          <Markdown content={props.message.content} class="text-sm" />
        </Show>

        {/* Attached images */}
        <Show when={props.message.images?.length}>
          <div class="mt-2 flex flex-wrap gap-2">
            <For each={props.message.images || []}>
              {(img) => (
                <a href={img.src} target="_blank" rel="noreferrer" class="block">
                  <img
                    src={img.src}
                    alt={img.name || 'chat image'}
                    class="max-w-[180px] max-h-[180px] object-cover rounded border border-pasture-600/70 hover:border-pasture-400 transition-colors"
                  />
                </a>
              )}
            </For>
          </div>
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
