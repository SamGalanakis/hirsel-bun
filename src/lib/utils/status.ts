/**
 * Status-related utilities for the Hirsel GUI
 *
 * Centralized worker status configuration used by the current UI.
 * The old run-status shell is gone, so this module stays focused on worker state.
 */

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

/**
 * Get box-shadow glow style for a worker based on status
 */
export function getWorkerGlowStyle(status: string): string | undefined {
  if (status === 'working') return '0 0 12px rgba(var(--amber-500-rgb), 0.4)';
  if (status === 'error') return '0 0 12px rgba(var(--terra-rgb), 0.4)';
  return undefined;
}
