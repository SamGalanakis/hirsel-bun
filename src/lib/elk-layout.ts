/**
 * ELK.js Layout Engine for SpecBoard
 *
 * Uses Eclipse Layout Kernel for hierarchical graph layout with orthogonal edge routing.
 *
 * Layout strategy:
 * - Use only tree structure (parent-child) edges for node positioning
 * - blockedBy and validates edges are visual overlays that don't affect layout
 * - This keeps the graph compact and hierarchical
 */

import ELK, {
  type ElkNode,
  type ElkExtendedEdge,
  type LayoutOptions,
} from 'elkjs/lib/elk.bundled.js';

const elk = new ELK();

// =============================================================================
// Types
// =============================================================================

export interface LayoutInputNode {
  id: string;
  name: string;
  kind: 'feature' | 'task' | 'check' | 'plan';
  isRoot: boolean;
  blockedBy: string[];
  validates: string[];
  resolves: string | null;
  children: LayoutInputNode[];
}

export interface LayoutNodePosition {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
  layer: number;
  isDummy: boolean;
}

export interface LayoutEdgeRoute {
  fromId: string;
  toId: string;
  edgeType: 'blockedBy' | 'validates' | 'hierarchy' | 'resolves';
  waypoints: [number, number][];
}

export interface ElkLayoutResult {
  positions: LayoutNodePosition[];
  edges: LayoutEdgeRoute[];
  width: number;
  height: number;
}

// =============================================================================
// Layout Constants
// =============================================================================

const CHAR_WIDTH = 5.8;
const TEXT_PADDING = 20;
const MIN_NODE_WIDTH = 60;
const MAX_NODE_WIDTH = 180;
const SINGLE_LINE_HEIGHT = 28;
const MULTI_LINE_HEIGHT = 40;
const MAX_LINES = 2;

// =============================================================================
// Layout Configuration
// =============================================================================

const LAYOUT_OPTIONS: LayoutOptions = {
  'elk.algorithm': 'layered',
  'elk.direction': 'DOWN',
  // Spacing - tighter horizontal, comfortable vertical
  'elk.spacing.nodeNode': '12',
  'elk.layered.spacing.nodeNodeBetweenLayers': '40',
  'elk.spacing.edgeNode': '12',
  'elk.spacing.edgeEdge': '6',
  // Edge routing
  'elk.edgeRouting': 'ORTHOGONAL',
  // More crossing minimization passes for cleaner layout
  'elk.layered.crossingMinimization.strategy': 'LAYER_SWEEP',
  'elk.layered.crossingMinimization.greedySwitch.type': 'TWO_SIDED',
  // Node placement
  'elk.layered.nodePlacement.strategy': 'BRANDES_KOEPF',
  'elk.layered.nodePlacement.bk.fixedAlignment': 'BALANCED',
  // Compaction for tighter layout
  'elk.layered.compaction.postCompaction.strategy': 'LEFT_RIGHT_CONSTRAINT_LOCKING',
  'elk.layered.compaction.connectedComponents': 'true',
};

// =============================================================================
// Helper Functions
// =============================================================================

function calculateNodeDimensions(name: string, isRoot = false): { width: number; height: number } {
  // Root nodes use slightly wider char width (11px italic font) and more padding
  const charWidth = isRoot ? 6.4 : CHAR_WIDTH;
  const padding = isRoot ? 28 : TEXT_PADDING;
  const minWidth = isRoot ? 80 : MIN_NODE_WIDTH;
  const maxWidth = isRoot ? 220 : MAX_NODE_WIDTH;
  const singleHeight = isRoot ? 34 : SINGLE_LINE_HEIGHT;
  const multiHeight = isRoot ? 48 : MULTI_LINE_HEIGHT;

  const idealWidth = name.length * charWidth + padding;

  if (idealWidth <= maxWidth) {
    return { width: Math.max(minWidth, idealWidth), height: singleHeight };
  }

  const charsPerLine = Math.floor((maxWidth - padding) / charWidth);
  const lines = Math.min(MAX_LINES, Math.ceil(name.length / charsPerLine));

  return {
    width: maxWidth,
    height: lines > 1 ? multiHeight : singleHeight,
  };
}

