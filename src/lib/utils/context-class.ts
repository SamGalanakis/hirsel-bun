/**
 * Context utilization color helper
 */

/**
 * Get CSS class for context utilization percentage
 * - >=90%: terra (red, danger)
 * - >=75%: amber-500 (yellow, warning)
 * - <75%: wool-500 (gray, normal)
 */
export function getContextClass(utilization: number | null | undefined): string {
  if (utilization == null) return 'text-wool-500';
  if (utilization >= 90) return 'text-terra';
  if (utilization >= 75) return 'text-amber-500';
  return 'text-wool-500';
}

/**
 * Get background class for context utilization bar
 */
export function getContextBarClass(utilization: number | null | undefined): string {
  if (utilization == null) return 'bg-wool-600';
  if (utilization >= 90) return 'bg-terra';
  if (utilization >= 75) return 'bg-amber-500';
  return 'bg-sage';
}

/**
 * Get status description for context utilization
 */
export function getContextStatus(utilization: number | null | undefined): string {
  if (utilization == null) return 'Unknown';
  if (utilization >= 90) return 'Critical';
  if (utilization >= 75) return 'High';
  if (utilization >= 50) return 'Moderate';
  return 'Low';
}
