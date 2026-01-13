/**
 * Activity Log Alpine.js Component
 *
 * Displays a scrolling activity log with timestamps and real-time updates.
 * Shows history entries for the selected run including:
 * - Task claims and completions
 * - Worker status changes
 * - Messages sent
 * - Eval results
 * - System events
 */

import { getHistory } from '../lib/api';
import type { HistoryEntry } from '../lib/types';
import { getActionIcon as getActionIconSvg } from '../lib/icons';

/**
 * Activity log component data
 */
export interface ActivityLogData {
  runName: string | null;
  entries: HistoryEntry[];
  loading: boolean;
  error: string | null;
  pollInterval: ReturnType<typeof setInterval> | null;
  autoScroll: boolean;
}

/**
 * Action color mapping for visual distinction
 */
const ACTION_COLORS: Record<string, string> = {
  // Task actions
  task_claimed: 'text-amber-400',
  task_done: 'text-sage',
  task_added: 'text-sky-400',
  task_deleted: 'text-terra',
  task_unclaimed: 'text-wool-400',
  task_reopened: 'text-golden',

  // Worker actions
  worker_started: 'text-sage',
  worker_stopped: 'text-wool-500',
  worker_error: 'text-terra',
  worker_paused: 'text-golden',
  worker_resumed: 'text-amber-400',

  // Eval actions
  eval_started: 'text-amber-400',
  eval_passed: 'text-sage',
  eval_failed: 'text-terra',

  // Run actions
  run_started: 'text-sage',
  run_paused: 'text-golden',
  run_resumed: 'text-amber-400',
  run_done: 'text-sage',
  run_delivered: 'text-sage',
  run_timed_out: 'text-terra',

  // Message actions
  message_sent: 'text-sky-400',
  message_received: 'text-wool-300',

  // Default
  default: 'text-wool-300',
};

/**
 * Format timestamp for display
 */
function formatTime(timestamp: string | null): string {
  if (!timestamp) return '';
  const d = new Date(timestamp);
  return d.toLocaleTimeString('en-US', {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

/**
 * Format timestamp with date for older entries
 */
function formatFullTime(timestamp: string | null): string {
  if (!timestamp) return '';
  const d = new Date(timestamp);
  const now = new Date();
  const isToday = d.toDateString() === now.toDateString();

  if (isToday) {
    return formatTime(timestamp);
  }

  return d.toLocaleDateString('en-US', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

/**
 * Get color class for action type
 */
function getActionColor(action: string): string {
  // Normalize action to snake_case for lookup
  const normalized = action.toLowerCase().replace(/[\s-]+/g, '_');
  return ACTION_COLORS[normalized] || ACTION_COLORS.default;
}

/**
 * Get icon SVG for action type
 */
function getActionIcon(action: string): string {
  return getActionIconSvg(action, 14);
}

/**
 * Alpine.js component factory for activity log
 */
export function activityLog(): ActivityLogData & {
  init(): void;
  destroy(): void;
  loadHistory(name: string): Promise<void>;
  clearHistory(): void;
  formatTime(timestamp: string | null): string;
  formatFullTime(timestamp: string | null): string;
  getActionColor(action: string): string;
  getActionIcon(action: string): string;
  scrollToBottom(): void;
  handleScroll(event: Event): void;
} {
  return {
    runName: null,
    entries: [],
    loading: false,
    error: null,
    pollInterval: null,
    autoScroll: true,

    /**
     * Initialize the component
     */
    init(): void {
      // Listen for run selection events
      window.addEventListener('run-selected', ((e: CustomEvent<string | null>) => {
        if (e.detail) {
          this.loadHistory(e.detail);
        } else {
          this.clearHistory();
        }
      }) as EventListener);
    },

    /**
     * Clean up when component is destroyed
     */
    destroy(): void {
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }
    },

    /**
     * Load history for a run
     */
    async loadHistory(name: string): Promise<void> {
      // Stop existing polling
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }

      this.runName = name;
      this.loading = true;
      this.error = null;

      try {
        this.entries = await getHistory(name, 100);
        this.loading = false;

        // Auto-scroll to bottom on initial load
        if (this.autoScroll) {
          requestAnimationFrame(() => this.scrollToBottom());
        }

        // Start polling for updates every 2 seconds
        this.pollInterval = setInterval(async () => {
          if (!this.runName) return;
          try {
            const prevLength = this.entries.length;
            this.entries = await getHistory(this.runName, 100);

            // Auto-scroll if new entries and autoScroll is enabled
            if (this.entries.length > prevLength && this.autoScroll) {
              requestAnimationFrame(() => this.scrollToBottom());
            }
          } catch (err) {
            console.error('Failed to poll history:', err);
          }
        }, 2000);
      } catch (err) {
        this.error = err instanceof Error ? err.message : String(err);
        this.loading = false;
      }
    },

    /**
     * Clear history when no run is selected
     */
    clearHistory(): void {
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }
      this.runName = null;
      this.entries = [];
      this.loading = false;
      this.error = null;
    },

    /**
     * Format timestamp for display
     */
    formatTime,

    /**
     * Format timestamp with date for older entries
     */
    formatFullTime,

    /**
     * Get color class for action type
     */
    getActionColor,

    /**
     * Get icon for action type
     */
    getActionIcon,

    /**
     * Scroll to bottom of log
     */
    scrollToBottom(): void {
      // Find the scrollable container
      const container = document.querySelector('.activity-panel .overflow-y-auto');
      if (container) {
        container.scrollTop = container.scrollHeight;
      }
    },

    /**
     * Handle scroll to disable auto-scroll when user scrolls up
     */
    handleScroll(event: Event): void {
      const target = event.target as HTMLElement;
      if (!target) return;

      // Check if scrolled to bottom (with some tolerance)
      const isAtBottom = target.scrollHeight - target.scrollTop - target.clientHeight < 50;
      this.autoScroll = isAtBottom;
    },
  };
}

/**
 * Register the component with Alpine.js
 */
export function registerActivityLogComponent(): void {
  if (typeof window !== 'undefined') {
    (window as unknown as Record<string, unknown>).activityLog = activityLog;
  }
}

// Auto-register if Alpine is already loaded
if (typeof window !== 'undefined' && typeof Alpine !== 'undefined') {
  registerActivityLogComponent();
}

// Declare Alpine global for TypeScript
declare const Alpine: {
  store: (name: string) => Record<string, unknown> | undefined;
};
