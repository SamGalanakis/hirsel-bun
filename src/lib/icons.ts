/**
 * Lucide icon utilities for Hirsel
 *
 * Provides icon SVG strings for use in Alpine.js templates
 * and initializes Lucide icon replacement in the DOM.
 */

import { createIcons, icons } from 'lucide';

// Re-export createIcons for initialization
export { createIcons, icons };

/**
 * Initialize Lucide icons - call this after DOM is ready
 * Replaces all <i data-lucide="icon-name"></i> elements with SVG icons
 * Also sets up a MutationObserver to handle dynamically added elements
 */
export function initLucideIcons(): void {
  // Initial icon replacement
  createIcons({ icons });

  // Debounced re-initialization for Alpine-rendered icons
  let pendingRefresh = false;
  const refreshIcons = () => {
    if (pendingRefresh) return;
    pendingRefresh = true;
    requestAnimationFrame(() => {
      createIcons({ icons });
      pendingRefresh = false;
    });
  };

  // Listen for Alpine updates to refresh icons
  document.addEventListener('alpine:initialized', refreshIcons);

  // Also refresh after a short delay to catch initial Alpine render
  setTimeout(refreshIcons, 100);
  setTimeout(refreshIcons, 500);
}

/**
 * Convert kebab-case to PascalCase for Lucide icon lookup
 * e.g., 'arrow-right' -> 'ArrowRight'
 */
function toPascalCase(str: string): string {
  return str
    .split('-')
    .map(part => part.charAt(0).toUpperCase() + part.slice(1))
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
