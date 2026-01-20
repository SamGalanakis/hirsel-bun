/**
 * Validation utilities for draft editor
 */

/**
 * Parse time limit string to minutes
 * Returns { value, error } where value is null if empty or invalid
 */
export function parseTimeLimit(input: string): { value: number | null; error: string | null } {
  if (!input.trim()) return { value: null, error: null }; // Empty is valid (no limit)
  const s = input.trim().toLowerCase();

  // Check for combined format like "1h30m"
  if (s.includes('h') && s.includes('m')) {
    const hMatch = s.match(/^(\d+(?:\.\d+)?)h(\d+)m$/);
    if (!hMatch) {
      return { value: null, error: 'Invalid format. Use: 30m, 1h, or 1h30m' };
    }
    const hours = Number.parseFloat(hMatch[1]);
    const mins = Number.parseInt(hMatch[2], 10);
    if (mins >= 60) {
      return { value: null, error: 'Minutes should be less than 60' };
    }
    const total = Math.round(hours * 60 + mins);
    if (total <= 0) {
      return { value: null, error: 'Time limit must be positive' };
    }
    return { value: total, error: null };
  }

  // Hours format
  if (s.endsWith('h')) {
    const num = Number.parseFloat(s.slice(0, -1));
    if (Number.isNaN(num)) {
      return { value: null, error: 'Invalid hours value' };
    }
    if (num <= 0) {
      return { value: null, error: 'Time limit must be positive' };
    }
    return { value: Math.round(num * 60), error: null };
  }

  // Minutes format
  if (s.endsWith('m')) {
    const num = Number.parseFloat(s.slice(0, -1));
    if (Number.isNaN(num)) {
      return { value: null, error: 'Invalid minutes value' };
    }
    if (num <= 0) {
      return { value: null, error: 'Time limit must be positive' };
    }
    return { value: Math.round(num), error: null };
  }

  // Plain number - assume minutes
  const num = Number.parseFloat(s);
  if (Number.isNaN(num)) {
    return { value: null, error: 'Invalid format. Use: 30m, 1h, or 1h30m' };
  }
  if (num <= 0) {
    return { value: null, error: 'Time limit must be positive' };
  }
  return { value: Math.round(num), error: null };
}

/**
 * Validate worker scale string
 * Valid formats: "3" (fixed), "1-5" (range), "2+" (min with no max)
 */
export function validateWorkerScale(input: string): { value: string | null; error: string | null } {
  const s = input.trim();
  if (!s) {
    return { value: null, error: 'Workers is required' };
  }

  // Check for "N+" pattern (autoscale from N)
  if (s.endsWith('+')) {
    const num = Number.parseInt(s.slice(0, -1), 10);
    if (Number.isNaN(num) || num < 1) {
      return { value: null, error: 'Minimum workers must be at least 1' };
    }
    return { value: s, error: null };
  }

  // Check for "N-M" pattern (range)
  if (s.includes('-')) {
    const parts = s.split('-');
    if (parts.length !== 2) {
      return { value: null, error: 'Invalid range. Use: 1-5' };
    }
    const min = Number.parseInt(parts[0], 10);
    const max = Number.parseInt(parts[1], 10);
    if (Number.isNaN(min) || Number.isNaN(max)) {
      return { value: null, error: 'Invalid range. Use numbers like: 1-5' };
    }
    if (min < 1) {
      return { value: null, error: 'Minimum workers must be at least 1' };
    }
    if (max < min) {
      return { value: null, error: 'Maximum must be greater than minimum' };
    }
    return { value: s, error: null };
  }

  // Simple number - fixed count
  const num = Number.parseInt(s, 10);
  if (Number.isNaN(num)) {
    return { value: null, error: 'Enter a number (e.g., 1, 1-3, or 2+)' };
  }
  if (num < 1) {
    return { value: null, error: 'Workers must be at least 1' };
  }
  return { value: s, error: null };
}

/**
 * Format minutes to display string
 */
export function formatTimeLimitDisplay(minutes: number | null): string {
  if (minutes === null) return '';
  if (minutes < 60) return `${minutes}m`;
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return m > 0 ? `${h}h${m}m` : `${h}h`;
}
