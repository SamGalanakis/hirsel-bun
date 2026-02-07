/**
 * Theme-aware color helpers using CSS custom properties.
 *
 * These produce `rgba(var(--x-rgb), alpha)` strings that automatically
 * adapt to the active theme.
 */

export const amber = (a: number) => `rgba(var(--amber-500-rgb), ${a})`;
export const sage = (a: number) => `rgba(var(--sage-rgb), ${a})`;
export const terra = (a: number) => `rgba(var(--terra-rgb), ${a})`;
