/**
 * Activity log utilities - action colors, icons, and formatting
 */

/** Color classes for activity actions */
export const ACTION_COLORS: Record<string, string> = {
  // Task actions
  task_claimed: 'text-amber-400',
  task_done: 'text-sage',
  task_added: 'text-sky-400',
  task_reopened: 'text-amber-300',
  task_unclaimed: 'text-wool-400',
  task_deleted: 'text-terra',
  task_blocked: 'text-golden',
  task_unblocked: 'text-sage',

  // Worker actions
  worker_started: 'text-sage',
  worker_stopped: 'text-wool-500',
  worker_paused: 'text-golden',
  worker_resumed: 'text-sage',
  worker_error: 'text-terra',
  worker_waiting: 'text-golden',

  // Run actions
  run_started: 'text-sage',
  run_paused: 'text-golden',
  run_resumed: 'text-sage',
  run_completed: 'text-sage',
  run_failed: 'text-terra',
  run_delivered: 'text-sage',

  // Eval actions
  eval_started: 'text-amber-400',
  eval_passed: 'text-sage',
  eval_failed: 'text-terra',

  // Message actions
  message_sent: 'text-sky-400',
  message_received: 'text-wool-300',

  // Default
  default: 'text-wool-400',
};

/** Background classes for activity actions */
export const ACTION_BG_COLORS: Record<string, string> = {
  task_claimed: 'bg-amber-500/10',
  task_done: 'bg-sage/10',
  task_added: 'bg-sky-500/10',
  task_deleted: 'bg-terra/10',
  worker_error: 'bg-terra/10',
  run_failed: 'bg-terra/10',
  eval_passed: 'bg-sage/10',
  eval_failed: 'bg-terra/10',
  default: 'bg-transparent',
};

/** Lucide icons for activity actions */
export const ACTION_ICONS: Record<string, string> = {
  // Task actions
  task_claimed: 'user-check',
  task_done: 'check-circle',
  task_added: 'plus-circle',
  task_reopened: 'rotate-ccw',
  task_unclaimed: 'user-minus',
  task_deleted: 'trash-2',
  task_blocked: 'lock',
  task_unblocked: 'unlock',

  // Worker actions
  worker_started: 'play',
  worker_stopped: 'square',
  worker_paused: 'pause',
  worker_resumed: 'play',
  worker_error: 'alert-triangle',
  worker_waiting: 'clock',

  // Run actions
  run_started: 'play-circle',
  run_paused: 'pause-circle',
  run_resumed: 'play-circle',
  run_completed: 'check-circle-2',
  run_failed: 'x-circle',
  run_delivered: 'git-pull-request',

  // Eval actions
  eval_started: 'clipboard-check',
  eval_passed: 'check-circle',
  eval_failed: 'x-circle',

  // Message actions
  message_sent: 'send',
  message_received: 'message-square',

  // Default
  default: 'activity',
};

/**
 * Get the color class for an action
 */
export function getActionColor(action: string): string {
  // Check exact match first
  if (action in ACTION_COLORS) {
    return ACTION_COLORS[action];
  }

  // Check for prefix matches (e.g., "task_claimed" from "task_claimed by worker-0")
  for (const key of Object.keys(ACTION_COLORS)) {
    if (action.startsWith(key)) {
      return ACTION_COLORS[key];
    }
  }

  return ACTION_COLORS.default;
}

/**
 * Get the background class for an action
 */
export function getActionBgClass(action: string): string {
  if (action in ACTION_BG_COLORS) {
    return ACTION_BG_COLORS[action];
  }

  for (const key of Object.keys(ACTION_BG_COLORS)) {
    if (action.startsWith(key)) {
      return ACTION_BG_COLORS[key];
    }
  }

  return ACTION_BG_COLORS.default;
}

/**
 * Get the icon name for an action
 */
export function getActionIcon(action: string): string {
  if (action in ACTION_ICONS) {
    return ACTION_ICONS[action];
  }

  for (const key of Object.keys(ACTION_ICONS)) {
    if (action.startsWith(key)) {
      return ACTION_ICONS[key];
    }
  }

  return ACTION_ICONS.default;
}

/**
 * Format action label for display (capitalize, replace underscores)
 */
export function formatActionLabel(action: string): string {
  return action
    .split('_')
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(' ');
}

/**
 * Extract worker name from action detail if present
 */
export function extractWorkerName(detail: string | null): string | null {
  if (!detail) return null;

  // Match patterns like "by worker-0" or "worker-0:"
  const match = detail.match(/(?:by\s+)?(\w+-\d+)/i);
  return match ? match[1] : null;
}
