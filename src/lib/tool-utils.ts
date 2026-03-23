/**
 * Shared tool display utilities
 *
 * Used by WorkerOutputViewer and ShepherdConsole for consistent tool display.
 */

/** Get icon name based on tool kind */
export function getToolIcon(kind: string | null | undefined): string {
  switch (kind) {
    case 'read':
      return 'file-text';
    case 'write':
    case 'edit':
      return 'pencil';
    case 'execute':
      return 'terminal';
    case 'search':
      return 'search';
    case 'fetch':
      return 'globe';
    case 'think':
      return 'brain';
    default:
      return 'wrench';
  }
}

/** Get icon name based on tool status */
export function getToolStatusIcon(status: string | null | undefined): string {
  switch (status) {
    case 'completed':
      return 'check';
    case 'failed':
      return 'x';
    case 'in_progress':
      return 'loader';
    default:
      return 'clock';
  }
}

/** Get CSS color class based on tool status */
export function getToolStatusColor(status: string | null | undefined): string {
  switch (status) {
    case 'completed':
      return 'text-sage';
    case 'failed':
      return 'text-terra';
    case 'in_progress':
      return 'text-amber-500';
    default:
      return 'text-wool-500';
  }
}

/**
 * Get a short display label from a tool title.
 *
 * Examples:
 * - "mcp__hirsel__get_route_work_tree" → "get_route_work_tree"
 * - "Read" → "Read"
 * - "mcp__eval__run_tests" → "run_tests"
 */
export function getToolShortLabel(title: string | null | undefined): string {
  if (!title) return 'Tool';

  // Handle MCP-style names: mcp__server__tool_name → tool_name
  if (title.includes('__')) {
    const parts = title.split('__');
    return parts[parts.length - 1] || title;
  }

  // Handle simple names or extract first word
  const firstWord = title.split(/[\s:(]/)[0];
  return firstWord || title;
}

/** Check if tool is currently working (pending or in_progress) */
export function isToolWorking(status: string | null | undefined): boolean {
  return status === 'pending' || status === 'in_progress';
}

/**
 * Determine effective tool status from events.
 *
 * Tool events don't always send explicit `in_progress` status.
 * If a tool has started but not completed/failed, it's effectively running.
 */
export function getEffectiveToolStatus(
  hasStarted: boolean,
  latestStatus: string | null | undefined,
  isFollowedByContent = false,
): string {
  // If completed or failed, use that
  if (latestStatus === 'completed' || latestStatus === 'failed') {
    return latestStatus;
  }

  // If subsequent content exists, the tool must have completed
  // (the agent can't continue without finishing the tool call)
  if (isFollowedByContent) {
    return 'completed';
  }

  // If tool has started (tool_start event exists), it's running
  if (hasStarted) {
    return 'in_progress';
  }

  // Otherwise pending
  return latestStatus || 'pending';
}

/** Tool status indicator with distinct visual treatment for pending vs in_progress */
export interface ToolStatusIndicator {
  icon: string;
  color: string;
  animate: boolean;
  label: string;
}

/** Get distinct visual indicator based on tool status */
export function getToolStatusIndicator(status: string | null | undefined): ToolStatusIndicator {
  switch (status) {
    case 'pending':
      return { icon: 'clock', color: 'text-wool-500', animate: false, label: 'Queued' };
    case 'in_progress':
      return { icon: 'loader', color: 'text-amber-500', animate: true, label: 'Running' };
    case 'completed':
      return { icon: 'check', color: 'text-sage', animate: false, label: 'Done' };
    case 'failed':
      return { icon: 'x', color: 'text-terra', animate: false, label: 'Failed' };
    default:
      return { icon: 'circle', color: 'text-wool-600', animate: false, label: '' };
  }
}

/** Get pip CSS class based on tool status */
export function getToolPipClass(status: string | null | undefined): string {
  switch (status) {
    case 'pending':
      return 'pending';
    case 'in_progress':
      return 'working';
    case 'completed':
      return 'done';
    case 'failed':
      return 'failed';
    default:
      return 'pending';
  }
}
