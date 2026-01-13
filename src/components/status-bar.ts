/**
 * Status Bar Alpine.js Component
 *
 * Displays run status, worker count, task progress, and time information
 * at the bottom of the application window.
 */

import type { RunSummary, RunStatus } from '../lib/types';

// =============================================================================
// Types
// =============================================================================

export interface StatusBarState {
  run: RunSummary | null;
  tasksDone: number;
  tasksTotal: number;
}

export interface StatusBarComponent extends StatusBarState {
  getStatusDotClass: (status: RunStatus | undefined) => Record<string, boolean>;
  getStatusText: () => string;
  formatElapsed: (minutes: number | null | undefined) => string;
  formatTimeRemaining: (
    limit: number | null | undefined,
    elapsed: number | null | undefined
  ) => string;
  getProgressPercent: () => number;
  isTimeWarning: () => boolean;
}

// =============================================================================
// Status Dot Classes
// =============================================================================

/**
 * Returns CSS classes for the status indicator dot
 */
export function getStatusDotClass(
  status: RunStatus | undefined
): Record<string, boolean> {
  return {
    'status-working': status === 'working' || status === 'eval',
    'status-waiting':
      status === 'waiting' || status === 'paused' || status === 'runaway',
    'status-done':
      status === 'done' || status === 'delivered' || status === 'merged',
    'status-error':
      status === 'timed_out' || status === 'eval_failed',
    'status-idle': !status || status === 'idle',
  };
}

// =============================================================================
// Time Formatting
// =============================================================================

/**
 * Format elapsed time in minutes to human-readable string
 */
export function formatElapsed(minutes: number | null | undefined): string {
  if (!minutes || minutes <= 0) return '0m';
  if (minutes < 60) return `${minutes}m`;
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return m > 0 ? `${h}h ${m}m` : `${h}h`;
}

/**
 * Format time remaining based on limit and elapsed
 */
export function formatTimeRemaining(
  limit: number | null | undefined,
  elapsed: number | null | undefined
): string {
  if (!limit) return '';
  const remaining = limit - (elapsed || 0);
  if (remaining <= 0) return 'Time up!';
  if (remaining < 60) return `${remaining}m remaining`;
  const h = Math.floor(remaining / 60);
  const m = remaining % 60;
  return m > 0 ? `${h}h ${m}m remaining` : `${h}h remaining`;
}

/**
 * Calculate progress percentage for time limit
 */
export function getTimeProgress(
  elapsed: number | null | undefined,
  limit: number | null | undefined
): number {
  if (!limit || !elapsed) return 0;
  return Math.min(100, (elapsed / limit) * 100);
}

/**
 * Check if time is running low (>80% used)
 */
export function isTimeWarning(
  elapsed: number | null | undefined,
  limit: number | null | undefined
): boolean {
  if (!limit) return false;
  return getTimeProgress(elapsed, limit) >= 80;
}

// =============================================================================
// Status Text
// =============================================================================

const STATUS_LABELS: Record<RunStatus, string> = {
  idle: 'Idle',
  working: 'Working',
  paused: 'Paused',
  runaway: 'Runaway',
  timed_out: 'Timed Out',
  eval: 'Running Eval',
  eval_failed: 'Eval Failed',
  waiting: 'Waiting',
  done: 'Done',
  delivered: 'Delivered',
  merged: 'Merged',
};

/**
 * Get human-readable status text
 */
export function getStatusText(status: RunStatus | undefined): string {
  if (!status) return 'No run selected';
  return STATUS_LABELS[status] || status;
}

// =============================================================================
// Alpine.js Component
// =============================================================================

/**
 * Creates the status bar Alpine.js component
 *
 * Usage in HTML:
 * ```html
 * <footer x-data="statusBar()" ...>
 * ```
 */
export function statusBar(): StatusBarComponent {
  return {
    // State (will be bound from parent via Alpine's reactive system)
    run: null,
    tasksDone: 0,
    tasksTotal: 0,

    getStatusDotClass(status: RunStatus | undefined): Record<string, boolean> {
      return getStatusDotClass(status);
    },

    getStatusText(): string {
      return getStatusText(this.run?.status);
    },

    formatElapsed(minutes: number | null | undefined): string {
      return formatElapsed(minutes);
    },

    formatTimeRemaining(
      limit: number | null | undefined,
      elapsed: number | null | undefined
    ): string {
      return formatTimeRemaining(limit, elapsed);
    },

    getProgressPercent(): number {
      return getTimeProgress(
        this.run?.elapsedMinutes,
        this.run?.timeLimitMinutes
      );
    },

    isTimeWarning(): boolean {
      return isTimeWarning(
        this.run?.elapsedMinutes,
        this.run?.timeLimitMinutes
      );
    },
  };
}

// =============================================================================
// Registration
// =============================================================================

/**
 * Register the status bar component on the window object
 *
 * This makes it available for use as x-data="statusBar()" in HTML.
 */
export function registerStatusBar(): void {
  if (typeof window !== 'undefined') {
    (window as unknown as Record<string, unknown>).statusBar = statusBar;
  }
}

/**
 * Auto-register if we're in a browser environment
 */
if (typeof window !== 'undefined') {
  registerStatusBar();
}

export default statusBar;
