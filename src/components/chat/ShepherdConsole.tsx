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
import type { ChatMessage, ShepherdImageInput } from '../../lib/types';
import { emit, on as onEvent } from '../../lib/events';
import { invoke } from '../../lib/invoke';
import { useProject, useWorkspace } from '../../stores';
import { useShepherdChat, type ShepherdChatContext } from '../../hooks/use-shepherd-chat';
import { Icon, Markdown, ThinkingBlock, ToolCard, ToolCluster, type ToolInfo } from '../shared';
const MAX_PASTED_IMAGES = 8;

interface SkillSummary {
  name: string;
  description: string;
  source: string;
}

interface SlashCommand {
  name: string;
  description: string;
  type: 'builtin' | 'skill';
}

const BUILTIN_COMMANDS: SlashCommand[] = [
  { name: 'clear', description: 'Clear chat history', type: 'builtin' },
  { name: 'reset', description: 'Reset chat session', type: 'builtin' },
  { name: 'help', description: 'Show keyboard shortcuts', type: 'builtin' },
  { name: 'skills', description: 'List installed skills', type: 'builtin' },
];

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
  const workspace = useWorkspace();
  let messagesRef: HTMLDivElement | undefined;
  let inputRef: HTMLTextAreaElement | undefined;
  let menuBtnRef: HTMLButtonElement | undefined;
  let connectionVersion = 0;

  const [inputText, setInputText] = createSignal('');
  const [sending, setSending] = createSignal(false);
  const [pendingImages, setPendingImages] = createSignal<PendingImage[]>([]);
  const [expandedTools, setExpandedTools] = createSignal<Set<string>>(new Set());
  const [menuOpen, setMenuOpen] = createSignal(false);
  const [setupPromptedProjectId, setSetupPromptedProjectId] = createSignal<number | null>(null);

  // Smart scroll: track whether user has scrolled up
  const [userScrolledUp, setUserScrolledUp] = createSignal(false);

  // Slash command autocomplete
  const [skills, setSkills] = createSignal<SkillSummary[]>([]);
  const [autocompleteIndex, setAutocompleteIndex] = createSignal(0);

  // Load skills on mount
  createEffect(() => {
    void invoke<SkillSummary[]>('list_shepherd_skills')
      .then(setSkills)
      .catch(() => setSkills([]));
  });

  // Build unified command list (builtins + skills)
  const allCommands = (): SlashCommand[] => [
    ...BUILTIN_COMMANDS,
    ...skills().map((s): SlashCommand => ({
      name: s.name,
      description: s.description,
      type: 'skill',
    })),
  ];

  // Filtered autocomplete items (when input starts with /)
  const autocompleteItems = () => {
    const text = inputText();
    if (!text.startsWith('/')) return [];
    const query = text.slice(1).toLowerCase();
    return allCommands().filter((cmd) => cmd.name.toLowerCase().startsWith(query));
  };

  const showAutocomplete = () => autocompleteItems().length > 0 && inputText().startsWith('/');

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
    onEditComplete: () => undefined,
  });

  const ensureAssistantConfigured = async (showFlow: boolean) => {
    const config: {
      llm?: { provider?: 'codex' | 'openrouter' };
    } = await invoke<{
      llm?: { provider?: 'codex' | 'openrouter' };
    }>('get_config').catch(() => ({}));
    const provider = config.llm?.provider || 'codex';

    if (provider === 'openrouter') {
      const hasOpenrouter = await invoke<boolean>('has_credential', {
        keyType: 'openrouter_api_key',
      }).catch(() => false);
      if (hasOpenrouter) {
        return true;
      }

      if (showFlow) {
        emit('open-backend-settings', { section: 'llm' });
        window.toast?.error('Configure OpenRouter in Settings before using Shepherd.');
      }
      return false;
    }

    const [hasAccessToken, hasRefreshToken] = await Promise.all([
      invoke<boolean>('has_credential', { keyType: 'codex_access_token' }).catch(() => false),
      invoke<boolean>('has_credential', { keyType: 'codex_refresh_token' }).catch(() => false),
    ]);

    if (hasAccessToken && hasRefreshToken) {
      return true;
    }

    if (showFlow) {
      emit('open-backend-settings', { section: 'llm' });
      window.toast?.error('Connect Codex in Settings before using Shepherd.');
    }
    return false;
  };

  const startProjectSync = async (force: boolean) => {
    const projectId = project.selectedProjectId();
    if (!projectId) return;

    if (!(await ensureAssistantConfigured(true))) {
      return;
    }

    if (!chat.connected()) {
      await chat.connect();
    }
    if (!chat.connected()) {
      window.toast?.error('Shepherd is not connected');
      return;
    }

    try {
      const result = await invoke<{ item: { id: string; title: string }; prompt: string }>('ensure_sync_project_task_cmd', {
        projectId,
        requestSync: true,
        refresh: force,
      });
      await chat.sendBackgroundPrompt(result.prompt, {
        taskId: result.item.id,
        taskName: result.item.title,
      });
    } catch (error) {
      console.error('Failed to sync project:', error);
      window.toast?.error(`Failed to sync project: ${error}`);
    }
  };

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

  createEffect(
    on(
      () => project.selectedProjectId(),
      () => setSetupPromptedProjectId(null),
    ),
  );

  createEffect(
    on(
      () => ({ projectId: project.selectedProjectId(), connected: chat.connected() }),
      ({ projectId, connected }) => {
        if (!projectId || !connected || setupPromptedProjectId() === projectId) {
          return;
        }

        void (async () => {
          const ready = await ensureAssistantConfigured(false);
          if (!ready) {
            setSetupPromptedProjectId(projectId);
            emit('open-backend-settings', { section: 'llm' });
          }
        })();
      },
      { defer: true },
    ),
  );

  // Smart scroll: only auto-scroll when user hasn't scrolled up
  const scrollToBottom = () => {
    if (messagesRef) {
      messagesRef.scrollTop = messagesRef.scrollHeight;
    }
  };

  const handleMessagesScroll = () => {
    if (!messagesRef) return;
    const threshold = 60;
    const atBottom = messagesRef.scrollTop + messagesRef.clientHeight >= messagesRef.scrollHeight - threshold;
    setUserScrolledUp(!atBottom);
  };

  createEffect(() => {
    chat.messages.length;
    chat.currentMessage();
    if (!userScrolledUp()) {
      scrollToBottom();
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

  // Listen for focused-work-item events from the active project surface
  createEffect(() => {
    const cleanup = onEvent('shepherd-focus-node', (detail) => {
      chat.setFocusNode(detail.id, detail.name);
    });
    onCleanup(cleanup);
  });

  createEffect(() => {
    const cleanup = onEvent('start-project-sync', (detail) => {
      void startProjectSync(detail?.force ?? true);
    });
    onCleanup(cleanup);
  });

  // Dispatch shepherd-editing-islands events when editing state changes
  createEffect(() => {
    const islands = chat.editingIslands();
    emit('shepherd-editing-islands', islands);
  });

  // Insert a local system-style message (not sent to backend)
  const insertLocalMessage = (content: string) => {
    const msg: ChatMessage = {
      id: `sys-${Date.now()}`,
      role: 'assistant',
      content,
      timestamp: new Date(),
    };
    // We push it directly via the store import — but since messages is from hook,
    // we'll just use chat.sendMessage with a special prefix. Instead, show via toast.
    // Actually, let's just show via toast for builtins.
    window.toast?.success(content);
  };

  // Handle slash commands
  const handleSlashCommand = async (text: string): Promise<boolean> => {
    if (!text.startsWith('/')) return false;

    const spaceIdx = text.indexOf(' ');
    const cmdName = spaceIdx === -1 ? text.slice(1) : text.slice(1, spaceIdx);
    const args = spaceIdx === -1 ? '' : text.slice(spaceIdx + 1).trim();

    // Built-in commands
    switch (cmdName) {
      case 'clear':
        await chat.clearHistory();
        window.toast?.success('History cleared');
        return true;
      case 'reset':
        await chat.reset();
        window.toast?.success('Chat reset');
        return true;
      case 'help':
        window.toast?.success(
          'Enter: send · Shift+Enter: newline · Esc: stop · Cmd+Shift+A: focus · /skills: list skills',
        );
        return true;
      case 'skills': {
        const skillList = skills();
        if (skillList.length === 0) {
          window.toast?.success('No skills installed. Add folders with SKILL.md to ~/.hirsel/skills/');
        } else {
          window.toast?.success(
            `${skillList.length} skill(s): ${skillList.map((s) => s.name).join(', ')}`,
          );
        }
        return true;
      }
      default:
        break;
    }

    // Check if it's a skill invocation
    const skill = skills().find((s) => s.name === cmdName);
    if (skill) {
      try {
        const detail = await invoke<{ name: string; content: string }>('get_shepherd_skill', {
          name: cmdName,
        });
        // Prepend skill content to user's message args
        const skillPrompt = `<skill name="${detail.name}">\n${detail.content}\n</skill>\n\n${args}`;
        setInputText('');
        setPendingImages([]);
        resetTextareaHeight();
        setSending(true);
        try {
          await chat.sendMessage(skillPrompt, []);
        } finally {
          setSending(false);
        }
        return true;
      } catch (e) {
        window.toast?.error(`Failed to load skill "${cmdName}": ${e}`);
        return true;
      }
    }

    return false; // Not a recognized command — send as normal message
  };

  // Handle send
  const handleSend = async () => {
    const text = inputText().trim();
    const images = pendingImages();
    if ((!text && images.length === 0) || sending()) return;

    // Handle slash commands first
    if (text.startsWith('/') && images.length === 0) {
      const handled = await handleSlashCommand(text);
      if (handled) {
        setInputText('');
        resetTextareaHeight();
        return;
      }
    }

    if (!(await ensureAssistantConfigured(true))) {
      return;
    }

    setSending(true);
    setInputText('');
    setPendingImages([]);
    resetTextareaHeight();
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

  // Textarea auto-resize
  const resizeTextarea = () => {
    if (!inputRef) return;
    inputRef.style.height = 'auto';
    inputRef.style.height = `${Math.min(inputRef.scrollHeight, 160)}px`;
  };

  const resetTextareaHeight = () => {
    if (!inputRef) return;
    inputRef.style.height = '40px';
  };

  const handleInput = (e: InputEvent & { currentTarget: HTMLTextAreaElement }) => {
    setInputText(e.currentTarget.value);
    resizeTextarea();
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
    // Escape: cancel active turn or close autocomplete
    if (e.key === 'Escape') {
      if (chat.turnActive()) {
        e.preventDefault();
        void chat.cancelTurn();
        return;
      }
      if (showAutocomplete()) {
        e.preventDefault();
        setInputText('');
        return;
      }
    }

    // Autocomplete navigation
    if (showAutocomplete()) {
      const items = autocompleteItems();
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setAutocompleteIndex((i) => (i + 1) % items.length);
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        setAutocompleteIndex((i) => (i - 1 + items.length) % items.length);
        return;
      }
      if (e.key === 'Tab') {
        e.preventDefault();
        const item = items[autocompleteIndex()];
        if (item) {
          setInputText(`/${item.name} `);
          resizeTextarea();
        }
        return;
      }
    }

    // Enter (no shift): send or queue
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  // Reset autocomplete index when input changes
  createEffect(() => {
    inputText();
    setAutocompleteIndex(0);
  });

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

  // Copy message content to clipboard
  const copyMessage = (content: string) => {
    void navigator.clipboard.writeText(content).then(() => {
      window.toast?.success('Copied to clipboard');
    });
  };

  // Recent messages (last 50)
  const recentMessages = () => chat.messages.slice(-50);

  // Placeholder text
  const placeholder = () => {
    if (chat.turnActive()) return 'Shepherd is working... (Esc to stop, Enter to queue)';
    return 'Message Shepherd...';
  };

  return (
    <section
      class="h-full min-h-0 shepherd-chat-panel overflow-hidden flex flex-col bg-pasture-900/96"
      classList={{
        'shepherd-editing': chat.shepherdEditing(),
      }}
    >
          {/* Toolbar */}
          <div class="px-2 py-1 shrink-0 border-b border-pasture-700/30 flex items-center justify-between">
            <div class="flex items-center gap-2">
              {/* Connection indicator */}
              <span
                class="w-1.5 h-1.5"
                classList={{
                  'bg-sage': chat.connected() && !chat.turnActive(),
                  'bg-amber-500 animate-pulse': chat.turnActive() || chat.connecting(),
                  'bg-wool-700': !chat.connected() && !chat.connecting(),
                }}
              />
              {/* Focused work item indicator */}
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
              {/* Queue count */}
              <Show when={chat.queuedCount() > 0}>
                <span class="text-[9px] uppercase tracking-[0.15em] text-wool-500 bg-pasture-700/60 px-1.5 py-0.5">
                  Queued {chat.queuedCount()}
                </span>
              </Show>
            </div>

            {/* Options menu */}
            <div class="flex items-center gap-0.5">
              <button
                type="button"
                onClick={() => workspace.setShepherdMinimized(true)}
                class="shepherd-options-btn"
                title="Minimize chat"
              >
                <Icon name="panel-right-close" class="w-4 h-4" />
              </button>
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
                    class="fixed z-[1002] w-48 bg-popover border border-border rounded-none shadow-md py-1"
                    style={menuPosition()}
                  >
                    <div role="listbox">
                      <div
                        role="option"
                        class="px-3 py-2 text-sm cursor-pointer hover:bg-accent flex items-center gap-2"
                        onClick={async () => {
                          setMenuOpen(false);
                          await chat.clearHistory();
                          window.toast?.success('History cleared');
                        }}
                      >
                        <Icon name="trash-2" class="w-3.5 h-3.5" />
                        <span>Clear history</span>
                      </div>
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
                      <div
                        role="option"
                        class="px-3 py-2 text-sm cursor-pointer hover:bg-accent flex items-center gap-2"
                        onClick={() => {
                          setMenuOpen(false);
                          const last = recentMessages().filter(m => m.role === 'assistant').pop();
                          if (last?.content) {
                            copyMessage(last.content);
                          }
                        }}
                      >
                        <Icon name="clipboard" class="w-3.5 h-3.5" />
                        <span>Copy last response</span>
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
            onScroll={handleMessagesScroll}
            class="shepherd-messages-area flex-1 overflow-y-auto px-4 py-3 space-y-3 relative"
          >
            {/* Empty state */}
            <Show when={chat.connected() && recentMessages().length === 0 && !chat.currentMessage()}>
              <div class="shepherd-empty-state h-full">
                <div class="shepherd-decorative-dots mb-3">
                  <span /><span /><span />
                </div>
                <p class="text-[10px] uppercase tracking-[0.25em] text-wool-700">
                  Ready
                </p>
              </div>
            </Show>

            {/* Connecting state */}
            <Show when={chat.connecting()}>
              <div class="shepherd-empty-state h-full">
                <div class="flex items-center gap-2 text-[10px] uppercase tracking-[0.25em] text-wool-700">
                  <div class="w-2.5 h-2.5 border border-wool-700 border-t-wool-400 animate-spin" />
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
                    onCopy={copyMessage}
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

            {/* Scroll-to-bottom button */}
            <Show when={userScrolledUp()}>
              <button
                type="button"
                onClick={() => {
                  setUserScrolledUp(false);
                  scrollToBottom();
                }}
                class="sticky bottom-2 left-1/2 -translate-x-1/2 z-10 w-7 h-7 flex items-center justify-center bg-pasture-800 border border-pasture-600 text-wool-400 hover:text-wool-100 hover:bg-pasture-700 shadow-lg"
              >
                <Icon name="chevron-down" class="w-4 h-4" />
              </button>
            </Show>
          </div>

          {/* Input */}
          {/* Slash command autocomplete */}
          <Show when={showAutocomplete()}>
            <div class="px-2 py-1 border-t border-pasture-700/30 max-h-48 overflow-y-auto">
              <For each={autocompleteItems()}>
                {(item, index) => (
                  <button
                    type="button"
                    class="w-full flex items-center gap-2 px-2 py-1.5 text-left text-[12px]"
                    classList={{
                      'bg-pasture-700/50 text-wool-100': autocompleteIndex() === index(),
                      'text-wool-400 hover:bg-pasture-700/30': autocompleteIndex() !== index(),
                    }}
                    onMouseEnter={() => setAutocompleteIndex(index())}
                    onClick={() => {
                      setInputText(`/${item.name} `);
                      resizeTextarea();
                      inputRef?.focus();
                    }}
                  >
                    <span class="text-wool-500">/{item.name}</span>
                    <Show when={item.description}>
                      <span class="text-wool-600 text-[11px] truncate">{item.description}</span>
                    </Show>
                    <Show when={item.type === 'skill'}>
                      <span class="ml-auto text-[9px] uppercase tracking-[0.1em] text-wool-700">skill</span>
                    </Show>
                  </button>
                )}
              </For>
            </div>
          </Show>

          <div class="shepherd-input-area px-2 pb-2 pt-1 shrink-0 border-t border-pasture-700/30">
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
              class="shepherd-input-wrapper relative"
              classList={{ disabled: !chat.connected() }}
            >
              <textarea
                ref={inputRef}
                value={inputText()}
                onInput={handleInput}
                onKeyDown={handleKeyDown}
                onPaste={handlePaste}
                placeholder={placeholder()}
                disabled={!chat.connected()}
                class="w-full min-h-[40px] max-h-[160px] resize-none text-sm text-wool-100 placeholder-wool-700 focus:outline-none p-2 pr-10 bg-transparent disabled:opacity-50"
                style={{ height: '40px' }}
              />
              {/* Send / Stop button */}
              <button
                type="button"
                onClick={() => {
                  if (chat.turnActive()) {
                    void chat.cancelTurn();
                  } else {
                    handleSend();
                  }
                }}
                disabled={!chat.connected() || (!chat.turnActive() && !inputText().trim() && pendingImages().length === 0)}
                class="shepherd-send-btn absolute right-1.5 bottom-1.5"
                title={chat.turnActive() ? 'Stop (Esc)' : 'Send (Enter)'}
              >
                <Show
                  when={chat.turnActive()}
                  fallback={<Icon name="arrow-up" class="w-3.5 h-3.5" />}
                >
                  <Icon name="square" class="w-3 h-3" />
                </Show>
              </button>
            </div>
          </div>
    </section>
  );
};

// Message bubble component
const MessageBubble: Component<{
  message: ChatMessage;
  expandedTools: Set<string>;
  onToggleTool: (id: string) => void;
  onCopy: (content: string) => void;
  isLatest: boolean;
}> = (props) => {
  const isUser = () => props.message.role === 'user';

  return (
    <div
      class={`flex shepherd-message-enter group ${isUser() ? 'justify-end' : 'justify-start'}`}
    >
      <div
        class={`max-w-[85%] px-3 py-2.5 relative ${
          isUser()
            ? 'shepherd-message-user text-wool-200 ml-8'
            : 'shepherd-message-assistant text-wool-300 mr-8'
        }`}
      >
        {/* Thinking block */}
        <Show when={!isUser() && props.message.thinking}>
          <div class="mb-2">
            <ThinkingBlock content={props.message.thinking!} />
          </div>
        </Show>

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

        {/* Copy button (assistant messages only) */}
        <Show when={!isUser() && props.message.content}>
          <button
            type="button"
            onClick={() => props.onCopy(props.message.content)}
            class="absolute top-1.5 right-1.5 p-1 text-wool-700 hover:text-wool-400 opacity-0 group-hover:opacity-100 transition-opacity"
            title="Copy"
          >
            <Icon name="clipboard" class="w-3 h-3" />
          </button>
        </Show>
      </div>
    </div>
  );
};
