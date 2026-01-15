/**
 * Chat panel Alpine component
 */

import type { ThreadSummary, Message, WorkerDisplay } from '../types';

// Known group chat thread names
const GROUP_CHAT_NAMES = ['learning', 'group'];

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

    async init() {
      window.addEventListener('run-selected', (e: Event) => {
        const customEvent = e as CustomEvent<string | null>;
        this.onRunSelected(customEvent.detail);
      });

      // Initial load if a run is already selected
      const app = this.getAppState();
      if (app && app.selectedRun) {
        await this.onRunSelected(app.selectedRun);
      }
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
      while (el && el.parentElement) {
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
        return;
      }

      // If already polling for this run, just refresh
      if (this._currentRunName === runName && this._threadPollInterval) {
        await Promise.all([this.fetchThreads(), this.fetchWorkers()]);
        return;
      }

      // Stop existing polling and start fresh for new run
      this.stopPolling();
      this._currentRunName = runName;

      await Promise.all([this.fetchThreads(), this.fetchWorkers()]);
      this._threadPollInterval = setInterval(
        () => Promise.all([this.fetchThreads(), this.fetchWorkers()]),
        4000
      );
    },

    async fetchWorkers() {
      const runName = this.getSelectedRun();
      if (!runName) return;

      try {
        if (window.tauriInvoke) {
          this.workers = await window.tauriInvoke<WorkerDisplay[]>('get_workers', { runName });
        } else {
          // Mock data for development
          this.workers = [
            { name: 'worker-1', status: 'running' } as WorkerDisplay,
            { name: 'worker-2', status: 'idle' } as WorkerDisplay,
          ];
        }
      } catch (e) {
        console.debug('Failed to fetch workers:', e);
      }
    },

    async fetchThreads() {
      const runName = this.getSelectedRun();
      if (!runName) return;

      try {
        if (window.tauriInvoke) {
          this.threads = await window.tauriInvoke<ThreadSummary[]>('get_threads', { runName });
        } else {
          // Mock data for development
          this.threads = [
            {
              name: 'learning',
              messageCount: 3,
              unreadCount: 1,
              lastMessage: 'Learned something new!',
              lastTimestamp: new Date().toISOString(),
            },
            {
              name: 'group',
              messageCount: 5,
              unreadCount: 0,
              lastMessage: 'Task complete',
              lastTimestamp: new Date().toISOString(),
            },
            {
              name: 'worker-1',
              messageCount: 2,
              unreadCount: 1,
              lastMessage: 'Working on it',
              lastTimestamp: new Date().toISOString(),
            },
          ];
        }

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
        const error = e as Error;
        window.toast.error('Failed to load threads');
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

        const thread = this.threads.find(t => t.name === name);
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
          await window.tauriInvoke('send_message', {
            runName,
            threadName: this.selectedThread,
            content,
          });
        }

        await this.fetchMessages();
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

    formatTime(timestamp: string | null | undefined): string {
      if (!timestamp) return '';
      const d = new Date(timestamp);
      return d.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
    },

    formatDate(timestamp: string | null | undefined): string {
      if (!timestamp) return '';
      const d = new Date(timestamp);
      const today = new Date();
      if (d.toDateString() === today.toDateString()) return 'Today';
      const yesterday = new Date(today);
      yesterday.setDate(yesterday.getDate() - 1);
      if (d.toDateString() === yesterday.toDateString()) return 'Yesterday';
      return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
    },

    getSenderColor(sender: string): string {
      const colors: Record<string, string> = {
        user: 'text-amber-500',
        admin: 'text-amber-500',
        system: 'text-wool-500',
      };
      if (colors[sender]) return colors[sender];
      const workerColors = ['text-sage', 'text-sky-400', 'text-pink-400', 'text-purple-400', 'text-golden'];
      let hash = 0;
      for (let i = 0; i < sender.length; i++) {
        hash = sender.charCodeAt(i) + ((hash << 5) - hash);
      }
      return workerColors[Math.abs(hash) % workerColors.length];
    },
  };
}
