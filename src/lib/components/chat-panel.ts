/**
 * Chat panel Alpine component
 */

import { getIcon } from '../icons';
import { generateSheepSvg } from '../sheep-avatar';
import type { Message, SheepConfig, ThreadSummary, WorkerDisplay } from '../types';
import { formatDate, formatTimeHHMM } from '../utils/formatters';

// Known group chat thread names (user is the channel for workers to message the human)
const GROUP_CHAT_NAMES = ['user', 'group', 'learnings'];

// Memoization cache for sender color hashing
const senderColorCache = new Map<string, string>();

// Pre-defined colors for known senders
const SENDER_COLORS: Record<string, string> = {
  user: 'text-amber-500',
  admin: 'text-amber-500',
  system: 'text-wool-500',
};

// Colors for workers (assigned by hash)
const WORKER_COLORS = [
  'text-sage',
  'text-sky-400',
  'text-pink-400',
  'text-purple-400',
  'text-golden',
];

// Get color for a sender (memoized)
function getSenderColorCached(sender: string): string {
  // Check cache first
  const cached = senderColorCache.get(sender);
  if (cached) return cached;

  // Check known senders
  if (SENDER_COLORS[sender]) {
    senderColorCache.set(sender, SENDER_COLORS[sender]);
    return SENDER_COLORS[sender];
  }

  // Compute hash for worker colors
  let hash = 0;
  for (let i = 0; i < sender.length; i++) {
    hash = sender.charCodeAt(i) + ((hash << 5) - hash);
  }
  const color = WORKER_COLORS[Math.abs(hash) % WORKER_COLORS.length];
  senderColorCache.set(sender, color);
  return color;
}

/**
 * Chat panel component
 */
