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
 * These control visual size, not layout position.
 */
export const RENDER_CONFIGS = {
  dot: {
    nodeWidth: 24,
    nodeHeight: 24,
  },
  compact: {
    nodeWidth: 140,
    nodeHeight: 48,
  },
  full: {
    nodeWidth: 280,
    nodeHeight: 180,
  },
} as const;

/** Level of detail based on zoom level */
export type LOADLevel = 'dot' | 'compact' | 'full';

/** Get LOAD level based on zoom factor */
export function getLOADLevel(zoom: number): LOADLevel {
  if (zoom < 0.3) return 'dot';
  if (zoom < 0.7) return 'compact';
  return 'full';
}

/** Get render config for current LOAD */
export function getRenderConfig(load: LOADLevel) {
  return RENDER_CONFIGS[load];
}

/**
 * Compute tree layout for a list of root nodes and evals
 *
 * Uses d3-hierarchy's tree layout algorithm to position nodes.
 * Always uses fixed LAYOUT_CONFIG so positions are stable across zoom levels.
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

  // Layout task trees
  for (const root of roots) {
    const h = hierarchy(root, (d) => d.children);

    const treeLayout = tree<TaskTree>()
      .nodeSize([config.nodeWidth + config.horizontalGap, config.nodeHeight + config.verticalGap])
      .separation(() => 1);

    treeLayout(h);

    let minX = Number.POSITIVE_INFINITY;
    let maxX = Number.NEGATIVE_INFINITY;

    h.each((node) => {
      const x = node.x! + offsetX;
      const y = node.y!;

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

  // Layout evals in a row below the tree
  if (evalList.length > 0) {
    const evalRowY = globalMaxY + config.verticalGap * 1.5;

    // Sort evals by the x-position of their first validated task
    const evalsWithX = evalList.map((ev) => {
      // Use stored position if available
      if (ev.x != null && ev.y != null) {
        return { ev, x: ev.x, y: ev.y, hasPosition: true };
      }
      // Otherwise, compute position based on validated tasks
      const validatedXs = ev.validates
        .map((taskId) => positions.get(taskId)?.x)
        .filter((x): x is number => x != null);
      const avgX =
        validatedXs.length > 0
          ? validatedXs.reduce((sum, x) => sum + x, 0) / validatedXs.length
          : 0;
      return { ev, x: avgX, y: evalRowY, hasPosition: false };
    });

    // Sort by x position
    evalsWithX.sort((a, b) => a.x - b.x);

    // Place evals with minimum spacing to avoid overlap
    const minSpacing = config.nodeWidth + config.horizontalGap;
    let lastX = Number.NEGATIVE_INFINITY;

    for (const item of evalsWithX) {
      if (item.hasPosition) {
        positions.set(item.ev.id, { x: item.x, y: item.y });
      } else {
        // Ensure minimum spacing from previous eval
        const x = Math.max(item.x, lastX + minSpacing);
        positions.set(item.ev.id, { x, y: evalRowY });
        lastX = x;

        // Update bounds
        globalMinX = Math.min(globalMinX, x - config.nodeWidth / 2);
        globalMaxX = Math.max(globalMaxX, x + config.nodeWidth / 2);
      }
    }

    globalMaxY = evalRowY + config.nodeHeight;
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
 * Always uses LAYOUT_CONFIG dimensions for consistent edge positioning.
 */
export function generateEdgePath(parentPos: NodePosition, childPos: NodePosition): string {
  const nodeHeight = LAYOUT_CONFIG.nodeHeight;

  // Start from bottom center of parent
  const x1 = parentPos.x;
  const y1 = parentPos.y + nodeHeight / 2;

  // End at top center of child
  const x2 = childPos.x;
  const y2 = childPos.y - nodeHeight / 2;

  // Control points for smooth curve
  const midY = (y1 + y2) / 2;

  return `M ${x1} ${y1} C ${x1} ${midY}, ${x2} ${midY}, ${x2} ${y2}`;
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
