/**
 * Tree layout hook using d3-hierarchy
 *
 * Computes node positions for a tree of BoardNodes using
 * d3-hierarchy's tree layout algorithm.
 */

import { hierarchy, tree } from 'd3-hierarchy';
import type { TaskTree } from '../../lib/types';

export interface LayoutConfig {
  /** Width of a node (varies by LOAD) */
  nodeWidth: number;
  /** Height of a node (varies by LOAD) */
  nodeHeight: number;
  /** Horizontal gap between nodes */
  horizontalGap: number;
  /** Vertical gap between levels */
  verticalGap: number;
}

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

/** Default layout configs for different LOAD levels */
export const LOAD_CONFIGS = {
  dot: {
    nodeWidth: 24,
    nodeHeight: 24,
    horizontalGap: 16,
    verticalGap: 32,
  },
  compact: {
    nodeWidth: 140,
    nodeHeight: 48,
    horizontalGap: 24,
    verticalGap: 48,
  },
  full: {
    nodeWidth: 280,
    nodeHeight: 180,
    horizontalGap: 40,
    verticalGap: 64,
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

/** Get layout config for a zoom level */
export function getLayoutConfigForZoom(zoom: number): LayoutConfig {
  return LOAD_CONFIGS[getLOADLevel(zoom)];
}

/**
 * Compute tree layout for a list of root nodes
 *
 * Uses d3-hierarchy's tree layout algorithm to position nodes.
 * Multiple root trees are laid out horizontally.
 */
export function computeTreeLayout(roots: TaskTree[], config: LayoutConfig): TreeLayoutResult {
  const positions = new Map<string, NodePosition>();

  if (roots.length === 0) {
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

  for (const root of roots) {
    // Build d3 hierarchy
    const h = hierarchy(root, (d) => d.children);

    // Configure tree layout
    const treeLayout = tree<TaskTree>()
      .nodeSize([config.nodeWidth + config.horizontalGap, config.nodeHeight + config.verticalGap])
      .separation(() => 1);

    // Compute layout
    treeLayout(h);

    // Get bounds for this tree
    let minX = Number.POSITIVE_INFINITY;
    let maxX = Number.NEGATIVE_INFINITY;

    h.each((node) => {
      // d3 tree layout puts x on horizontal axis, y on vertical
      // We want root at top, so swap and adjust
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

    // Offset for next tree (with gap between trees)
    offsetX = maxX + config.horizontalGap * 2;
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
 */
export function generateEdgePath(
  parentPos: NodePosition,
  childPos: NodePosition,
  nodeHeight: number,
): string {
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
  nodeWidth: number,
  nodeHeight: number,
  viewport: { x: number; y: number; width: number; height: number },
  transform: { x: number; y: number; k: number },
): boolean {
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
