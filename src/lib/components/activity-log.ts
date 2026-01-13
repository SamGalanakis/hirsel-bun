/**
 * Activity log Alpine component
 */

import type { HistoryEntry } from '../types';
import { getActionIcon as getActionIconSvg } from '../icons';

const ACTION_COLORS: Record<string, string> = {
  task_claimed: 'text-amber-400',
  task_done: 'text-sage',
  task_added: 'text-sky-400',
  task_deleted: 'text-terra',
  task_unclaimed: 'text-wool-400',
  task_reopened: 'text-golden',
  worker_started: 'text-sage',
  worker_stopped: 'text-wool-500',
  worker_error: 'text-terra',
  worker_paused: 'text-golden',
  worker_resumed: 'text-amber-400',
  eval_started: 'text-amber-400',
  eval_passed: 'text-sage',
  eval_failed: 'text-terra',
  run_started: 'text-sage',
  run_paused: 'text-golden',
  run_resumed: 'text-amber-400',
  run_done: 'text-sage',
  run_delivered: 'text-sage',
  run_timed_out: 'text-terra',
  message_sent: 'text-sky-400',
  message_received: 'text-wool-300',
  default: 'text-wool-300',
};

const ACTION_BG_CLASSES: Record<string, string> = {
  task_claimed: 'bg-amber-500/20',
  task_done: 'bg-sage/20',
  task_add: 'bg-sky-500/20',
  task_added: 'bg-sky-500/20',
  task_deleted: 'bg-terra/20',
  task_unclaimed: 'bg-wool-500/20',
  task_reopened: 'bg-golden/20',
  task_claim: 'bg-amber-500/20',
  worker_started: 'bg-sage/20',
  worker_stopped: 'bg-wool-500/20',
  worker_error: 'bg-terra/20',
  worker_paused: 'bg-golden/20',
  worker_resumed: 'bg-amber-500/20',
  worker_status: 'bg-amber-500/20',
  eval_started: 'bg-amber-500/20',
  eval_passed: 'bg-sage/20',
  eval_failed: 'bg-terra/20',
  eval_complete: 'bg-sage/20',
  run_started: 'bg-sage/20',
  run_paused: 'bg-golden/20',
  run_resumed: 'bg-amber-500/20',
  run_done: 'bg-sage/20',
  run_delivered: 'bg-sage/20',
  run_timed_out: 'bg-terra/20',
  status_change: 'bg-amber-500/20',
  message_sent: 'bg-sky-500/20',
  message_received: 'bg-wool-500/20',
  init: 'bg-sage/20',
  default: 'bg-wool-600/20',
};

/**
 * Activity log component
 */
export function activityLog() {
  return {
    runName: null as string | null,
    entries: [] as HistoryEntry[],
    loading: false,
    error: null as string | null,
    pollInterval: null as ReturnType<typeof setInterval> | null,
    autoScroll: true,
    isFullscreen: false,

    toggleFullscreen() {
      this.isFullscreen = !this.isFullscreen;
      window.dispatchEvent(new CustomEvent('activity-fullscreen', { detail: this.isFullscreen }));
    },

    async init() {
      window.addEventListener('run-selected', async (e: Event) => {
        const customEvent = e as CustomEvent<string | null>;
        if (customEvent.detail) {
          await this.loadHistory(customEvent.detail);
        } else {
          this.clearHistory();
        }
      });

      document.addEventListener('keydown', (e: KeyboardEvent) => {
        if (e.key === 'Escape' && this.isFullscreen) {
          this.isFullscreen = false;
          window.dispatchEvent(new CustomEvent('activity-fullscreen', { detail: false }));
        }
      });

      window.addEventListener('toggle-activity-fullscreen', () => {
        this.toggleFullscreen();
      });
    },

    destroy() {
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }
    },

    async loadHistory(name: string) {
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }

      this.runName = name;
      this.loading = true;
      this.error = null;

      try {
        if (window.tauriInvoke) {
          this.entries = await window.tauriInvoke<HistoryEntry[]>('get_history', {
            runName: name,
            limit: 100,
          });
        } else {
          this.entries = [];
        }
        this.loading = false;

        if (this.autoScroll) {
          // @ts-expect-error Alpine.js $nextTick magic method
          this.$nextTick(() => this.scrollToBottom());
        }

        this.pollInterval = setInterval(async () => {
          if (!this.runName) return;
          try {
            const prevLength = this.entries.length;
            if (window.tauriInvoke) {
              this.entries = await window.tauriInvoke<HistoryEntry[]>('get_history', {
                runName: this.runName,
                limit: 100,
              });
            }
            if (this.entries.length > prevLength && this.autoScroll) {
              // @ts-expect-error Alpine.js $nextTick magic method
              this.$nextTick(() => this.scrollToBottom());
            }
          } catch (err) {
            console.error('Failed to poll history:', err);
          }
        }, 2000);
      } catch (err) {
        const error = err as Error;
        this.error = error.message || String(error);
        this.loading = false;
      }
    },

    clearHistory() {
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }
      this.runName = null;
      this.entries = [];
      this.loading = false;
      this.error = null;
    },

    formatTime(timestamp: string | null | undefined): string {
      if (!timestamp) return '';
      const d = new Date(timestamp);
      const hours = String(d.getHours()).padStart(2, '0');
      const mins = String(d.getMinutes()).padStart(2, '0');
      const secs = String(d.getSeconds()).padStart(2, '0');
      return `${hours}:${mins}:${secs}`;
    },

    getActionColor(action: string): string {
      const normalized = action.toLowerCase().replace(/[\s-]+/g, '_');
      return ACTION_COLORS[normalized] || ACTION_COLORS.default;
    },

    getActionIcon(action: string): string {
      return getActionIconSvg(action, 14);
    },

    getActionBgClass(action: string): string {
      const normalized = action.toLowerCase().replace(/[\s-]+/g, '_');
      return ACTION_BG_CLASSES[normalized] || ACTION_BG_CLASSES.default;
    },

    formatActionLabel(action: string): string {
      return action.replace(/_/g, ' ').replace(/\b\w/g, c => c.toUpperCase());
    },

    scrollToBottom() {
      // @ts-expect-error Alpine.js $el magic property
      const container = this.$el as HTMLElement;
      if (container) {
        container.scrollTop = container.scrollHeight;
      }
    },

    handleScroll(event: Event) {
      const target = event.target as HTMLElement;
      if (!target) return;
      const isAtBottom = target.scrollHeight - target.scrollTop - target.clientHeight < 50;
      this.autoScroll = isAtBottom;
    },
  };
}
