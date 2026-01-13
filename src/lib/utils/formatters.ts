/**
 * Shared formatting utilities for the Hirsel GUI
 */

/**
 * Format elapsed time in minutes to human-readable string
 */
export function formatElapsed(minutes: number | null | undefined): string {
  if (!minutes) return '0m';
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
