/**
 * Status-related utilities for the Hirsel GUI
 */

import type { RunStatus, TaskStatus, WorkerStatus } from '../types';

/**
 * Get CSS class for status badge
 */
export function getStatusBadgeClass(status: RunStatus | null | undefined): string {
  const classes: Record<string, string> = {
    draft: 'bg-sky-500/20 text-sky-400',
    idle: 'bg-wool-700/20 text-wool-500',
    working: 'bg-amber-500/20 text-amber-500',
    paused: 'bg-golden/20 text-golden',
    runaway: 'bg-terra/20 text-terra',
    timed_out: 'bg-terra/20 text-terra',
    eval: 'bg-amber-400/20 text-amber-400',
    eval_failed: 'bg-terra/20 text-terra',
    waiting: 'bg-golden/20 text-golden',
    done: 'bg-sage/20 text-sage',
    delivered: 'bg-sage/20 text-sage',
    merged: 'bg-sage/20 text-sage',
  };
  return classes[status || 'idle'] || classes.idle;
}

/**
 * Get human-readable label for status
 */
export function getStatusLabel(status: RunStatus | null | undefined): string {
  const labels: Record<string, string> = {
    draft: 'Draft',
    idle: 'Idle',
    working: 'Working',
    paused: 'Paused',
    runaway: 'Runaway',
    timed_out: 'Timed Out',
    eval: 'Evaluating',
    eval_failed: 'Eval Failed',
    waiting: 'Waiting',
    done: 'Done',
    delivered: 'Delivered',
    merged: 'Merged',
  };
  return labels[status || ''] || status || 'Unknown';
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
 * Get CSS class for task status
 */
export function getTaskStatusClass(status: TaskStatus | null | undefined): string {
  const classes: Record<string, string> = {
    todo: 'text-wool-600',
    doing: 'text-amber-500',
    done: 'text-sage',
  };
  return classes[status || 'todo'] || 'text-wool-600';
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
