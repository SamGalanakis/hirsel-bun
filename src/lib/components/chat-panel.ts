/**
 * Chat panel Alpine component
 */

import type { ThreadSummary, Message } from '../types';

/**
 * Chat panel component
 */
export function chatPanel() {
  return {
    threads: [] as ThreadSummary[],
    selectedThread: 'user',
    messages: [] as Message[],
    newMessage: '',
    loading: false,
    error: null as string | null,
    _pollInterval: null as ReturnType<typeof setInterval> | null,
    _threadPollInterval: null as ReturnType<typeof setInterval> | null,

    async init() {
      window.addEventListener('run-selected', (e: Event) => {
        const customEvent = e as CustomEvent<string | null>;
        this.onRunSelected(customEvent.detail);
      });

      const app = this.getAppState();
      if (app && app.selectedRun) {
        await this.fetchThreads();
      }
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
      const app = this.getAppState();
      return app ? app.selectedRun || null : null;
    },

    async onRunSelected(runName: string | null) {
      this.stopPolling();

      if (!runName) {
        this.threads = [];
        this.messages = [];
        return;
      }

      await this.fetchThreads();
      this._threadPollInterval = setInterval(() => this.fetchThreads(), 4000);
    },

    async fetchThreads() {
      const runName = this.getSelectedRun();
      if (!runName) return;

      try {
        if (window.tauriInvoke) {
          this.threads = await window.tauriInvoke<ThreadSummary[]>('get_threads', { runName });
        } else {
          this.threads = [
            {
              name: 'user',
              messageCount: 3,
              unreadCount: 1,
              lastMessage: 'Hello!',
              lastTimestamp: new Date().toISOString(),
            },
            {
              name: 'group',
              messageCount: 5,
              unreadCount: 0,
              lastMessage: 'Task complete',
              lastTimestamp: new Date().toISOString(),
            },
          ];
        }

        const totalUnread = this.threads.reduce((sum, t) => sum + t.unreadCount, 0);
        const app = this.getAppState();
        if (app) {
          app.unreadCount = totalUnread;
        }

        if (this.threads.length > 0 && !this.threads.find(t => t.name === this.selectedThread)) {
          this.selectThread(this.threads[0].name);
        }
      } catch (e) {
        const error = e as Error;
        window.toast.error(error.message || String(error), 'Failed to load threads');
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
        window.toast.error(error.message || String(error), 'Failed to load messages');
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
        window.toast.error(error.message || String(error), 'Failed to send message');
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
