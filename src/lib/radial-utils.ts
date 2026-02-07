/**
 * Shared utilities for radial / half-moon menus.
 *
 * Extracted from RadialMenu.tsx and WorkerHalfMoon.tsx so that angle
 * calculation, style resolution, and SVG arc-path generation live in
 * one place.
 */

// ---------------------------------------------------------------------------
// findClosestItem
// ---------------------------------------------------------------------------

interface AngleItem {
  angle: number;
}

/**
 * Find the closest menu item based on mouse offset from center.
 * Returns null if within dead zone or (for half-moon) if angle is in excluded range.
 */
export function findClosestItem<T extends AngleItem>(
  offsetX: number,
  offsetY: number,
  items: T[],
  deadZone: number,
  angleRestriction?: { min: number; max: number },
): { index: number; angle: number } | null {
  const distance = Math.sqrt(offsetX * offsetX + offsetY * offsetY);
  if (distance < deadZone) return null;

  let angle = Math.atan2(-offsetY, offsetX) * (180 / Math.PI);
  angle = (90 - angle + 360) % 360;

  if (angleRestriction && (angle < angleRestriction.min || angle > angleRestriction.max)) {
    return null;
  }

  let closestIndex = 0;
  let closestDiff = 360;

  for (let i = 0; i < items.length; i++) {
    let diff = Math.abs(items[i].angle - angle);
    if (diff > 180) diff = 360 - diff;
    if (diff < closestDiff) {
      closestDiff = diff;
      closestIndex = i;
    }
  }

  return { index: closestIndex, angle };
}

// ---------------------------------------------------------------------------
// Radial item styles
// ---------------------------------------------------------------------------

export const RADIAL_STYLES = {
  disabled: {
    bg: 'rgba(25, 25, 25, 0.95)',
    border: 'rgba(40, 40, 40, 0.8)',
    color: 'var(--wool-700)',
    shadow: '0 2px 8px rgba(0, 0, 0, 0.2)',
  },
  default: {
    selected: {
      bg: 'rgba(70, 70, 70, 0.95)',
      border: 'rgba(140, 140, 140, 0.9)',
      color: 'var(--wool-50)',
      shadow: '0 0 16px rgba(255, 255, 255, 0.1), 0 4px 16px rgba(0, 0, 0, 0.3)',
    },
    normal: {
      bg: 'rgba(35, 35, 35, 0.95)',
      border: 'rgba(55, 55, 55, 0.8)',
      color: 'var(--wool-300)',
      shadow: '0 2px 8px rgba(0, 0, 0, 0.2)',
    },
  },
  primary: {
    selected: {
      bg: 'rgba(212, 165, 116, 0.4)',
      border: 'rgba(212, 165, 116, 0.9)',
      color: 'var(--amber-300)',
      shadow: '0 0 20px rgba(212, 165, 116, 0.5), 0 4px 16px rgba(0, 0, 0, 0.3)',
    },
    normal: {
      bg: 'rgba(212, 165, 116, 0.15)',
      border: 'rgba(212, 165, 116, 0.4)',
      color: 'var(--amber-400)',
      shadow: '0 2px 8px rgba(0, 0, 0, 0.2)',
    },
  },
  success: {
    selected: {
      bg: 'rgba(125, 153, 112, 0.4)',
      border: 'rgba(125, 153, 112, 0.9)',
      color: 'var(--sage)',
      shadow: '0 0 20px rgba(125, 153, 112, 0.5), 0 4px 16px rgba(0, 0, 0, 0.3)',
    },
    normal: {
      bg: 'rgba(125, 153, 112, 0.15)',
      border: 'rgba(125, 153, 112, 0.4)',
      color: 'var(--sage)',
      shadow: '0 2px 8px rgba(0, 0, 0, 0.2)',
    },
  },
} as const;

export type ItemStyle = { bg: string; border: string; color: string; shadow: string };

export function getRadialItemStyle(
  variant: string,
  isSelected: boolean,
  isDisabled: boolean,
): ItemStyle {
  if (isDisabled) return RADIAL_STYLES.disabled;
  const variantStyles =
    RADIAL_STYLES[variant as keyof typeof RADIAL_STYLES] ?? RADIAL_STYLES.default;
  if ('selected' in variantStyles) {
    return isSelected ? variantStyles.selected : variantStyles.normal;
  }
  return RADIAL_STYLES.default.normal;
}

// ---------------------------------------------------------------------------
// SVG arc-path builder
// ---------------------------------------------------------------------------

export function renderSlicePath(
  cx: number,
  cy: number,
  startAngle: number,
  endAngle: number,
  innerR: number,
  outerR: number,
): string {
  const startRad = ((startAngle - 90) * Math.PI) / 180;
  const endRad = ((endAngle - 90) * Math.PI) / 180;

  const x1 = cx + Math.cos(startRad) * innerR;
  const y1 = cy + Math.sin(startRad) * innerR;
  const x2 = cx + Math.cos(startRad) * outerR;
  const y2 = cy + Math.sin(startRad) * outerR;
  const x3 = cx + Math.cos(endRad) * outerR;
  const y3 = cy + Math.sin(endRad) * outerR;
  const x4 = cx + Math.cos(endRad) * innerR;
  const y4 = cy + Math.sin(endRad) * innerR;

  return `M ${x1} ${y1} L ${x2} ${y2} A ${outerR} ${outerR} 0 0 1 ${x3} ${y3} L ${x4} ${y4} A ${innerR} ${innerR} 0 0 0 ${x1} ${y1} Z`;
}
