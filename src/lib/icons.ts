/**
 * Lucide icon utilities for Hirsel
 *
 * Provides icon SVG strings for use in SolidJS components via the Icon component.
 */

import { icons } from 'lucide';

/**
 * Convert kebab-case to PascalCase for Lucide icon lookup
 * e.g., 'arrow-right' -> 'ArrowRight'
 */
function toPascalCase(str: string): string {
  return str
    .split('-')
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join('');
}

/**
 * Get an icon SVG string by name for use in dynamic templates
 * @param name - Icon name in kebab-case (e.g., 'play', 'pause', 'check', 'arrow-right')
 * @param size - Icon size in pixels (default: 16)
 * @param strokeWidth - Stroke width (default: 2)
 * @returns SVG string or empty string if icon not found
 */
export function getIcon(name: string, size = 16, strokeWidth = 2): string {
  const pascalName = toPascalCase(name);
  const iconData = icons[pascalName as keyof typeof icons];
  if (!iconData) {
    console.warn(`[Icons] Unknown icon: ${name} (looked up as ${pascalName})`);
    return '';
  }

  // iconData is an array of [tag, attributes] tuples
  const paths = (iconData as Array<[string, Record<string, string>]>)
    .map(([tag, attrs]) => {
      const attrStr = Object.entries(attrs)
        .map(([k, v]) => `${k}="${v}"`)
        .join(' ');
      return `<${tag} ${attrStr}/>`;
    })
    .join('');

  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="${strokeWidth}" stroke-linecap="round" stroke-linejoin="round" class="lucide lucide-${name}">${paths}</svg>`;
}

/**
 * Icon name mappings for activity log actions
 */
export const ACTION_ICON_NAMES: Record<string, string> = {
  task_claimed: 'arrow-right',
  task_done: 'check',
  task_added: 'plus',
  task_deleted: 'x',
  task_unclaimed: 'arrow-left',
  task_reopened: 'rotate-ccw',
  worker_started: 'play',
  worker_stopped: 'square',
  worker_error: 'alert-triangle',
  worker_paused: 'pause',
  worker_resumed: 'play',
  eval_started: 'settings',
  eval_passed: 'check',
  eval_failed: 'x',
  run_started: 'play',
  run_paused: 'pause',
  run_resumed: 'play',
  run_done: 'check',
  run_delivered: 'package',
  run_timed_out: 'clock',
  message_sent: 'arrow-right',
  message_received: 'arrow-left',
  default: 'circle',
};

/**
 * Icon name mappings for task status
 */
export const TASK_STATUS_ICON_NAMES: Record<string, string> = {
  todo: 'circle',
  doing: 'circle-dot',
  done: 'check-circle',
};

/**
 * Icon name mappings for worker status
 */
export const WORKER_STATUS_ICON_NAMES: Record<string, string> = {
  idle: 'circle',
  working: 'circle-dot',
  waiting: 'clock',
  awaiting: 'clock',
  paused: 'pause-circle',
  done: 'check-circle',
  error: 'alert-circle',
};

/**
 * Get action icon SVG for activity log
 */
export function getActionIcon(action: string, size = 14): string {
  const normalized = action.toLowerCase().replace(/[\s-]+/g, '_');
  const iconName = ACTION_ICON_NAMES[normalized] || ACTION_ICON_NAMES.default;
  return getIcon(iconName, size);
}

/**
 * Get task status icon SVG
 */
export function getTaskStatusIcon(status: string, size = 16): string {
  const iconName = TASK_STATUS_ICON_NAMES[status] || 'circle';
  return getIcon(iconName, size);
}

/**
 * Get worker status icon SVG
 */
export function getWorkerStatusIcon(status: string, size = 16): string {
  const iconName = WORKER_STATUS_ICON_NAMES[status] || 'circle';
  return getIcon(iconName, size);
}

/**
 * Icon name mappings for tool kinds (AI tool calls)
 */
export const TOOL_KIND_ICON_NAMES: Record<string, string> = {
  read: 'file-text',
  edit: 'pencil',
  delete: 'trash-2',
  move: 'move',
  search: 'search',
  execute: 'terminal',
  think: 'brain',
  fetch: 'globe',
  switch_mode: 'toggle-left',
  default: 'wrench',
};

/**
 * Icon name mappings for tool call status
 */
export const TOOL_STATUS_ICON_NAMES: Record<string, string> = {
  pending: 'clock',
  in_progress: 'loader-2',
  completed: 'check',
  failed: 'x',
};

/**
 * Get tool kind icon SVG
 */
export function getToolKindIcon(kind: string | null, size = 14): string {
  const iconName = TOOL_KIND_ICON_NAMES[kind || ''] || TOOL_KIND_ICON_NAMES.default;
  return getIcon(iconName, size);
}

/**
 * Get tool status icon SVG
 */
export function getToolStatusIcon(status: string, size = 12): string {
  const iconName = TOOL_STATUS_ICON_NAMES[status] || 'clock';
  return getIcon(iconName, size);
}