function flattenNodes(nodes: LayoutInputNode[]): LayoutInputNode[] {
  const result: LayoutInputNode[] = [];
  const flatten = (node: LayoutInputNode) => {
    result.push(node);
    for (const child of node.children) {
      flatten(child);
    }
  };
  for (const root of nodes) {
    flatten(root);
  }
  return result;
}

/** Build ELK graph with tree structure + validates edges for layout */
function buildElkGraph(nodes: LayoutInputNode[]): ElkNode {
  const allNodes = flattenNodes(nodes);
  const nodeIds = new Set(allNodes.map((n) => n.id));

  // Build ELK children
  const elkChildren: ElkNode[] = allNodes.map((node) => {
    const dims = calculateNodeDimensions(node.name, node.isRoot);
    return {
      id: node.id,
      width: dims.width,
      height: dims.height,
    };
  });

  const edges: ElkExtendedEdge[] = [];
  const edgeSet = new Set<string>();
  let edgeIndex = 0;

  // Add tree structure edges (parent -> child)
  const addTreeEdges = (parent: LayoutInputNode) => {
    for (const child of parent.children) {
      const key = `${parent.id}->${child.id}`;
      if (!edgeSet.has(key)) {
        edgeSet.add(key);
        edges.push({
          id: `e${edgeIndex++}`,
          sources: [parent.id],
          targets: [child.id],
          labels: [{ text: 'hierarchy' }],
        });
      }
      addTreeEdges(child);
    }
  };

  for (const root of nodes) {
    addTreeEdges(root);
  }

  // Add validates edges: node -> check (check depends on node completing)
  // This ensures checks are placed BELOW the nodes they validate
  for (const node of allNodes) {
    if (node.kind === 'check') {
      for (const validatedId of node.validates) {
        if (nodeIds.has(validatedId)) {
          const key = `${validatedId}->${node.id}`;
          if (!edgeSet.has(key)) {
            edgeSet.add(key);
            edges.push({
              id: `e${edgeIndex++}`,
              sources: [validatedId],
              targets: [node.id],
              labels: [{ text: 'validates' }],
            });
          }
        }
      }
    }
  }

  // Add blockedBy edges: blocker -> blocked
  for (const node of allNodes) {
    for (const blockerId of node.blockedBy) {
      if (nodeIds.has(blockerId)) {
        const key = `${blockerId}->${node.id}`;
        if (!edgeSet.has(key)) {
          edgeSet.add(key);
          edges.push({
            id: `e${edgeIndex++}`,
            sources: [blockerId],
            targets: [node.id],
            labels: [{ text: 'blockedBy' }],
          });
        }
      }
    }
  }

  // Add resolves edges: repair node -> eval it resolves
  for (const node of allNodes) {
    if (node.resolves && nodeIds.has(node.resolves)) {
      const key = `${node.id}->${node.resolves}`;
      if (!edgeSet.has(key)) {
        edgeSet.add(key);
        edges.push({
          id: `e${edgeIndex++}`,
          sources: [node.id],
          targets: [node.resolves],
          labels: [{ text: 'resolves' }],
        });
      }
    }
  }

  return {
    id: 'root',
    layoutOptions: LAYOUT_OPTIONS,
    children: elkChildren,
    edges,
  };
}

