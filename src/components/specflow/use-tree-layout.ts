/**
 * Tree layout hook using d3-hierarchy
 *
 * Layout uses fixed dimensions so positions never change when zooming.
 * Visual rendering uses LOAD (Level of Detail) based on zoom level.
 */

import { hierarchy, tree } from 'd3-hierarchy';
import type { BoardEval, TaskTree } from '../../lib/types';

export interface NodePosition {
  x: number;
  y: number;
}

export interface TreeLayoutResult {
  /** Map of node ID to position */
  positions: Map<string, NodePosition>;
  /** Bounding box of the entire tree */
  bounds: {
    minX: number;
    minY: number;
    maxX: number;
    maxY: number;
    width: number;
    height: number;
  };
}

/**
 * Fixed layout dimensions - used for positioning only.
 * This ensures positions never change when zooming.
 */
export const LAYOUT_CONFIG = {
  nodeWidth: 280,
  nodeHeight: 180,
  horizontalGap: 40,
  verticalGap: 64,
} as const;

/**
 * Render configs for different LOAD levels.
 * Compact and full are now similar size - compact just shows less detail.
 * This enables Google Maps-style semantic zoom where cards stay readable.
 */
export const RENDER_CONFIGS = {
  compact: {
    nodeWidth: 280,
    nodeHeight: 120,
  },
  full: {
    nodeWidth: 280,
    nodeHeight: 180,
  },
} as const;

/** Level of detail based on zoom level */
export type LOADLevel = 'compact' | 'full';

/**
 * Get LOAD level for tasks based on zoom factor
 *
 * Tasks are only visible when focused on a project (k >= FOCUS_THRESHOLD = 0.8)
 * Stay in full view as long as content is readable, switch to compact only
 * when zoomed out far enough that details become hard to read.
 * - full:    k >= 0.9 - Full detail cards (readable content)
 * - compact: k < 0.9  - Title-only cards (zoomed out)
 */
export function getLOADLevel(zoom: number): LOADLevel {
  if (zoom < 0.9) return 'compact';
  return 'full';
}

/** Get render config for current LOAD */
export function getRenderConfig(load: LOADLevel) {
  return RENDER_CONFIGS[load];
}

/**
 * Compute tree layout for a list of root nodes and evals
 *
 * Uses d3-hierarchy's tree layout algorithm for initial positioning.
 * Both tasks and evals use stored x/y positions if available, otherwise computed.
 * Tasks and evals are treated identically for consistency.
 */
export function computeTreeLayout(roots: TaskTree[], evalList: BoardEval[] = []): TreeLayoutResult {
  const positions = new Map<string, NodePosition>();
  const config = LAYOUT_CONFIG;

  if (roots.length === 0 && evalList.length === 0) {
    return {
      positions,
      bounds: { minX: 0, minY: 0, maxX: 0, maxY: 0, width: 0, height: 0 },
    };
  }

  let offsetX = 0;
  let globalMinX = Number.POSITIVE_INFINITY;
  let globalMaxX = Number.NEGATIVE_INFINITY;
  let globalMinY = Number.POSITIVE_INFINITY;
  let globalMaxY = Number.NEGATIVE_INFINITY;

  // Layout task trees using d3 hierarchy (for default positions)
  // Then override with stored positions if available
  for (const root of roots) {
    const h = hierarchy(root, (d) => d.children);

    const treeLayout = tree<TaskTree>()
      .nodeSize([config.nodeWidth + config.horizontalGap, config.nodeHeight + config.verticalGap])
      .separation(() => 1);

    treeLayout(h);

    let minX = Number.POSITIVE_INFINITY;
    let maxX = Number.NEGATIVE_INFINITY;

    h.each((node) => {
      // Use stored position if available, otherwise use computed
      const x = node.data.x ?? node.x! + offsetX;
      const y = node.data.y ?? node.y!;

      positions.set(node.data.id, { x, y });

      minX = Math.min(minX, x - config.nodeWidth / 2);
      maxX = Math.max(maxX, x + config.nodeWidth / 2);
      globalMinY = Math.min(globalMinY, y);
      globalMaxY = Math.max(globalMaxY, y + config.nodeHeight);
    });

    globalMinX = Math.min(globalMinX, minX);
    globalMaxX = Math.max(globalMaxX, maxX);

    offsetX = maxX + config.horizontalGap * 2;
  }

  // Layout evals - use stored positions if available, otherwise compute
  if (evalList.length > 0) {
    const defaultEvalRowY = globalMaxY + config.verticalGap * 1.5;

    // Sort evals by the x-position of their validated tasks (for default positioning)
    const evalsWithX = evalList.map((ev) => {
      // Use stored position if available
      if (ev.x != null && ev.y != null) {
        return { ev, x: ev.x, y: ev.y, hasStoredPos: true };
      }
      // Otherwise compute default position
      const validatedXs = ev.validates
        .map((taskId) => positions.get(taskId)?.x)
        .filter((x): x is number => x != null);
      const avgX =
        validatedXs.length > 0
          ? validatedXs.reduce((sum, x) => sum + x, 0) / validatedXs.length
          : 0;
      return { ev, x: avgX, y: defaultEvalRowY, hasStoredPos: false };
    });

    // Sort by x position (only affects default positioning order)
    evalsWithX.sort((a, b) => a.x - b.x);

    // Place evals
    const minSpacing = config.nodeWidth + config.horizontalGap;
    let lastX = Number.NEGATIVE_INFINITY;

    for (const item of evalsWithX) {
      let x: number;
      let y: number;

      if (item.hasStoredPos) {
        // Use stored position exactly
        x = item.x;
        y = item.y;
      } else {
        // Compute with minimum spacing
        x = Math.max(item.x, lastX + minSpacing);
        y = item.y;
        lastX = x;
      }

      positions.set(item.ev.id, { x, y });

      // Update bounds
      globalMinX = Math.min(globalMinX, x - config.nodeWidth / 2);
      globalMaxX = Math.max(globalMaxX, x + config.nodeWidth / 2);
      globalMaxY = Math.max(globalMaxY, y + config.nodeHeight);
    }
  }

  return {
    positions,
    bounds: {
      minX: globalMinX,
      minY: globalMinY,
      maxX: globalMaxX,
      maxY: globalMaxY,
      width: globalMaxX - globalMinX,
      height: globalMaxY - globalMinY,
    },
  };
}

