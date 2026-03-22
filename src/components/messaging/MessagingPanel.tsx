/**
 * MessagingPanel - Right drawer for project messaging
 *
 * A refined side panel for route chat and worker DMs.
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
import { useProject, useRoute, useRuns, useWorkspace } from '../../stores';
import { useDelta } from '../../stores/delta-context';
import {
  getProjectMessages,
  getProjectThreads,
  markProjectMessagesRead,
  sendProjectMessage,
} from '../../lib/api';
import { Icon, WorkerAvatar } from '../shared';
import { amber } from '../../lib/theme-colors';

export const MessagingPanel: Component = () => {
  const project = useProject();
  const route = useRoute();
  const workspace = useWorkspace();
  const delta = useDelta();
  const runsCtx = useRuns();
  const [messageInput, setMessageInput] = createSignal('');
  const [sending, setSending] = createSignal(false);
  let messagesEndRef: HTMLDivElement | undefined;
  let inputRef: HTMLTextAreaElement | undefined;

  const projectId = () => project.selectedProjectId();
  const routeId = () => route.currentRouteId();
  const activeThread = () => workspace.activeThread();
  const tabIsActive = () =>
    workspace.machineryOpen() && workspace.activeMachineryTab() === 'workers';

  // Workers — read from RunsContext store (centralized polling)
  const workers = () => {
    const run = delta.projectRun();
    if (!run) return [];
    if (runsCtx.selectedRun() === run.runName) return runsCtx.workers();
    return [];
  };

  createEffect(() => {
    const run = delta.projectRun();
    if (!run) return;
    if (!tabIsActive()) return;
    if (runsCtx.selectedRun() !== run.runName) {
      runsCtx.setSelectedRun(run.runName);
    }
  });

  // Fetch threads for tab display
  const [threads, { refetch: refetchThreads }] = createResource(
    () => ({ pid: projectId(), rid: routeId() }),
    async ({ pid, rid }) => {
      if (!pid || !rid) return [];
      try {
        return await getProjectThreads(pid, rid);
      } catch (e) {
        console.error('Failed to fetch threads:', e);
        return [];
      }
    }
  );

  // Fetch messages for active thread
  const [messages, { refetch: refetchMessages }] = createResource(
    () => ({ pid: projectId(), rid: routeId(), thread: activeThread() }),
    async ({ pid, rid, thread }) => {
      if (!pid || !rid) return [];
      try {
        return await getProjectMessages(pid, rid, thread, 100);
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
    const rid = routeId();
    const thread = activeThread();
    if (pid && rid && tabIsActive()) {
      markProjectMessagesRead(pid, rid, thread).catch(console.error);
    }
  });

  // Poll for new messages
  createEffect(() => {
    if (!tabIsActive()) return;

    const interval = setInterval(() => {
      refetchMessages();
      refetchThreads();
    }, 2000);

    onCleanup(() => clearInterval(interval));
  });

  // Focus input when opened
  createEffect(() => {
    if (tabIsActive()) {
      setTimeout(() => inputRef?.focus(), 100);
    }
  });

  const handleSend = async () => {
    const content = messageInput().trim();
    const pid = projectId();
    const rid = routeId();
    if (!content || !pid || !rid || sending()) return;

    setSending(true);
    try {
      await sendProjectMessage(pid, rid, activeThread(), content);
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
    if (threadName === 'chat') return null;
    return workers().find((w) => w.name === threadName);
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

  const handleClose = () => {
    workspace.setMachineryOpen(false);
  };

  return (
    <div class="h-full flex flex-col bg-pasture-900/95 backdrop-blur-sm">
      {/* Header */}
      <div class="flex items-center justify-between px-3 py-2 border-b border-pasture-600/50">
        <div class="flex items-center gap-2">
          <Icon name="message-circle" class="w-4 h-4 text-amber-500" />
          <span class="text-sm font-medium text-wool-200">Workers & Chat</span>
        </div>
        <button
          type="button"
          onClick={handleClose}
          class="p-1.5 rounded-none text-wool-500 hover:text-wool-300 hover:bg-pasture-800 transition-colors"
          title="Hide machinery"
        >
          <Icon name="x" class="w-4 h-4" />
        </button>
      </div>

      {/* Thread tabs */}
      <div class="flex items-center gap-1 px-2 py-2 border-b border-pasture-700/50 overflow-x-auto scrollbar-none">
        {/* Meadow tab */}
        <button
          type="button"
          onClick={() => workspace.setActiveThread('chat')}
          class="relative flex items-center gap-1.5 px-2.5 py-1.5 rounded-none text-xs font-medium transition-all whitespace-nowrap"
          classList={{
            'bg-amber-500/15 text-amber-300': activeThread() === 'chat',
            'text-wool-400 hover:text-wool-200 hover:bg-pasture-700': activeThread() !== 'chat',
          }}
        >
          <Icon name="users" class="w-3.5 h-3.5" />
          <span>Chat</span>
          <Show when={getThreadUnread('chat') > 0}>
            <span
              class="ml-1 min-w-[16px] h-4 px-1 rounded-none text-[9px] font-bold flex items-center justify-center"
              style={{ background: 'var(--amber-500)', color: 'var(--pasture-900)' }}
            >
              {getThreadUnread('chat')}
            </span>
          </Show>
        </button>

        {/* Worker tabs */}
        <For each={workers()}>
          {(worker) => {
            const unread = () => getThreadUnread(worker.name);
            const isActive = () => activeThread() === worker.name;

            return (
              <button
                type="button"
                onClick={() => workspace.setActiveThread(worker.name)}
                class="relative flex items-center gap-1.5 px-2 py-1.5 rounded-none text-xs font-medium transition-all whitespace-nowrap"
                classList={{
                  'bg-pasture-700/60 ring-1 ring-amber-500/30': isActive(),
                  'hover:bg-pasture-700': !isActive(),
                }}
                title={worker.name}
              >
                <WorkerAvatar name={worker.name} size={18} />
                <Show when={unread() > 0}>
                  <span
                    class="absolute -top-1 -right-1 min-w-[14px] h-3.5 px-1 rounded-none text-[8px] font-bold flex items-center justify-center"
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

      {/* Thread info bar */}
      <div class="px-3 py-1.5 border-b border-pasture-700/30 bg-pasture-800/30">
        <Show when={activeThread() === 'chat'}>
          <div class="flex items-center gap-1.5">
            <Icon name="users" class="w-3.5 h-3.5 text-wool-500" />
            <span class="text-[11px] font-medium text-wool-300">Chat</span>
            <span class="text-[10px] text-wool-600">· Group chat</span>
          </div>
        </Show>
        <Show when={activeThread() !== 'chat'}>
          {(() => {
            const worker = () => getWorkerForThread(activeThread());
            return (
              <div class="flex items-center gap-1.5">
                <Show when={worker()}>
                  <WorkerAvatar name={worker()!.name} size={14} />
                </Show>
                <span class="text-[11px] font-medium text-wool-300">{activeThread()}</span>
                <span class="text-[10px] text-wool-600">· Direct message</span>
              </div>
            );
          })()}
        </Show>
      </div>

      {/* Messages area */}
      <div class="flex-1 overflow-y-auto px-3 py-2 space-y-3">
        <Show when={messages.loading && !messages()}>
          <div class="flex items-center justify-center py-8">
            <Icon name="loader-2" class="w-5 h-5 text-wool-500 animate-spin" />
          </div>
        </Show>

        <Show when={!messages.loading && messages()?.length === 0}>
          <div class="flex flex-col items-center justify-center py-12 text-center">
            <div
              class="w-12 h-12 rounded-none flex items-center justify-center mb-3"
              style={{ background: amber(0.08) }}
            >
              <Icon name="message-circle" class="w-6 h-6 text-wool-600" />
            </div>
            <p class="text-sm text-wool-400">No messages yet</p>
            <p class="text-xs text-wool-600 mt-1">
              {activeThread() === 'chat'
                ? 'Start a conversation with your team'
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
                classList={{ 'flex-row-reverse': isUser() }}
              >
                {/* Avatar */}
                <div class="flex-shrink-0 pt-0.5">
                  <Show
                    when={!isUser() && worker()}
                    fallback={
                      <div
                        class="w-7 h-7 rounded-none flex items-center justify-center"
                        style={{
                          background: isUser()
                            ? 'linear-gradient(135deg, var(--amber-500), var(--amber-600))'
                            : 'var(--pasture-700)',
                        }}
                      >
                        <Icon
                          name={isUser() ? 'user' : 'bot'}
                          class={`w-3.5 h-3.5 ${isUser() ? 'text-pasture-900' : 'text-wool-400'}`}
                        />
                      </div>
                    }
                  >
                    <WorkerAvatar name={worker()!.name} size={28} />
                  </Show>
                </div>

                {/* Message bubble */}
                <div
                  class="max-w-[75%] rounded-none px-3 py-2"
                  style={{
                    background: isUser()
                      ? `linear-gradient(135deg, ${amber(0.18)}, ${amber(0.12)})`
                      : 'rgba(45, 45, 45, 0.8)',
                    border: isUser()
                      ? `1px solid ${amber(0.25)}`
                      : '1px solid rgba(64, 64, 64, 0.5)',
                  }}
                >
                  {/* Sender name (for non-user in group chat) */}
                  <Show when={!isUser() && activeThread() === 'chat'}>
                    <p class="text-[10px] font-medium text-wool-500 mb-1">{msg.sender}</p>
                  </Show>
                  <p class="text-[13px] text-wool-200 whitespace-pre-wrap break-words leading-relaxed">
                    {msg.content}
                  </p>
                  <p class="text-[10px] text-wool-600 mt-1.5 text-right">
                    {formatTimestamp(msg.timestamp)}
                  </p>
                </div>
              </div>
            );
          }}
        </For>

        <div ref={messagesEndRef} />
      </div>

      {/* Input area */}
      <div class="p-3 border-t border-pasture-700/50">
        <div
          class="flex items-end gap-2 rounded-none p-2"
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
              activeThread() === 'chat'
                ? 'Message the team...'
                : `Message ${activeThread()}...`
            }
            class="flex-1 bg-transparent text-[13px] text-wool-200 placeholder-wool-600 resize-none focus:outline-none"
            style={{ 'min-height': '36px', 'max-height': '100px' }}
            rows={1}
          />
          <button
            type="button"
            onClick={handleSend}
            disabled={!messageInput().trim() || sending()}
            class="p-2 rounded-none transition-all disabled:opacity-40"
            style={{
              background: messageInput().trim()
                ? 'linear-gradient(135deg, var(--amber-500), var(--amber-600))'
                : 'transparent',
              color: messageInput().trim() ? 'var(--pasture-900)' : 'var(--wool-500)',
            }}
          >
            <Icon
              name={sending() ? 'loader-2' : 'send'}
              class={`w-4 h-4 ${sending() ? 'animate-spin' : ''}`}
            />
          </button>
        </div>
        <p class="text-[10px] text-wool-700 mt-1.5 text-center">
          Press Enter to send · Shift+Enter for new line
        </p>
      </div>
    </div>
  );
};

export default MessagingPanel;