/** Generate orthogonal route between two nodes */
function generateOrthogonalRoute(
  source: LayoutNodePosition,
  target: LayoutNodePosition,
): [number, number][] {
  // Determine connection points based on relative position
  const sourceBelow = source.y > target.y + target.height;
  const sourceAbove = source.y + source.height < target.y;
  const sourceLeft = source.x + source.width < target.x;
  const sourceRight = source.x > target.x + target.width;

  let startX: number;
  let startY: number;
  let endX: number;
  let endY: number;

  if (sourceAbove) {
    // Source above target - connect bottom of source to top of target
    startX = source.x + source.width / 2;
    startY = source.y + source.height;
    endX = target.x + target.width / 2;
    endY = target.y;
  } else if (sourceBelow) {
    // Source below target - connect top of source to bottom of target
    startX = source.x + source.width / 2;
    startY = source.y;
    endX = target.x + target.width / 2;
    endY = target.y + target.height;
  } else if (sourceLeft) {
    // Source to the left - connect right side to left side
    startX = source.x + source.width;
    startY = source.y + source.height / 2;
    endX = target.x;
    endY = target.y + target.height / 2;
  } else if (sourceRight) {
    // Source to the right - connect left side to right side
    startX = source.x;
    startY = source.y + source.height / 2;
    endX = target.x + target.width;
    endY = target.y + target.height / 2;
  } else {
    // Overlapping - default to bottom/top
    startX = source.x + source.width / 2;
    startY = source.y + source.height;
    endX = target.x + target.width / 2;
    endY = target.y;
  }

  // Nearly straight line
  if (Math.abs(startX - endX) < 2 || Math.abs(startY - endY) < 2) {
    return [
      [startX, startY],
      [endX, endY],
    ];
  }

  // Create orthogonal path with midpoint bend
  const midY = (startY + endY) / 2;

  return [
    [startX, startY],
    [startX, midY],
    [endX, midY],
    [endX, endY],
  ];
}

/** Transform ELK result to our format */
function transformResult(elkResult: ElkNode): ElkLayoutResult {
  const positions: LayoutNodePosition[] = [];
  const edges: LayoutEdgeRoute[] = [];

  // Build position map from ELK result
  const positionMap = new Map<string, LayoutNodePosition>();

  if (elkResult.children) {
    for (const child of elkResult.children) {
      if (child.x !== undefined && child.y !== undefined) {
        const pos: LayoutNodePosition = {
          id: child.id,
          x: child.x,
          y: child.y,
          width: child.width || MIN_NODE_WIDTH,
          height: child.height || SINGLE_LINE_HEIGHT,
          layer: Math.round(child.y / 60),
          isDummy: false,
        };
        positions.push(pos);
        positionMap.set(child.id, pos);
      }
    }
  }

  // Extract edges with ELK-computed routing
  if (elkResult.edges) {
    for (const edge of elkResult.edges as ElkExtendedEdge[]) {
      const waypoints: [number, number][] = [];
      const sourceNode = positionMap.get(edge.sources[0]);
      const targetNode = positionMap.get(edge.targets[0]);

      if (sourceNode && targetNode && edge.sections && edge.sections.length > 0) {
        for (const section of edge.sections) {
          if (section.startPoint) {
            waypoints.push([section.startPoint.x, section.startPoint.y]);
          }
          if (section.bendPoints) {
            for (const bp of section.bendPoints) {
              waypoints.push([bp.x, bp.y]);
            }
          }
          if (section.endPoint) {
            waypoints.push([section.endPoint.x, section.endPoint.y]);
          }
        }
      } else if (sourceNode && targetNode) {
        // Fallback to simple orthogonal route
        const route = generateOrthogonalRoute(sourceNode, targetNode);
        waypoints.push(...route);
      }

      if (waypoints.length >= 2) {
        // Determine edge type from label
        const labelText = edge.labels?.[0]?.text;
        const edgeType: 'blockedBy' | 'validates' | 'hierarchy' | 'resolves' =
          labelText === 'validates'
            ? 'validates'
            : labelText === 'hierarchy'
              ? 'hierarchy'
              : labelText === 'resolves'
                ? 'resolves'
                : 'blockedBy';
        edges.push({
          fromId: edge.sources[0],
          toId: edge.targets[0],
          edgeType,
          waypoints,
        });
      }
    }
  }

  // Calculate dimensions
  let maxX = 0;
  let maxY = 0;
  for (const pos of positions) {
    maxX = Math.max(maxX, pos.x + pos.width);
    maxY = Math.max(maxY, pos.y + pos.height);
  }

  return {
    positions,
    edges,
    width: maxX + 24,
    height: maxY + 24,
  };
}

// =============================================================================
// Main Export
// =============================================================================

export async function computeElkLayout(nodes: LayoutInputNode[]): Promise<ElkLayoutResult> {
  if (nodes.length === 0) {
    return { positions: [], edges: [], width: 0, height: 0 };
  }

  const elkGraph = buildElkGraph(nodes);
  const result = await elk.layout(elkGraph);
  return transformResult(result);
}
