/**
 * SheepfoldDrawer - Project messaging side drawer
 *
 * Contains:
 * - Meadow (group chat with all workers + human)
 * - Worker DMs (direct messages via avatar clicks)
 */
import {
  type Component,
  For,
  Show,
  createEffect,
  createResource,
  createSignal,
  onCleanup,
} from 'solid-js';
import { useProject } from '../../stores';
import {
  getProjectMessages,
  getProjectThreads,
  markProjectMessagesRead,
  sendProjectMessage,
} from '../../lib/api';
import type { ProjectMessage, ProjectThreadSummary, WorkerDisplay } from '../../lib/types';
import { Icon, SheepAvatar } from '../shared';

interface SheepfoldDrawerProps {
  workers: WorkerDisplay[];
  onClose: () => void;
}

export const SheepfoldDrawer: Component<SheepfoldDrawerProps> = (props) => {
  const project = useProject();
  const [messageInput, setMessageInput] = createSignal('');
  const [sending, setSending] = createSignal(false);
  let messagesEndRef: HTMLDivElement | undefined;
  let inputRef: HTMLTextAreaElement | undefined;

  const projectId = () => project.selectedProjectId();
  const activeThread = () => project.activeThread();

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
    if (e.key === 'Escape') {
      props.onClose();
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
    <div class="w-96 flex flex-col border-l border-pasture-600/50 bg-pasture-900/95 backdrop-blur-sm">
      {/* Header */}
      <div class="flex items-center justify-between px-4 py-3 border-b border-pasture-600/50">
        <div class="flex items-center gap-2">
          <Icon name="messages-square" class="w-4 h-4 text-amber-500" />
          <span class="text-sm font-medium text-wool-200">Sheepfold</span>
        </div>
        <button
          type="button"
          onClick={props.onClose}
          class="p-1.5 rounded text-wool-500 hover:text-wool-300 hover:bg-pasture-800 transition-colors"
          title="Close"
        >
          <Icon name="x" class="w-4 h-4" />
        </button>
      </div>

      {/* Thread Tabs */}
      <div class="flex items-center gap-2 px-3 py-2 border-b border-pasture-700/50 overflow-x-auto scrollbar-none">
        {/* Meadow tab (group) */}
        <button
          type="button"
          onClick={() => project.setActiveThread('meadow')}
          class="relative flex items-center gap-1.5 px-2.5 py-1.5 rounded-full text-xs font-medium transition-all flex-shrink-0"
          classList={{
            'bg-amber-500/20 text-amber-300 border border-amber-500/30': activeThread() === 'meadow',
            'text-wool-400 hover:text-wool-200 hover:bg-pasture-800': activeThread() !== 'meadow',
          }}
        >
          <Icon name="users" class="w-3.5 h-3.5" />
          Meadow
          <Show when={getThreadUnread('meadow') > 0}>
            <span class="absolute -top-1 -right-1 w-4 h-4 rounded-full bg-amber-500 text-pasture-900 text-[10px] font-bold flex items-center justify-center">
              {getThreadUnread('meadow')}
            </span>
          </Show>
        </button>

        {/* Worker DM tabs */}
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
                  size={28}
                  status={worker.status}
                  class="rounded-full"
                />
                <Show when={unread() > 0}>
                  <span class="absolute -top-0.5 -right-0.5 w-3.5 h-3.5 rounded-full bg-amber-500 text-pasture-900 text-[9px] font-bold flex items-center justify-center">
                    {unread()}
                  </span>
                </Show>
              </button>
            );
          }}
        </For>
      </div>

      {/* Thread header */}
      <div class="px-4 py-2 border-b border-pasture-700/30">
        <Show when={activeThread() === 'meadow'}>
          <div class="flex items-center gap-2">
            <Icon name="users" class="w-4 h-4 text-wool-500" />
            <span class="text-sm font-medium text-wool-300">Meadow</span>
            <span class="text-xs text-wool-600">Group chat</span>
          </div>
        </Show>
        <Show when={activeThread() !== 'meadow'}>
          {(() => {
            const worker = () => getWorkerForThread(activeThread());
            return (
              <div class="flex items-center gap-2">
                <Show when={worker()}>
                  <SheepAvatar
                    config={worker()!.sheepConfig}
                    size={20}
                    status={worker()!.status}
                  />
                </Show>
                <span class="text-sm font-medium text-wool-300">{activeThread()}</span>
                <span class="text-xs text-wool-600">Direct message</span>
              </div>
            );
          })()}
        </Show>
      </div>

      {/* Messages */}
      <div class="flex-1 overflow-y-auto px-4 py-3 space-y-3">
        <Show when={messages.loading && !messages()}>
          <div class="flex items-center justify-center py-8">
            <Icon name="loader-2" class="w-5 h-5 text-wool-500 animate-spin" />
          </div>
        </Show>

        <Show when={!messages.loading && messages()?.length === 0}>
          <div class="flex flex-col items-center justify-center py-8 text-center">
            <Icon name="message-circle" class="w-8 h-8 text-wool-600 mb-2" />
            <p class="text-sm text-wool-500">No messages yet</p>
            <p class="text-xs text-wool-600 mt-1">
              {activeThread() === 'meadow'
                ? 'Start a conversation with the flock'
                : `Send a message to ${activeThread()}`}
            </p>
          </div>
        </Show>

        <For each={messages()}>
          {(msg) => {
            const isUser = () => msg.sender === 'user';
            const worker = () => (isUser() ? null : getWorkerForThread(msg.sender));

            return (
              <div
                class="flex gap-2"
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
                        class="w-7 h-7 rounded-full flex items-center justify-center"
                        style={{
                          background: isUser() ? 'var(--amber-500)' : 'var(--pasture-700)',
                        }}
                      >
                        <Icon
                          name={isUser() ? 'user' : 'bot'}
                          class={`w-4 h-4 ${isUser() ? 'text-pasture-900' : 'text-wool-400'}`}
                        />
                      </div>
                    }
                  >
                    <SheepAvatar
                      config={worker()!.sheepConfig}
                      size={28}
                      status={worker()!.status}
                    />
                  </Show>
                </div>

                {/* Message content */}
                <div
                  class="max-w-[75%] rounded-lg px-3 py-2"
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
                    <p class="text-[10px] font-medium text-wool-500 mb-0.5">{msg.sender}</p>
                  </Show>
                  <p class="text-sm text-wool-200 whitespace-pre-wrap break-words">{msg.content}</p>
                  <p class="text-[10px] text-wool-600 mt-1">{formatTimestamp(msg.timestamp)}</p>
                </div>
              </div>
            );
          }}
        </For>

        <div ref={messagesEndRef} />
      </div>

      {/* Input */}
      <div class="px-4 py-3 border-t border-pasture-700/50">
        <div
          class="flex items-end gap-2 rounded-lg p-2"
          style={{
            background: 'rgba(36, 36, 36, 0.6)',
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
            class="flex-1 bg-transparent text-sm text-wool-200 placeholder-wool-600 resize-none focus:outline-none"
            style={{ 'min-height': '36px', 'max-height': '120px' }}
            rows={1}
          />
          <button
            type="button"
            onClick={handleSend}
            disabled={!messageInput().trim() || sending()}
            class="p-2 rounded-md transition-colors disabled:opacity-40"
            style={{
              background: messageInput().trim() ? 'var(--amber-500)' : 'transparent',
              color: messageInput().trim() ? 'var(--pasture-900)' : 'var(--wool-500)',
            }}
          >
            <Icon name={sending() ? 'loader-2' : 'send'} class={`w-4 h-4 ${sending() ? 'animate-spin' : ''}`} />
          </button>
        </div>
        <p class="text-[10px] text-wool-600 mt-1.5 text-center">
          Press Enter to send, Shift+Enter for new line
        </p>
      </div>
    </div>
  );
};

export default SheepfoldDrawer;