/**
 * Generate SVG path for an edge from parent to child
 *
 * Uses a curved bezier path for smooth connections.
 * At lower LODs (smaller nodes), uses smaller offsets for tighter connections.
 */
export function generateEdgePath(
  parentPos: NodePosition,
  childPos: NodePosition,
  nodeHeight: number = LAYOUT_CONFIG.nodeHeight,
): string {
  // Use smaller offset for small nodes (dot/compact) for tighter connections
  // For larger nodes, use half height for edge-to-edge
  const offset = Math.min(nodeHeight / 2, 30);

  // Start from bottom center of parent
  const x1 = parentPos.x;
  const y1 = parentPos.y + offset;

  // End at top center of child
  const x2 = childPos.x;
  const y2 = childPos.y - offset;

  // Control points for smooth curve - adjust control point spread based on distance
  const verticalDist = y2 - y1;
  const controlSpread = Math.min(verticalDist * 0.4, 80);

  return `M ${x1} ${y1} C ${x1} ${y1 + controlSpread}, ${x2} ${y2 - controlSpread}, ${x2} ${y2}`;
}

// =============================================================================
// Counter-Scale Utilities for Semantic Zoom
// =============================================================================

/** Minimum screen size (px) for readability at any zoom level */
export const MIN_SCREEN_SIZE = {
  project: 60, // Project cards stay ~60px minimum
  task: 48, // Task/eval cards stay ~48px minimum
};

/** Base world dimensions for cards */
export const BASE_WORLD_SIZE = {
  project: 200, // ProjectCard full width
  task: 280, // TaskCard full width
};

/**
 * Calculate counter-scale to maintain minimum screen size
 *
 * When zoom makes a card smaller than min screen size, we scale it back up.
 * This ensures cards remain readable at any zoom level.
 */
export function getCounterScale(worldSize: number, minScreenSize: number, zoom: number): number {
  const screenSize = worldSize * zoom;
  if (screenSize >= minScreenSize) return 1;
  return minScreenSize / screenSize;
}

/**
 * Calculate bounding box for a set of positioned items
 */
export function getContentBounds<T>(
  items: T[],
  getPosition: (item: T, index: number) => { x: number; y: number },
  padding: { x: number; y: number } = { x: 100, y: 60 },
): { minX: number; minY: number; maxX: number; maxY: number; width: number; height: number } {
  if (items.length === 0) {
    return { minX: -100, minY: -100, maxX: 100, maxY: 100, width: 200, height: 200 };
  }

  let minX = Number.POSITIVE_INFINITY;
  let maxX = Number.NEGATIVE_INFINITY;
  let minY = Number.POSITIVE_INFINITY;
  let maxY = Number.NEGATIVE_INFINITY;
  items.forEach((item, i) => {
    const pos = getPosition(item, i);
    minX = Math.min(minX, pos.x - padding.x);
    maxX = Math.max(maxX, pos.x + padding.x);
    minY = Math.min(minY, pos.y - padding.y);
    maxY = Math.max(maxY, pos.y + padding.y);
  });

  return { minX, minY, maxX, maxY, width: maxX - minX, height: maxY - minY };
}

/**
 * Check if a node is visible in the viewport
 */
export function isNodeVisible(
  pos: NodePosition,
  viewport: { width: number; height: number },
  transform: { x: number; y: number; k: number },
): boolean {
  const { nodeWidth, nodeHeight } = LAYOUT_CONFIG;

  // Transform node position to screen coordinates
  const screenX = pos.x * transform.k + transform.x;
  const screenY = pos.y * transform.k + transform.y;
  const screenWidth = nodeWidth * transform.k;
  const screenHeight = nodeHeight * transform.k;

  // Check if any part of the node is visible
  return (
    screenX + screenWidth > 0 &&
    screenX < viewport.width &&
    screenY + screenHeight > 0 &&
    screenY < viewport.height
  );
}
