/**
 * Shared formatting utilities for the Hirsel GUI
 */

/**
 * Format elapsed time in minutes to human-readable string
 * Shows seconds for times under 1 minute
 */
export function formatElapsed(minutes: number | null | undefined): string {
  if (minutes === null || minutes === undefined) return '0s';
  if (minutes < 1) {
    const seconds = Math.round(minutes * 60);
    return seconds + 's';
  }
  const m = Math.round(minutes);
  if (m < 60) return m + 'm';
  const h = Math.floor(m / 60);
  const rem = m % 60;
  return h + 'h' + (rem > 0 ? ' ' + rem + 'm' : '');
}

/**
 * Format time remaining given limit and elapsed
 */
export function formatTimeRemaining(
  limit: number | null | undefined,
  elapsed: number | null | undefined
): string {
  if (!limit) return '';
  const remaining = limit - (elapsed || 0);
  if (remaining <= 0) return 'Time up!';
  if (remaining < 60) return remaining + 'm remaining';
  const h = Math.floor(remaining / 60);
  const m = remaining % 60;
  return m > 0 ? h + 'h ' + m + 'm remaining' : h + 'h remaining';
}

/**
 * Format timestamp to 24-hour time string
 */
export function formatTime(timestamp: string | null | undefined): string {
  if (!timestamp) return '';
  const d = new Date(timestamp);
  return d.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

/**
 * Format timestamp to short 24-hour time (no seconds)
 */
export function formatTimeShort(timestamp: string | null | undefined): string {
  if (!timestamp) return '';
  const d = new Date(timestamp);
  return d.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
}

/**
 * Format token count to human-readable string (e.g., 1.5M, 12k)
 */
export function formatTokens(tokens: number | null | undefined): string {
  if (!tokens) return '0';
  if (tokens >= 1000000) return (tokens / 1000000).toFixed(1) + 'M';
  if (tokens >= 1000) return (tokens / 1000).toFixed(1) + 'k';
  return String(tokens);
}

/**
 * Format progress as percentage
 */
export function formatProgress(done: number, total: number): number {
  if (total === 0) return 0;
  return Math.round((done / total) * 100);
}

/**
 * Calculate time progress percentage
 */
export function calculateTimeProgress(
  elapsed: number | null | undefined,
  limit: number | null | undefined
): number {
  if (!limit || !elapsed) return 0;
  return Math.min(100, Math.round((elapsed / limit) * 100));
}

/**
 * Format a timestamp as relative time (e.g., "2h ago", "3d ago")
 */
export function formatRelativeTime(timestamp: string | null | undefined): string {
  if (!timestamp) return '';
  // Timestamps from backend are UTC but without 'Z' suffix - add it for proper parsing
  const utcTimestamp = timestamp.endsWith('Z') ? timestamp : timestamp + 'Z';
  const now = Date.now();
  const then = new Date(utcTimestamp).getTime();
  const diffMs = now - then;
  const diffMins = Math.floor(diffMs / 60000);

  if (diffMins < 1) return 'just now';
  if (diffMins < 60) return diffMins + 'm ago';

  const diffHours = Math.floor(diffMins / 60);
  if (diffHours < 24) return diffHours + 'h ago';

  const diffDays = Math.floor(diffHours / 24);
  if (diffDays < 7) return diffDays + 'd ago';

  const diffWeeks = Math.floor(diffDays / 7);
  if (diffWeeks < 4) return diffWeeks + 'w ago';

  // For older dates, show the actual date
  const d = new Date(utcTimestamp);
  return d.toLocaleDateString('en-GB', { day: 'numeric', month: 'short' });
}
