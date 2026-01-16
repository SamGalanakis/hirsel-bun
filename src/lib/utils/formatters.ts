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
 * Ensure timestamp is parsed as UTC
 */
function parseUtcTimestamp(timestamp: string): Date {
  // Timestamps from backend are UTC but without 'Z' suffix - add it for proper parsing
  const utcTimestamp = timestamp.endsWith('Z') ? timestamp : timestamp + 'Z';
  return new Date(utcTimestamp);
}

/**
 * Format timestamp to 24-hour time string (HH:MM:SS)
 */
export function formatTime(timestamp: string | null | undefined): string {
  if (!timestamp) return '';
  const d = parseUtcTimestamp(timestamp);
  return d.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

/**
 * Format timestamp to short 24-hour time (HH:MM, no seconds)
 */
export function formatTimeShort(timestamp: string | null | undefined): string {
  if (!timestamp) return '';
  const d = parseUtcTimestamp(timestamp);
  return d.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
}

/**
 * Format timestamp to short 24-hour time (HH:MM) - alias for formatTimeShort
 */
export function formatTimeHHMM(timestamp: string | null | undefined): string {
  return formatTimeShort(timestamp);
}

/**
 * Format timestamp to date string (e.g., "Today", "Yesterday", "Jan 15")
 */
export function formatDate(timestamp: string | null | undefined): string {
  if (!timestamp) return '';
  const d = parseUtcTimestamp(timestamp);
  const today = new Date();
  if (d.toDateString() === today.toDateString()) return 'Today';
  const yesterday = new Date(today);
  yesterday.setDate(yesterday.getDate() - 1);
  if (d.toDateString() === yesterday.toDateString()) return 'Yesterday';
  return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
}

/**
 * Format timestamp to full date/time (e.g., "Jan 15, 10:30")
 */
export function formatFullDateTime(timestamp: string | null | undefined): string {
  if (!timestamp) return 'N/A';
  const d = parseUtcTimestamp(timestamp);
  return d.toLocaleString('en-US', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

/**
 * Format elapsed time from a session start timestamp
 */
export function formatElapsedTime(sessionStartedAt: string | null | undefined): string {
  if (!sessionStartedAt) return '';
  const start = parseUtcTimestamp(sessionStartedAt);
  const now = new Date();
  const seconds = Math.floor((now.getTime() - start.getTime()) / 1000);
  if (seconds < 60) return seconds + 's';
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return minutes + 'm';
  const hours = Math.floor(minutes / 60);
  const mins = minutes % 60;
  if (hours < 24) return mins > 0 ? hours + 'h ' + mins + 'm' : hours + 'h';
  const days = Math.floor(hours / 24);
  const hrs = hours % 24;
  return hrs > 0 ? days + 'd ' + hrs + 'h' : days + 'd';
}

/**
 * Format token count to human-readable string (e.g., 1.5M, 12k)
 */
export function formatTokens(tokens: number | null | undefined): string {
  if (tokens == null || tokens === 0) return '0';
  if (tokens < 1000) return String(tokens);
  if (tokens < 1000000) return (tokens / 1000).toFixed(1).replace(/\.0$/, '') + 'k';
  return (tokens / 1000000).toFixed(1).replace(/\.0$/, '') + 'M';
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
 * Format duration between two timestamps (or from start to now if end is null)
 * Returns formats like "2m", "1h 15m", "3h", "2d 4h"
 */
export function formatDuration(
  startTimestamp: string | null | undefined,
  endTimestamp: string | null | undefined = null
): string {
  if (!startTimestamp) return '';
  const start = parseUtcTimestamp(startTimestamp);
  const end = endTimestamp ? parseUtcTimestamp(endTimestamp) : new Date();
  const diffMs = end.getTime() - start.getTime();
  if (diffMs < 0) return '';

  const diffSecs = Math.floor(diffMs / 1000);
  if (diffSecs < 60) return diffSecs + 's';

  const diffMins = Math.floor(diffSecs / 60);
  if (diffMins < 60) return diffMins + 'm';

  const diffHours = Math.floor(diffMins / 60);
  const remMins = diffMins % 60;
  if (diffHours < 24) {
    return remMins > 0 ? diffHours + 'h ' + remMins + 'm' : diffHours + 'h';
  }

  const diffDays = Math.floor(diffHours / 24);
  const remHours = diffHours % 24;
  return remHours > 0 ? diffDays + 'd ' + remHours + 'h' : diffDays + 'd';
}

/**
 * Format a timestamp as relative time (e.g., "2h ago", "3d ago")
 */
export function formatRelativeTime(timestamp: string | null | undefined): string {
  if (!timestamp) return '';
  const d = parseUtcTimestamp(timestamp);
  const now = Date.now();
  const diffMs = now - d.getTime();
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
  return d.toLocaleDateString('en-GB', { day: 'numeric', month: 'short' });
}
