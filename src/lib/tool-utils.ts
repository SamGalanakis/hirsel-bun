/**
 * Shared tool display utilities
 *
 * Used by WorkerOutputViewer and GypMessenger for consistent tool display.
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
 * - "mcp__hirsel__board_view" → "board_view"
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
