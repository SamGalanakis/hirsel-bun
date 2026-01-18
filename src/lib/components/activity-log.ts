/**
 * Activity log Alpine component
 */

import type { HistoryEntry, WorkerDisplay } from '../types';
import { getActionIcon as getActionIconSvg, getIcon } from '../icons';
import { dataCache, DATA_EVENTS } from '../data-cache';

export type SortDirection = 'asc' | 'desc';

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
// Actions that are worker-specific (check detail for worker name)
// These must match the action names used in backend log_history calls
// Detail formats:
//   worker_add: "{worker_name}"
//   worker_status: "{worker_name} → {status}"
//   task_claim/done/unclaim: "{task_id} by {worker_name}"
const WORKER_ACTIONS = new Set([
  'worker_status',
  'worker_add',
  'task_claim',
  'task_done',
  'task_unclaim',
]);

export function activityLog() {
  return {
    runName: null as string | null,
    entries: [] as HistoryEntry[],
    workers: [] as WorkerDisplay[],
    loading: false,
    error: null as string | null,
    autoScroll: true,
    isFullscreen: false,
    sortDirection: 'desc' as SortDirection,
    _eventCleanups: [] as (() => void)[],
    _cacheUnsubscribe: null as (() => void) | null,

    // Computed: sorted entries based on direction
    get sortedEntries(): HistoryEntry[] {
      if (this.sortDirection === 'asc') {
        return [...this.entries].reverse();
      }
      return this.entries;
    },

    get sortLabel(): string {
      return this.sortDirection === 'desc' ? 'Latest' : 'Oldest';
    },

    get sortIcon(): string {
      return this.sortDirection === 'desc' ? '↓' : '↑';
    },

    toggleSort() {
      this.sortDirection = this.sortDirection === 'desc' ? 'asc' : 'desc';
      // When switching to "latest first", enable auto-scroll
      // When switching to "oldest first", disable it
      this.autoScroll = this.sortDirection === 'desc';
    },

    toggleFullscreen() {
      this.isFullscreen = !this.isFullscreen;
      window.dispatchEvent(new CustomEvent('activity-fullscreen', { detail: this.isFullscreen }));
    },

    async init() {
      // Subscribe to shared cache
      this._cacheUnsubscribe = dataCache.subscribe();

      // Listen for history updates from cache
      const historyUpdatedHandler = (e: Event) => {
        const customEvent = e as CustomEvent<HistoryEntry[]>;
        const prevLength = this.entries.length;
        this.entries = customEvent.detail;
        this.loading = false;
        if (this.entries.length > prevLength && this.autoScroll) {
          // @ts-expect-error Alpine.js $nextTick magic method
          this.$nextTick(() => this.scrollToBottom());
        }
      };
      window.addEventListener(DATA_EVENTS.HISTORY_UPDATED, historyUpdatedHandler);
      this._eventCleanups.push(() => window.removeEventListener(DATA_EVENTS.HISTORY_UPDATED, historyUpdatedHandler));

      // Listen for workers updates from cache
      const workersUpdatedHandler = (e: Event) => {
        const customEvent = e as CustomEvent<WorkerDisplay[]>;
        this.workers = customEvent.detail;
      };
      window.addEventListener(DATA_EVENTS.WORKERS_UPDATED, workersUpdatedHandler);
      this._eventCleanups.push(() => window.removeEventListener(DATA_EVENTS.WORKERS_UPDATED, workersUpdatedHandler));

      // Listen for run selection changes
      const runSelectedHandler = (e: Event) => {
        const customEvent = e as CustomEvent<string | null>;
        if (customEvent.detail) {
          this.runName = customEvent.detail;
          this.loading = true;
          this.workers = dataCache.getWorkers();
          // Get initial history from cache
          const cachedHistory = dataCache.getHistory();
          if (cachedHistory.length > 0) {
            this.entries = cachedHistory;
            this.loading = false;
            if (this.autoScroll) {
              // @ts-expect-error Alpine.js $nextTick magic method
              this.$nextTick(() => this.scrollToBottom());
            }
          }
        } else {
          this.clearHistory();
        }
      };
      window.addEventListener('run-selected', runSelectedHandler);
      this._eventCleanups.push(() => window.removeEventListener('run-selected', runSelectedHandler));

      const keydownHandler = (e: KeyboardEvent) => {
        if (e.key === 'Escape' && this.isFullscreen) {
          this.isFullscreen = false;
          window.dispatchEvent(new CustomEvent('activity-fullscreen', { detail: false }));
        }
      };
      document.addEventListener('keydown', keydownHandler);
      this._eventCleanups.push(() => document.removeEventListener('keydown', keydownHandler));

      const toggleFullscreenHandler = () => {
        this.toggleFullscreen();
      };
      window.addEventListener('toggle-activity-fullscreen', toggleFullscreenHandler);
      this._eventCleanups.push(() => window.removeEventListener('toggle-activity-fullscreen', toggleFullscreenHandler));

      // Get initial data from cache if a run is already selected
      const selectedRun = dataCache.getSelectedRun();
      if (selectedRun) {
        this.runName = selectedRun;
        this.workers = dataCache.getWorkers();
        const cachedHistory = dataCache.getHistory();
        if (cachedHistory.length > 0) {
          this.entries = cachedHistory;
          if (this.autoScroll) {
            // @ts-expect-error Alpine.js $nextTick magic method
            this.$nextTick(() => this.scrollToBottom());
          }
        }
      }
    },

    destroy() {
      this._eventCleanups.forEach(fn => fn());
      this._eventCleanups = [];
      if (this._cacheUnsubscribe) {
        this._cacheUnsubscribe();
        this._cacheUnsubscribe = null;
      }
    },

    clearHistory() {
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

    // Extract worker name from entry if this is a worker-related action
    getWorkerName(entry: HistoryEntry): string | null {
      const normalized = entry.action.toLowerCase().replace(/[\s-]+/g, '_');
      if (!WORKER_ACTIONS.has(normalized)) {
        return null;
      }

      if (!entry.detail) {
        return null;
      }

      // worker_add: detail is just the worker name
      if (normalized === 'worker_add') {
        return entry.detail.trim();
      }

      // worker_status: format is "{worker_name} → {status}"
      if (normalized === 'worker_status') {
        const arrowIdx = entry.detail.indexOf(' → ');
        if (arrowIdx > 0) {
          return entry.detail.substring(0, arrowIdx).trim();
        }
        // Fallback: first word
        const parts = entry.detail.split(/\s+/);
        return parts[0] || null;
      }

      // task_claim/done/unclaim: format is "{task_id} by {worker_name}"
      const byMatch = entry.detail.match(/\s+by\s+(.+?)(?:\s*\(|$)/i);
      if (byMatch) {
        return byMatch[1].trim();
      }

      return null;
    },

    // Get icon for entry based on action type
    getEntryAvatar(entry: HistoryEntry): string {
      const normalized = entry.action.toLowerCase().replace(/[\s-]+/g, '_');

      // Worker-related actions get a user icon
      if (WORKER_ACTIONS.has(normalized)) {
        return getIcon('user', 14);
      }

      // System/settings icon for system events
      return getIcon('settings', 14);
    },

    // Check if entry is worker-related (for styling)
    isWorkerEntry(entry: HistoryEntry): boolean {
      return this.getWorkerName(entry) !== null;
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
