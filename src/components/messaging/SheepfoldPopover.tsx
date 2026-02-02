/**
 * SheepfoldPopover - Floating messaging popover
 *
 * A floating card that appears below the Flock pill for project messaging.
 * Contains Meadow (group chat) and worker DMs.
 */
import {
  type Component,
  For,
  Show,
  createEffect,
  createResource,
  createSignal,
  onCleanup,
  onMount,
} from 'solid-js';
import { useProject } from '../../stores';
import {
  getProjectMessages,
  getProjectThreads,
  markProjectMessagesRead,
  sendProjectMessage,
} from '../../lib/api';
import type { WorkerDisplay } from '../../lib/types';
import { Icon, SheepAvatar } from '../shared';

interface SheepfoldPopoverProps {
  workers: WorkerDisplay[];
  anchorRef?: HTMLDivElement;
  onClose: () => void;
}

export const SheepfoldPopover: Component<SheepfoldPopoverProps> = (props) => {
  const project = useProject();
  const [messageInput, setMessageInput] = createSignal('');
  const [sending, setSending] = createSignal(false);
  const [position, setPosition] = createSignal({ x: 0, y: 0 });
  let popoverRef: HTMLDivElement | undefined;
  let messagesEndRef: HTMLDivElement | undefined;
  let inputRef: HTMLTextAreaElement | undefined;

  const projectId = () => project.selectedProjectId();
  const activeThread = () => project.activeThread();

  // Position the popover below the anchor
  const updatePosition = () => {
    if (!props.anchorRef) return;
    const rect = props.anchorRef.getBoundingClientRect();
    const popoverWidth = 380;
    const x = rect.left + rect.width / 2 - popoverWidth / 2;
    const y = rect.bottom + 8;
    setPosition({ x: Math.max(12, x), y });
  };

  onMount(() => {
    updatePosition();
    window.addEventListener('resize', updatePosition);
    // Focus input when opened
    setTimeout(() => inputRef?.focus(), 100);
  });

  onCleanup(() => {
    window.removeEventListener('resize', updatePosition);
  });

  // Fetch threads for tab display
  const [threads, { refetch: refetchThreads }] = createResource(projectId, async (pid) => {
    if (!pid) return [];
    try {
      return await getProjectThreads(pid);
    } catch (e) {
      console.error('Failed to fetch threads:', e);
      return [];
    }
  });

  // Fetch messages for active thread
  const [messages, { refetch: refetchMessages }] = createResource(
    () => ({ pid: projectId(), thread: activeThread() }),
    async ({ pid, thread }) => {
      if (!pid) return [];
      try {
        return await getProjectMessages(pid, thread, 100);
      } catch (e) {
        console.error('Failed to fetch messages:', e);
        return [];
      }
    }
  );

  // Auto-scroll to bottom when messages change
  createEffect(() => {
    if (messages() && messagesEndRef) {
      messagesEndRef.scrollIntoView({ behavior: 'smooth' });
    }
  });

  // Mark messages as read when viewing thread
  createEffect(() => {
    const pid = projectId();
    const thread = activeThread();
    if (pid && project.sheepfoldOpen()) {
      markProjectMessagesRead(pid, thread).catch(console.error);
    }
  });

  // Poll for new messages
  createEffect(() => {
    if (!project.sheepfoldOpen()) return;

    const interval = setInterval(() => {
      refetchMessages();
      refetchThreads();
    }, 2000);

    onCleanup(() => clearInterval(interval));
  });

  // Close on escape
  createEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        props.onClose();
      }
    };
    document.addEventListener('keydown', handleKeyDown);
    onCleanup(() => document.removeEventListener('keydown', handleKeyDown));
  });

  // Close on click outside
  createEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (popoverRef && !popoverRef.contains(e.target as Node)) {
        // Don't close if clicking on anchor
        if (props.anchorRef?.contains(e.target as Node)) return;
        props.onClose();
      }
    };
    // Delay to avoid immediate close on open click
    setTimeout(() => {
      document.addEventListener('mousedown', handleClickOutside);
    }, 100);
    onCleanup(() => document.removeEventListener('mousedown', handleClickOutside));
  });

  const handleSend = async () => {
    const content = messageInput().trim();
    const pid = projectId();
    if (!content || !pid || sending()) return;

    setSending(true);
    try {
      await sendProjectMessage(pid, activeThread(), content);
      setMessageInput('');
      refetchMessages();
      refetchThreads();
      inputRef?.focus();
    } catch (e) {
      console.error('Failed to send message:', e);
      window.toast?.error('Failed to send message');
    } finally {
      setSending(false);
    }
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const getThreadUnread = (threadName: string) => {
    const t = threads()?.find((t) => t.thread === threadName);
    return t?.unreadCount || 0;
  };

  const getWorkerForThread = (threadName: string) => {
    if (threadName === 'meadow') return null;
    return props.workers.find((w) => w.name === threadName);
  };

  const formatTimestamp = (ts: string) => {
    try {
      const date = new Date(ts);
      const now = new Date();
      const isToday = date.toDateString() === now.toDateString();
      if (isToday) {
        return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
      }
      return date.toLocaleDateString([], { month: 'short', day: 'numeric' });
    } catch {
      return '';
    }
  };

  return (
    <>
      {/* Backdrop (subtle) */}
      <div
        class="fixed inset-0 z-40"
        style={{ background: 'rgba(0, 0, 0, 0.2)' }}
        onClick={props.onClose}
      />

      {/* Popover */}
      <div
        ref={popoverRef}
        class="fixed z-50 flex flex-col rounded-xl overflow-hidden"
        style={{
          left: `${position().x}px`,
          top: `${position().y}px`,
          width: '380px',
          height: '420px',
          background: 'linear-gradient(180deg, rgba(36, 36, 36, 0.98) 0%, rgba(26, 26, 26, 0.98) 100%)',
          border: '1px solid rgba(64, 64, 64, 0.5)',
          'box-shadow': '0 12px 40px rgba(0, 0, 0, 0.5), 0 0 0 1px rgba(255,255,255,0.03)',
          'backdrop-filter': 'blur(12px)',
        }}
      >
        {/* Arrow pointing up */}
        <div
          class="absolute -top-2 left-1/2 -translate-x-1/2 w-4 h-4 rotate-45"
          style={{
            background: 'rgba(36, 36, 36, 0.98)',
            border: '1px solid rgba(64, 64, 64, 0.5)',
            'border-bottom': 'none',
            'border-right': 'none',
          }}
        />

        {/* Header with thread tabs */}
        <div class="relative flex items-center gap-2 px-3 py-2.5 border-b border-pasture-600/40">
          {/* Meadow tab */}
          <button
            type="button"
            onClick={() => project.setActiveThread('meadow')}
            class="relative flex items-center gap-1.5 px-2.5 py-1.5 rounded-full text-[10px] font-medium transition-all"
            classList={{
              'bg-amber-500/20 text-amber-300 border border-amber-500/30': activeThread() === 'meadow',
              'text-wool-400 hover:text-wool-200 hover:bg-pasture-700': activeThread() !== 'meadow',
            }}
          >
            <Icon name="users" class="w-3 h-3" />
            Meadow
            <Show when={getThreadUnread('meadow') > 0}>
              <span
                class="ml-1 min-w-[14px] h-3.5 px-1 rounded-full text-[8px] font-bold flex items-center justify-center"
                style={{ background: 'var(--amber-500)', color: 'var(--pasture-900)' }}
              >
                {getThreadUnread('meadow')}
              </span>
            </Show>
          </button>

          {/* Worker tabs */}
          <div class="flex items-center gap-1 overflow-x-auto scrollbar-none flex-1">
            <For each={props.workers}>
              {(worker) => {
                const unread = () => getThreadUnread(worker.name);
                const isActive = () => activeThread() === worker.name;

                return (
                  <button
                    type="button"
                    onClick={() => project.setActiveThread(worker.name)}
                    class="relative flex-shrink-0 rounded-full transition-all"
                    classList={{
                      'ring-2 ring-amber-500/50': isActive(),
                    }}
                    title={worker.name}
                  >
                    <SheepAvatar
                      config={worker.sheepConfig}
                      size={24}
                      status={worker.status}
                      class="rounded-full"
                    />
                    <Show when={unread() > 0}>
                      <span
                        class="absolute -top-0.5 -right-0.5 min-w-[12px] h-3 px-0.5 rounded-full text-[7px] font-bold flex items-center justify-center"
                        style={{ background: 'var(--amber-500)', color: 'var(--pasture-900)' }}
                      >
                        {unread()}
                      </span>
                    </Show>
                  </button>
                );
              }}
            </For>
          </div>

          {/* Close button */}
          <button
            type="button"
            onClick={props.onClose}
            class="p-1 rounded text-wool-500 hover:text-wool-300 hover:bg-pasture-700 transition-colors"
            title="Close"
          >
            <Icon name="x" class="w-3.5 h-3.5" />
          </button>
        </div>

        {/* Thread header (compact) */}
        <div class="px-3 py-1.5 border-b border-pasture-700/30">
          <Show when={activeThread() === 'meadow'}>
            <div class="flex items-center gap-1.5">
              <Icon name="users" class="w-3.5 h-3.5 text-wool-500" />
              <span class="text-[11px] font-medium text-wool-300">Meadow</span>
              <span class="text-[10px] text-wool-600">· Group chat</span>
            </div>
          </Show>
          <Show when={activeThread() !== 'meadow'}>
            {(() => {
              const worker = () => getWorkerForThread(activeThread());
              return (
                <div class="flex items-center gap-1.5">
                  <Show when={worker()}>
                    <SheepAvatar
                      config={worker()!.sheepConfig}
                      size={16}
                      status={worker()!.status}
                    />
                  </Show>
                  <span class="text-[11px] font-medium text-wool-300">{activeThread()}</span>
                  <span class="text-[10px] text-wool-600">· DM</span>
                </div>
              );
            })()}
          </Show>
        </div>

        {/* Messages */}
        <div class="flex-1 overflow-y-auto px-3 py-2 space-y-2">
          <Show when={messages.loading && !messages()}>
            <div class="flex items-center justify-center py-6">
              <Icon name="loader-2" class="w-4 h-4 text-wool-500 animate-spin" />
            </div>
          </Show>

          <Show when={!messages.loading && messages()?.length === 0}>
            <div class="flex flex-col items-center justify-center py-6 text-center">
              <Icon name="message-circle" class="w-6 h-6 text-wool-600 mb-1.5" />
              <p class="text-[11px] text-wool-500">No messages yet</p>
              <p class="text-[10px] text-wool-600 mt-0.5">
                {activeThread() === 'meadow'
                  ? 'Start a conversation'
                  : `Message ${activeThread()}`}
              </p>
            </div>
          </Show>

          <For each={messages()}>
            {(msg) => {
              const isUser = () => msg.sender === 'user';
              const worker = () => (isUser() ? null : getWorkerForThread(msg.sender));

              return (
                <div
                  class="flex gap-1.5"
                  classList={{
                    'flex-row-reverse': isUser(),
                  }}
                >
                  {/* Avatar */}
                  <div class="flex-shrink-0">
                    <Show
                      when={!isUser() && worker()}
                      fallback={
                        <div
                          class="w-6 h-6 rounded-full flex items-center justify-center"
                          style={{
                            background: isUser() ? 'var(--amber-500)' : 'var(--pasture-700)',
                          }}
                        >
                          <Icon
                            name={isUser() ? 'user' : 'bot'}
                            class={`w-3 h-3 ${isUser() ? 'text-pasture-900' : 'text-wool-400'}`}
                          />
                        </div>
                      }
                    >
                      <SheepAvatar
                        config={worker()!.sheepConfig}
                        size={24}
                        status={worker()!.status}
                      />
                    </Show>
                  </div>

                  {/* Message content */}
                  <div
                    class="max-w-[70%] rounded-lg px-2.5 py-1.5"
                    style={{
                      background: isUser()
                        ? 'rgba(212, 165, 116, 0.15)'
                        : 'rgba(36, 36, 36, 0.8)',
                      border: isUser()
                        ? '1px solid rgba(212, 165, 116, 0.2)'
                        : '1px solid rgba(64, 64, 64, 0.4)',
                    }}
                  >
                    {/* Sender name (for non-user in group chat) */}
                    <Show when={!isUser() && activeThread() === 'meadow'}>
                      <p class="text-[9px] font-medium text-wool-500 mb-0.5">{msg.sender}</p>
                    </Show>
                    <p class="text-[12px] text-wool-200 whitespace-pre-wrap break-words leading-relaxed">
                      {msg.content}
                    </p>
                    <p class="text-[9px] text-wool-600 mt-0.5">{formatTimestamp(msg.timestamp)}</p>
                  </div>
                </div>
              );
            }}
          </For>

          <div ref={messagesEndRef} />
        </div>

        {/* Input */}
        <div class="px-3 py-2.5 border-t border-pasture-700/40">
          <div
            class="flex items-end gap-2 rounded-lg p-2"
            style={{
              background: 'rgba(26, 26, 26, 0.6)',
              border: '1px solid rgba(64, 64, 64, 0.4)',
            }}
          >
            <textarea
              ref={inputRef}
              value={messageInput()}
              onInput={(e) => setMessageInput(e.currentTarget.value)}
              onKeyDown={handleKeyDown}
              placeholder={
                activeThread() === 'meadow'
                  ? 'Message the flock...'
                  : `Message ${activeThread()}...`
              }
              class="flex-1 bg-transparent text-[12px] text-wool-200 placeholder-wool-600 resize-none focus:outline-none"
              style={{ 'min-height': '32px', 'max-height': '80px' }}
              rows={1}
            />
            <button
              type="button"
              onClick={handleSend}
              disabled={!messageInput().trim() || sending()}
              class="p-1.5 rounded-md transition-colors disabled:opacity-40"
              style={{
                background: messageInput().trim() ? 'var(--amber-500)' : 'transparent',
                color: messageInput().trim() ? 'var(--pasture-900)' : 'var(--wool-500)',
              }}
            >
              <Icon name={sending() ? 'loader-2' : 'send'} class={`w-3.5 h-3.5 ${sending() ? 'animate-spin' : ''}`} />
            </button>
          </div>
        </div>
      </div>
    </>
  );
};

export default SheepfoldPopover;
