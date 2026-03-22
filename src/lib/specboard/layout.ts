/**
 * Layout utility functions and types for SpecBoard canvas rendering.
 *
 * Pure helpers with no component-level dependencies — extracted from SpecBoard.tsx.
 */

// =============================================================================
// Layout Constants
// =============================================================================

export const CHAR_WIDTH = 5.8;
export const TEXT_PADDING = 20;

// =============================================================================
// Layout Types
// =============================================================================

export interface NodePosition {
  x: number;
  y: number;
  width: number;
  height: number;
  lines: string[];
  layer: number;
}

export interface EdgeRoute {
  from: string;
  to: string;
  type: 'blockedBy' | 'validates' | 'hierarchy' | 'resolves';
  waypoints: [number, number][];
}

export interface LayoutTreeResult {
  positions: Map<string, NodePosition>;
  width: number;
  height: number;
  checksWithValidates: { id: string; validates: string[] }[];
  edgeRoutes: EdgeRoute[];
}

// =============================================================================
// Helper: Wrap text to fit within a given width
// =============================================================================

export function wrapTextToWidth(name: string, width: number): string[] {
  const maxChars = Math.floor((width - TEXT_PADDING) / CHAR_WIDTH);
  const charsPerLine = Math.max(8, maxChars);

  if (name.length <= charsPerLine) {
    return [name];
  }

  const words = name.split(/\s+/);
  const lines: string[] = [];
  let currentLine = '';

  for (const word of words) {
    const testLine = currentLine ? `${currentLine} ${word}` : word;
    if (testLine.length <= charsPerLine) {
      currentLine = testLine;
    } else {
      if (currentLine) lines.push(currentLine);
      currentLine = word.length > charsPerLine ? `${word.slice(0, charsPerLine - 1)}\u2026` : word;
    }
  }
  if (currentLine) lines.push(currentLine);

  if (lines.length > 2) {
    lines.length = 2;
    lines[1] = `${lines[1].slice(0, -1)}\u2026`;
  }

  return lines;
}

// =============================================================================
// Smooth Edge Path Builder
// =============================================================================

export function buildSmoothPath(waypoints: [number, number][]): string {
  if (waypoints.length < 2) return '';
  if (waypoints.length === 2) {
    return `M ${waypoints[0][0]} ${waypoints[0][1]} L ${waypoints[1][0]} ${waypoints[1][1]}`;
  }
  const parts: string[] = [`M ${waypoints[0][0]} ${waypoints[0][1]}`];
  const radius = 6;

  for (let i = 1; i < waypoints.length - 1; i++) {
    const prev = waypoints[i - 1];
    const curr = waypoints[i];
    const next = waypoints[i + 1];

    const dx1 = curr[0] - prev[0];
    const dy1 = curr[1] - prev[1];
    const len1 = Math.sqrt(dx1 * dx1 + dy1 * dy1);
    const dx2 = next[0] - curr[0];
    const dy2 = next[1] - curr[1];
    const len2 = Math.sqrt(dx2 * dx2 + dy2 * dy2);

    const r1 = Math.min(radius, len1 / 2);
    const r2 = Math.min(radius, len2 / 2);

    const beforeX = curr[0] - (dx1 / len1) * r1;
    const beforeY = curr[1] - (dy1 / len1) * r1;
    const afterX = curr[0] + (dx2 / len2) * r2;
    const afterY = curr[1] + (dy2 / len2) * r2;

    parts.push(`L ${beforeX} ${beforeY}`);
    parts.push(`Q ${curr[0]} ${curr[1]} ${afterX} ${afterY}`);
  }

  const last = waypoints[waypoints.length - 1];
  parts.push(`L ${last[0]} ${last[1]}`);
  return parts.join(' ');
}
