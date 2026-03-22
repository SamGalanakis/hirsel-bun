/**
 * ShepherdConsole - "The Cognitive Layer" - AI orchestration console
 */
import {
  type Component,
  For,
  Show,
  createEffect,
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
import { useProject } from '../../stores';
import { useShepherdChat, type ShepherdChatContext } from '../../hooks/use-shepherd-chat';
import { Icon, Markdown, ToolCard, ToolCluster, type ToolInfo } from '../shared';
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
  const project = useProject();
  let messagesRef: HTMLDivElement | undefined;
  let inputRef: HTMLTextAreaElement | undefined;
  let menuBtnRef: HTMLButtonElement | undefined;
  let connectionVersion = 0;

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

    if (projectId) {
      return { type: 'project', projectId };
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

  // Each project owns its own Shepherd conversation.
  createEffect(
    on(
      () => project.selectedProjectId(),
      (projectId) => {
        const version = ++connectionVersion;
        void (async () => {
          await chat.disconnect();
          if (projectId !== null && version === connectionVersion) {
            await chat.connect();
          }
        })();
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

  // Focus input when the docked pane is ready
  createEffect(() => {
    if (chat.connected()) {
      setTimeout(() => inputRef?.focus(), 100);
    }
  });

  // Keyboard shortcut handler for Cmd+Shift+A
  createEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === 'a') {
        e.preventDefault();
        inputRef?.focus();
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
    <section
      class="h-full min-h-0 shepherd-chat-panel overflow-hidden flex flex-col bg-pasture-900/96"
      classList={{
        'shepherd-editing': chat.shepherdEditing(),
      }}
    >
          {/* Toolbar */}
          <div class="px-2 py-1 shrink-0 border-b border-pasture-700/40 flex items-center justify-between">
            <div class="flex items-center gap-2">
              {/* Connection indicator */}
              <span
                class="w-1.5 h-1.5"
                classList={{
                  'bg-sage': chat.connected(),
                  'bg-wool-700': !chat.connected() && !chat.connecting(),
                  'bg-amber-500 animate-pulse': chat.connecting(),
                }}
              />
              {/* Focus node indicator */}
              <Show when={chat.context().focusNodeName}>
                <div class="shepherd-focus-indicator max-w-[180px]">
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
            </div>

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
                    class="fixed z-[1002] w-44 bg-popover border border-border rounded-none shadow-md py-1"
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
          </div>

          {/* Messages Area */}
          <div
            ref={messagesRef}
            class="shepherd-messages-area flex-1 overflow-y-auto px-4 py-3 space-y-3"
          >
            {/* Empty state */}
            <Show when={chat.connected() && recentMessages().length === 0 && !chat.currentMessage()}>
              <div class="shepherd-empty-state h-full">
                <p class="text-[11px] uppercase tracking-[0.15em] text-wool-700">
                  No messages yet
                </p>
              </div>
            </Show>

            {/* Connecting state */}
            <Show when={chat.connecting()}>
              <div class="shepherd-empty-state h-full">
                <div class="flex items-center gap-2 text-[11px] uppercase tracking-[0.15em] text-wool-700">
                  <div class="w-3 h-3 border border-wool-700 border-t-wool-300 animate-spin" />
                  <span>Connecting</span>
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
                    <div class="flex items-center gap-2 text-[11px] text-wool-500 mb-2">
                      <div class="w-3 h-3 border border-wool-500 border-t-wool-100 animate-spin" />
                      <span class="uppercase tracking-[0.1em]">Editing</span>
                    </div>
                  </Show>
                  <Show when={chat.currentMessage()?.toolCalls?.length && !chat.shepherdEditing()}>
                    <div class="flex items-center gap-2 text-[11px] text-wool-500 mb-2">
                      <div class="w-3 h-3 border border-wool-500 border-t-wool-100 animate-spin" />
                      <span class="uppercase tracking-[0.1em]">Processing</span>
                    </div>
                  </Show>
                  <Show when={chat.currentMessage()?.content}>
                    <Markdown content={chat.currentMessage()?.content || ''} class="text-sm text-wool-300" />
                  </Show>
                  <Show when={!chat.currentMessage()?.content && !(chat.currentMessage()?.toolCalls?.length)}>
                    <div class="flex items-center gap-2 text-[11px] text-wool-500">
                      <div class="w-3 h-3 border border-wool-500 border-t-wool-100 animate-spin" />
                      <span class="uppercase tracking-[0.1em]">Thinking</span>
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

          {/* Input */}
          <div class="shepherd-input-area px-2 pb-2 pt-1 shrink-0 border-t border-pasture-700/40">
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
                          class="w-16 h-16 object-cover rounded-none border border-pasture-600"
                        />
                        <button
                          type="button"
                          onClick={() => removePendingImage(img.id)}
                          class="absolute -top-1 -right-1 w-5 h-5 rounded-none bg-pasture-900/90 border border-pasture-600 text-wool-300 hover:text-white flex items-center justify-center"
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
                placeholder=""
                disabled={!chat.connected() || chat.shepherdEditing()}
                class="w-full h-10 resize-none text-sm text-wool-100 placeholder-wool-700 focus:outline-none p-2 bg-transparent disabled:opacity-50"
              />
            </div>
          </div>
    </section>
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
                    class="max-w-[180px] max-h-[180px] object-cover rounded-none border border-pasture-600/70 hover:border-pasture-400 transition-colors"
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