export function chatPanel() {
  return {
    threads: [] as ThreadSummary[],
    workers: [] as WorkerDisplay[],
    selectedThread: null as string | null,
    messages: [] as Message[],
    newMessage: '',
    loading: false,
    error: null as string | null,
    _pollInterval: null as ReturnType<typeof setInterval> | null,
    _threadPollInterval: null as ReturnType<typeof setInterval> | null,
    _currentRunName: null as string | null,
    _isLoadingRun: false,
    _eventCleanups: [] as (() => void)[],

    async init() {
      const runSelectedHandler = (e: Event) => {
        const customEvent = e as CustomEvent<string | null>;
        this.onRunSelected(customEvent.detail);
      };
      window.addEventListener('run-selected', runSelectedHandler);
      this._eventCleanups.push(() =>
        window.removeEventListener('run-selected', runSelectedHandler),
      );

      // Listen for deep-link thread selection from notifications
      const selectThreadHandler = (e: Event) => {
        const customEvent = e as CustomEvent<{ runName: string; thread: string }>;
        const { runName, thread } = customEvent.detail;
        // Only select if we're viewing the right run
        if (this._currentRunName === runName) {
          this.selectThread(thread);
        }
      };
      window.addEventListener('select-chat-thread', selectThreadHandler);
      this._eventCleanups.push(() =>
        window.removeEventListener('select-chat-thread', selectThreadHandler),
      );

      // Initial load if a run is already selected
      const app = this.getAppState();
      if (app?.selectedRun) {
        await this.onRunSelected(app.selectedRun);
      }
    },

    destroy() {
      this.stopPolling();
      this._eventCleanups.forEach((fn) => fn());
      this._eventCleanups = [];
    },

    // Get group chat threads (learning, group)
    get groupChats(): ThreadSummary[] {
      return this.threads.filter((t) => GROUP_CHAT_NAMES.includes(t.name));
    },

    // Get DM threads (worker names) - includes workers without threads yet
    get dmThreads(): Array<{ name: string; thread: ThreadSummary | null }> {
      const dms: Array<{ name: string; thread: ThreadSummary | null }> = [];

      // Add workers that have threads
      const workerThreads = this.threads.filter((t) => !GROUP_CHAT_NAMES.includes(t.name));
      for (const thread of workerThreads) {
        dms.push({ name: thread.name, thread });
      }

      // Add workers that don't have threads yet
      for (const worker of this.workers) {
        if (!dms.find((d) => d.name === worker.name)) {
          dms.push({ name: worker.name, thread: null });
        }
      }

      return dms.sort((a, b) => a.name.localeCompare(b.name));
    },

    getAppState(): { selectedRun?: string | null; unreadCount?: number } | null {
      // @ts-expect-error Alpine.js $el magic property
      let el = this.$el as HTMLElement;
      while (el?.parentElement) {
        el = el.parentElement;
        // @ts-expect-error Alpine.js internal property
        if (el._x_dataStack) {
          // @ts-expect-error Alpine.js internal property
          return el._x_dataStack[0];
        }
      }
      return null;
    },

    getSelectedRun(): string | null {
      // Use tracked run name first, fall back to app state
      if (this._currentRunName) {
        return this._currentRunName;
      }
      const app = this.getAppState();
      return app ? app.selectedRun || null : null;
    },

    async onRunSelected(runName: string | null) {
      if (!runName) {
        this.stopPolling();
        this.threads = [];
        this.workers = [];
        this.messages = [];
        this.selectedThread = null;
        this._currentRunName = null;
        this._isLoadingRun = false;
        return;
      }

      // If already polling for this run, just refresh
      if (this._currentRunName === runName && this._threadPollInterval) {
        await Promise.all([this.fetchThreads(), this.fetchWorkers()]);
        return;
      }

      // Prevent concurrent loading
      if (this._isLoadingRun) {
        return;
      }
      this._isLoadingRun = true;

      // Stop existing polling and start fresh for new run
      this.stopPolling();
      this._currentRunName = runName;

      try {
        await Promise.all([this.fetchThreads(), this.fetchWorkers()]);
        this._threadPollInterval = setInterval(
          () => Promise.all([this.fetchThreads(), this.fetchWorkers()]),
          4000,
        );
      } finally {
        this._isLoadingRun = false;
      }
    },

    async fetchWorkers() {
      const runName = this.getSelectedRun();
      if (!runName || !window.tauriInvoke) return;

      try {
        const workers = await window.tauriInvoke<WorkerDisplay[]>('get_workers', { runName });
        this.workers = (workers || []).filter((w): w is WorkerDisplay => w != null);
      } catch (e) {
        console.error('Failed to fetch workers:', e);
      }
    },

    async fetchThreads() {
      const runName = this.getSelectedRun();
      if (!runName || !window.tauriInvoke) return;

      try {
        this.threads = await window.tauriInvoke<ThreadSummary[]>('get_threads', { runName });

        const totalUnread = this.threads.reduce((sum, t) => sum + t.unreadCount, 0);
        const app = this.getAppState();
        if (app) {
          app.unreadCount = totalUnread;
        }

        // Auto-select first thread if current selection is invalid
        if (this.selectedThread) {
          const stillValid =
            this.threads.find((t) => t.name === this.selectedThread) ||
            this.workers.find((w) => w.name === this.selectedThread);
          if (!stillValid) {
            this.selectedThread = null;
          }
        }
      } catch (e) {
        console.error('Failed to load threads:', e);
        window.toast?.error('Failed to load threads');
        this.error = 'Failed to load threads';
      }
    },

    async selectThread(name: string) {
      this.selectedThread = name;
      this.loading = true;
      this.error = null;

      if (this._pollInterval) {
        clearInterval(this._pollInterval);
        this._pollInterval = null;
      }

      const runName = this.getSelectedRun();
      if (!runName) {
        this.loading = false;
        return;
      }

      try {
        await this.fetchMessages();

        if (window.tauriInvoke) {
          await window.tauriInvoke('mark_messages_read', {
            runName,
            threadName: name,
            reader: 'user',
          });
        }

        const thread = this.threads.find((t) => t.name === name);
        if (thread) {
          thread.unreadCount = 0;
        }

        this._pollInterval = setInterval(() => this.fetchMessages(), 2000);

        // @ts-expect-error Alpine.js $nextTick magic method
        this.$nextTick(() => this.scrollToBottom());
      } catch (e) {
        const error = e as Error;
        window.toast.error('Failed to load messages');
        this.error = 'Failed to load messages';
      } finally {
        this.loading = false;
      }
    },

    async fetchMessages() {
      const runName = this.getSelectedRun();
      if (!runName || !this.selectedThread) return;

      try {
        const prevLength = this.messages.length;

        if (window.tauriInvoke) {
          this.messages = await window.tauriInvoke<Message[]>('get_messages', {
            runName,
            threadName: this.selectedThread,
          });
        }

        if (this.messages.length > prevLength) {
          // @ts-expect-error Alpine.js $nextTick magic method
          this.$nextTick(() => this.scrollToBottom());
        }
      } catch (e) {
        console.debug('Polling fetch failed:', e);
      }
    },

    async sendMessage() {
      const content = this.newMessage.trim();
      if (!content) return;

      const runName = this.getSelectedRun();
      if (!runName) {
        this.error = 'No run selected';
        return;
      }

      this.newMessage = '';

      try {
        if (window.tauriInvoke) {
          // Send message and get the created message back (avoids full refetch)
          const newMsg = await window.tauriInvoke<Message>('send_message', {
            runName,
            threadName: this.selectedThread,
            content,
          });

          // Append to local messages instead of refetching all
          this.messages.push(newMsg);
        }

        // @ts-expect-error Alpine.js $nextTick magic method
        this.$nextTick(() => this.scrollToBottom());
      } catch (e) {
        const error = e as Error;
        window.toast.error('Failed to send message');
        this.error = 'Failed to send message';
        this.newMessage = content;
      }
    },

    scrollToBottom() {
      // @ts-expect-error Alpine.js $el magic property
      const container = (this.$el as HTMLElement).querySelector('.messages-container');
      if (container) {
        container.scrollTop = container.scrollHeight;
      }
    },

    stopPolling() {
      if (this._pollInterval) {
        clearInterval(this._pollInterval);
        this._pollInterval = null;
      }
      if (this._threadPollInterval) {
        clearInterval(this._threadPollInterval);
        this._threadPollInterval = null;
      }
    },

    // Use shared formatters
    formatTime: formatTimeHHMM,
    formatDate,

    getSenderColor: getSenderColorCached,

    // Get sheep avatar SVG for a worker
    getWorkerAvatar(workerName: string): string {
      const worker = this.workers.find((w) => w.name === workerName);
      if (worker?.sheepConfig) {
        return generateSheepSvg(worker.sheepConfig, 16);
      }
      // Fallback - grey circle
      return '<svg class="w-4 h-4"><circle cx="8" cy="8" r="6" fill="#6b7280"/></svg>';
    },

    // Get icon SVG for group chat threads
    getGroupChatIcon(threadName: string): string {
      const iconNames: Record<string, string> = {
        user: 'user',
        group: 'users',
        learnings: 'library',
      };
      const iconName = iconNames[threadName] || 'hash';
      return getIcon(iconName, 12);
    },
  };
}
