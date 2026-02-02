/**
 * Status-related utilities for the Hirsel GUI
 *
 * Centralized status configuration for tasks, workers, and runs.
 * All components should import from here instead of defining their own mappings.
 */

import type { RunStatus, WorkerStatus } from '../types';

// =============================================================================
// Worker Status Configuration
// =============================================================================

export const WORKER_STATUS_CONFIG = {
  idle: {
    label: 'Idle',
    color: 'text-wool-400',
    borderColor: 'border-wool-600',
    dotColor: 'bg-wool-500',
    dotClass: 'bg-wool-500',
  },
  working: {
    label: 'Working',
    color: 'text-amber-500',
    borderColor: 'border-amber-500',
    dotColor: 'bg-amber-500',
    dotClass: 'bg-amber-500 animate-pulse',
  },
  waiting: {
    label: 'Waiting',
    color: 'text-golden',
    borderColor: 'border-golden',
    dotColor: 'bg-golden',
    dotClass: 'bg-golden',
  },
  awaiting: {
    label: 'Awaiting',
    color: 'text-sky-500',
    borderColor: 'border-sky-500',
    dotColor: 'bg-sky-500',
    dotClass: 'bg-sky-500',
  },
  paused: {
    label: 'Paused',
    color: 'text-golden',
    borderColor: 'border-golden',
    dotColor: 'bg-golden',
    dotClass: 'bg-golden',
  },
  error: {
    label: 'Error',
    color: 'text-terra',
    borderColor: 'border-terra',
    dotColor: 'bg-terra',
    dotClass: 'bg-terra animate-pulse',
  },
} as const;

export function getWorkerStatusConfig(status: string | null | undefined) {
  return (
    WORKER_STATUS_CONFIG[status as keyof typeof WORKER_STATUS_CONFIG] ?? WORKER_STATUS_CONFIG.idle
  );
}

// =============================================================================
// Run Status Configuration
// =============================================================================

export const RUN_STATUS_CONFIG = {
  draft: {
    label: 'Draft',
    badgeClass: 'bg-sky-500/20 text-sky-400',
  },
  idle: {
    label: 'Idle',
    badgeClass: 'bg-wool-500/20 text-wool-400',
  },
  working: {
    label: 'Working',
    badgeClass: 'bg-amber-500/20 text-amber-400',
  },
  paused: {
    label: 'Paused',
    badgeClass: 'bg-golden/20 text-golden',
  },
  runaway: {
    label: 'Runaway',
    badgeClass: 'bg-terra/20 text-terra',
  },
  timed_out: {
    label: 'Timed Out',
    badgeClass: 'bg-terra/20 text-terra',
  },
  eval: {
    label: 'Evaluating',
    badgeClass: 'bg-amber-400/20 text-amber-300',
  },
  eval_failed: {
    label: 'Eval Failed',
    badgeClass: 'bg-terra/20 text-terra',
  },
  waiting: {
    label: 'Waiting',
    badgeClass: 'bg-golden/20 text-golden',
  },
  done: {
    label: 'Done',
    badgeClass: 'bg-sage/20 text-sage',
  },
  delivered: {
    label: 'Delivered',
    badgeClass: 'bg-sage/20 text-sage',
  },
  merged: {
    label: 'Merged',
    badgeClass: 'bg-sage/20 text-sage',
  },
  failed: {
    label: 'Failed',
    badgeClass: 'bg-terra/20 text-terra',
  },
  // Eval statuses (used in RunDetail)
  running: {
    label: 'Running',
    badgeClass: 'bg-amber-500/20 text-amber-400',
  },
  passed: {
    label: 'Passed',
    badgeClass: 'bg-sage/20 text-sage',
  },
} as const;

export function getRunStatusConfig(status: string | null | undefined) {
  return RUN_STATUS_CONFIG[status as keyof typeof RUN_STATUS_CONFIG] ?? RUN_STATUS_CONFIG.idle;
}

// =============================================================================
// Legacy Functions (for backwards compatibility with existing components)
// =============================================================================

/**
 * Get CSS class for status badge
 */
export function getStatusBadgeClass(status: RunStatus | string | null | undefined): string {
  return getRunStatusConfig(status).badgeClass;
}

/**
 * Get human-readable label for status
 */
export function getStatusLabel(status: RunStatus | string | null | undefined): string {
  return getRunStatusConfig(status).label;
}

/**
 * Get CSS class for status dot indicator
 */
export function getStatusDotClass(status: RunStatus | null | undefined): Record<string, boolean> {
  return {
    'status-draft': status === 'draft',
    'status-working': status === 'working',
    'status-waiting': status === 'waiting' || status === 'paused',
    'status-done': status === 'done' || status === 'delivered' || status === 'merged',
    'status-error': status === 'runaway' || status === 'timed_out' || status === 'eval_failed',
    'status-idle': !status || status === 'idle',
  };
}

/**
 * Get CSS class for progress bar based on status
 */
export function getProgressBarClass(
  status: RunStatus | null | undefined,
  _done?: number,
  _total?: number,
): string {
  // Error states: red
  if (['runaway', 'timed_out', 'eval_failed'].includes(status || '')) {
    return 'bg-terra';
  }
  // Done states: green
  if (['done', 'delivered', 'merged'].includes(status || '')) {
    return 'bg-sage';
  }
  // Waiting/paused: yellow
  if (['waiting', 'paused'].includes(status || '')) {
    return 'bg-golden';
  }
  // Working: amber
  return 'bg-amber-500';
}

/**
 * Get CSS class for worker status
 */
export function getWorkerStatusClass(status: WorkerStatus | null | undefined): string {
  const classes: Record<string, string> = {
    idle: 'status-idle',
    working: 'status-working',
    waiting: 'status-waiting',
    awaiting: 'status-waiting',
    paused: 'status-waiting',
    error: 'status-error',
  };
  return classes[status || 'idle'] || 'status-idle';
}

/**
 * Check if run can be paused
 */
export function canPause(status: RunStatus | null | undefined): boolean {
  return status === 'working' || status === 'eval' || status === 'waiting';
}

/**
 * Check if run can be resumed
 */
export function canResume(status: RunStatus | null | undefined): boolean {
  return status === 'paused' || status === 'timed_out' || status === 'runaway';
}

/**
 * Check if run can be delivered
 */
export function canDeliver(status: RunStatus | null | undefined): boolean {
  return status === 'done' || status === 'paused' || status === 'timed_out';
}
