/**
 * SpecBoard - Canvas-based Delta Board
 *
 * Renders draft and live trees with:
 * - Compact auto-sized nodes
 * - Single tree before first dispatch, split view after
 * - Draft editable via context menu, live read-only
 */

import {
  type Component,
  For,
  Show,
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
} from 'solid-js';
import { useProject } from '../../stores';
import { useDelta } from '../../stores/delta-context';
import type {
  DraftNodeTree,
  LiveNodeTree,
  TreeDiff,
  NodeType,
  LiveNodeStatus,
} from '../../lib/types';

// =============================================================================
// Layout Constants (Horizontal Tree: root left → children right)
// =============================================================================

const MIN_NODE_WIDTH = 80;
const MAX_NODE_WIDTH = 120;
const NODE_HEIGHT = 26;
const NODE_LINE_HEIGHT = 12;
const SIBLING_GAP = 4;
const LEVEL_GAP = 12;
const LAYER_OFFSET = 50;     // Compact horizontal offset between layers
const TREE_PADDING = 12;
const PROJECT_BAR_WIDTH = 24;  // Narrow vertical bar for project node
const DIVIDER_WIDTH = 40;
const CHAR_WIDTH = 5.5;
const MAX_CHARS_PER_LINE = 16;

interface NodePosition {
  x: number;
  y: number;
  width: number;
  height: number;
  lines: string[]; // Wrapped text lines
}

// =============================================================================
// Helper: Calculate node dimensions with text wrapping
// =============================================================================

function calcNodeDimensions(name: string): { width: number; height: number; lines: string[] } {
  const padding = 20; // dot (6px) + gaps + px padding

  // If name fits on one line, use single line
  if (name.length <= MAX_CHARS_PER_LINE) {
    const textWidth = name.length * CHAR_WIDTH;
    return {
      width: Math.max(MIN_NODE_WIDTH, Math.min(MAX_NODE_WIDTH, textWidth + padding)),
      height: NODE_HEIGHT,
      lines: [name],
    };
  }

  // Wrap text into multiple lines
  const words = name.split(/\s+/);
  const lines: string[] = [];
  let currentLine = '';

  for (const word of words) {
    const testLine = currentLine ? `${currentLine} ${word}` : word;
    if (testLine.length <= MAX_CHARS_PER_LINE) {
      currentLine = testLine;
    } else {
      if (currentLine) lines.push(currentLine);
      // If a single word is too long, truncate it
      currentLine = word.length > MAX_CHARS_PER_LINE ? word.slice(0, MAX_CHARS_PER_LINE - 1) + '…' : word;
    }
  }
  if (currentLine) lines.push(currentLine);

  // Cap at 2 lines max
  if (lines.length > 2) {
    lines.length = 2;
    lines[1] = lines[1].slice(0, -1) + '…';
  }

  const maxLineLength = Math.max(...lines.map(l => l.length));
  const textWidth = maxLineLength * CHAR_WIDTH;
  const height = NODE_HEIGHT + (lines.length - 1) * NODE_LINE_HEIGHT;

  return {
    width: Math.max(MIN_NODE_WIDTH, Math.min(MAX_NODE_WIDTH, textWidth + padding)),
    height,
    lines,
  };
}

// =============================================================================
// Column-Based Dependency Layout Algorithm
//
// X position = dependency depth (steps from a node with no blockers)
// Y position = stacked vertically within each column
// Lines = explicit blocked_by arrows flowing left-to-right
// =============================================================================

interface LayoutNode {
  id: string;
  name: string;
  nodeType?: NodeType;
  x: number;
  y: number;
  width: number;
  height: number;
  lines: string[];
  validates?: string[];
  blockedBy?: string[];
  parentId?: string; // For grouping related nodes
}

/** Dependency relationship for drawing connector lines */
interface DependencyRelationship {
  from: string;    // blocker node ID
  to: string;      // blocked node ID
  type: 'blockedBy' | 'validates' | 'project';
}

interface LayoutTreeResult {
  positions: Map<string, NodePosition>;
  width: number;
  height: number;
  evalsWithValidates: { id: string; validates: string[] }[];
  restructuredTree: { id: string; nodeType?: NodeType; children: any[] }[];
  crossTreeRelationships: DependencyRelationship[];
}

function layoutTree<T extends { id: string; name: string; children: T[]; nodeType?: NodeType; validates?: string[]; blockedBy?: string[] }>(
  roots: T[],
  _startX: number = 0
): LayoutTreeResult {
  const positions = new Map<string, NodePosition>();
  const evalsWithValidates: { id: string; validates: string[] }[] = [];
  const dependencyRelationships: DependencyRelationship[] = [];

  if (roots.length === 0) {
    return { positions, width: MIN_NODE_WIDTH, height: NODE_HEIGHT, evalsWithValidates, restructuredTree: [], crossTreeRelationships: [] };
  }

  // Flatten all nodes and build lookup maps
  const allNodes: LayoutNode[] = [];
  const nodeById = new Map<string, LayoutNode>();
  const childrenByParent = new Map<string, string[]>();

  function flattenNodes(node: T, parentId?: string) {
    const dims = calcNodeDimensions(node.name);
    // Project nodes are narrow vertical bars - height will be computed later
    const isProjectNode = node.nodeType === 'project';
    const layoutNode: LayoutNode = {
      id: node.id,
      name: node.name,
      nodeType: node.nodeType,
      x: 0,
      y: 0,
      width: isProjectNode ? PROJECT_BAR_WIDTH : dims.width,
      height: isProjectNode ? NODE_HEIGHT : dims.height,  // Placeholder, will be updated
      lines: dims.lines,
      validates: node.validates,
      blockedBy: node.blockedBy,
      parentId,
    };
    allNodes.push(layoutNode);
    nodeById.set(node.id, layoutNode);

    // Track children for grouping
    if (parentId) {
      const siblings = childrenByParent.get(parentId) || [];
      siblings.push(node.id);
      childrenByParent.set(parentId, siblings);
    }

    // Collect evals with validates
    if (node.nodeType === 'eval' && node.validates && node.validates.length > 0) {
      evalsWithValidates.push({ id: node.id, validates: node.validates });
    }

    for (const child of node.children) {
      flattenNodes(child, node.id);
    }
  }

  for (const root of roots) {
    flattenNodes(root);
  }

  const allNodeIds = new Set(allNodes.map(n => n.id));

  // Find project nodes and other evals for final gate positioning
  const projectNodes = allNodes.filter(n => n.nodeType === 'project');
  const otherEvals = allNodes.filter(n => n.nodeType === 'eval' && n.validates && n.validates.length > 0);

  // Compute effective blockers for DEPTH CALCULATION (positioning)
  // This determines which column a node goes in
  // Note: Evals use blockers of their validated tasks (same column as what they validate)
  function getDepthBlockers(node: LayoutNode): string[] {
    const blockers: string[] = [];

    if (node.nodeType === 'eval') {
      if (node.validates && node.validates.length > 0) {
        // Eval should be in same column as tasks it validates
        // So inherit the blockers OF those tasks (not the tasks themselves)
        for (const taskId of node.validates) {
          const task = nodeById.get(taskId);
          if (task && task.blockedBy) {
            for (const blockerId of task.blockedBy) {
              if (allNodeIds.has(blockerId) && !blockers.includes(blockerId)) {
                blockers.push(blockerId);
              }
            }
          }
        }
      }
      // Empty validates = final gate, no blockers = column 1 (like parentless tasks)
    }

    // For tasks (and evals with explicit blockedBy), use blocked_by
    if (node.blockedBy) {
      for (const blockerId of node.blockedBy) {
        if (allNodeIds.has(blockerId) && !blockers.includes(blockerId)) {
          blockers.push(blockerId);
        }
      }
    }

    return blockers;
  }

  // Compute effective blockers for LINE DRAWING (visual connections)
  // This determines which lines are drawn
  function getLineBlockers(node: LayoutNode): string[] {
    const blockers: string[] = [];

    // Skip project nodes - they don't connect to anything
    if (node.nodeType === 'project') {
      return blockers;
    }

    if (node.nodeType === 'eval') {
      if (node.validates && node.validates.length > 0) {
        // Eval validates specific tasks - show lines to those tasks
        for (const taskId of node.validates) {
          if (allNodeIds.has(taskId)) {
            blockers.push(taskId);
          }
        }
      } else if (projectNodes.length > 0) {
        // Empty validates = final gate, connects to project (like parentless tasks)
        blockers.push(projectNodes[0].id);
      }
    }

    // For tasks (and evals with explicit blockedBy), use blocked_by
    if (node.blockedBy) {
      for (const blockerId of node.blockedBy) {
        if (allNodeIds.has(blockerId) && !blockers.includes(blockerId)) {
          blockers.push(blockerId);
        }
      }
    }

    // If node has no blockers, connect to project (visual anchor)
    if (blockers.length === 0 && projectNodes.length > 0) {
      blockers.push(projectNodes[0].id);
    }

    return blockers;
  }

  // Compute dependency depth for each node using Kahn's algorithm
  const depthByNode = new Map<string, number>();
  const inDegree = new Map<string, number>();
  const dependents = new Map<string, string[]>();

  // Initialize
  for (const node of allNodes) {
    inDegree.set(node.id, 0);
    dependents.set(node.id, []);
  }

  // Build graph
  for (const node of allNodes) {
    const blockers = getDepthBlockers(node);
    inDegree.set(node.id, blockers.length);
    for (const blockerId of blockers) {
      dependents.get(blockerId)?.push(node.id);
    }
  }

  // BFS to assign depths
  const queue: string[] = [];
  for (const node of allNodes) {
    if ((inDegree.get(node.id) || 0) === 0) {
      queue.push(node.id);
      depthByNode.set(node.id, 0);
    }
  }

  while (queue.length > 0) {
    const nodeId = queue.shift()!;
    const currentDepth = depthByNode.get(nodeId) || 0;

    for (const depId of dependents.get(nodeId) || []) {
      const newInDegree = (inDegree.get(depId) || 1) - 1;
      inDegree.set(depId, newInDegree);

      // Update depth to be max of all blockers + 1
      const existingDepth = depthByNode.get(depId);
      const newDepth = currentDepth + 1;
      if (existingDepth === undefined || newDepth > existingDepth) {
        depthByNode.set(depId, newDepth);
      }

      if (newInDegree === 0) {
        queue.push(depId);
      }
    }
  }

  // Handle any cycles (nodes not processed yet) - put them at depth 0
  for (const node of allNodes) {
    if (!depthByNode.has(node.id)) {
      depthByNode.set(node.id, 0);
    }
  }

  // Shift all non-project nodes right by 1 so project has its own column
  for (const node of allNodes) {
    if (node.nodeType !== 'project') {
      const currentDepth = depthByNode.get(node.id) || 0;
      depthByNode.set(node.id, currentDepth + 1);
    }
  }

  // Group nodes by depth (column)
  const nodesByColumn = new Map<number, LayoutNode[]>();
  for (const node of allNodes) {
    const depth = depthByNode.get(node.id) || 0;
    const column = nodesByColumn.get(depth) || [];
    column.push(node);
    nodesByColumn.set(depth, column);
  }

  // =========================================================================
  // Barycenter Y-positioning to minimize line crossings
  // Column 0: simple stack. Columns 1+: position at average Y of dependencies.
  // =========================================================================

  const maxDepth = Math.max(...Array.from(nodesByColumn.keys()), 0);

  // First pass: assign X positions
  for (const node of allNodes) {
    const depth = depthByNode.get(node.id) || 0;
    node.x = TREE_PADDING + depth * LAYER_OFFSET;
  }

  // Helper: check if two nodes overlap in 2D
  const nodesOverlap = (a: LayoutNode, b: LayoutNode): boolean => {
    const xOverlap = a.x < b.x + b.width && a.x + a.width > b.x;
    const yOverlap = a.y < b.y + b.height && a.y + a.height > b.y;
    return xOverlap && yOverlap;
  };

  // Helper: find Y where node doesn't overlap, trying both up and down from target
  const findNonOverlappingY = (node: LayoutNode, targetY: number, positionedNodes: LayoutNode[]): number => {
    // Find nodes that could overlap horizontally
    const horizontallyOverlapping = positionedNodes.filter(other =>
      node.x < other.x + other.width && node.x + node.width > other.x
    );

    if (horizontallyOverlapping.length === 0) {
      return Math.max(TREE_PADDING, targetY);
    }

    // Sort by Y
    horizontallyOverlapping.sort((a, b) => a.y - b.y);

    // Try target Y first
    node.y = Math.max(TREE_PADDING, targetY);
    let hasOverlap = horizontallyOverlapping.some(other => nodesOverlap(node, other));
    if (!hasOverlap) return node.y;

    // Find gaps and try to fit - alternate between going up and down
    const gaps: { start: number; end: number }[] = [];

    // Gap before first node
    if (horizontallyOverlapping[0].y > TREE_PADDING + node.height + SIBLING_GAP) {
      gaps.push({ start: TREE_PADDING, end: horizontallyOverlapping[0].y - SIBLING_GAP });
    }

    // Gaps between nodes
    for (let i = 0; i < horizontallyOverlapping.length - 1; i++) {
      const gapStart = horizontallyOverlapping[i].y + horizontallyOverlapping[i].height + SIBLING_GAP;
      const gapEnd = horizontallyOverlapping[i + 1].y - SIBLING_GAP;
      if (gapEnd - gapStart >= node.height) {
        gaps.push({ start: gapStart, end: gapEnd });
      }
    }

    // Gap after last node (infinite)
    const lastNode = horizontallyOverlapping[horizontallyOverlapping.length - 1];
    gaps.push({ start: lastNode.y + lastNode.height + SIBLING_GAP, end: Infinity });

    // Find best gap (closest to target Y)
    let bestY = gaps[gaps.length - 1].start; // Default to after last node
    let bestDistance = Math.abs(bestY - targetY);

    for (const gap of gaps) {
      // Try fitting at start of gap
      const fitY = Math.max(gap.start, Math.min(targetY, gap.end - node.height));
      if (fitY >= gap.start && fitY + node.height <= gap.end) {
        const distance = Math.abs(fitY - targetY);
        if (distance < bestDistance) {
          bestDistance = distance;
          bestY = fitY;
        }
      }
    }

    return Math.max(TREE_PADDING, bestY);
  };

  // Track positioned nodes for collision detection
  const positionedNodes: LayoutNode[] = [];

  // Column 0: project node
  const col0 = nodesByColumn.get(0) || [];
  const projectNode = col0.find(n => n.nodeType === 'project');

  // Position columns 1+ with 2D collision detection
  for (let depth = 1; depth <= maxDepth; depth++) {
    const nodes = nodesByColumn.get(depth) || [];

    // Calculate target Y based on DEPTH blockers (barycenter positioning)
    // Use getDepthBlockers for positioning (includes final gate's eval dependencies)
    // Use getLineBlockers only for actual line drawing
    const targetYs: { node: LayoutNode; targetY: number }[] = [];
    for (const node of nodes) {
      const blockers = getDepthBlockers(node);
      if (blockers.length > 0) {
        let sumY = 0;
        let count = 0;
        for (const blockerId of blockers) {
          const blocker = nodeById.get(blockerId);
          if (blocker && blocker.y !== undefined) {
            sumY += blocker.y + blocker.height / 2;
            count++;
          }
        }
        const avgY = count > 0 ? sumY / count - node.height / 2 : TREE_PADDING;
        targetYs.push({ node, targetY: avgY });
      } else {
        targetYs.push({ node, targetY: TREE_PADDING });
      }
    }

    // Sort by target Y
    targetYs.sort((a, b) => a.targetY - b.targetY);

    // Position each node, avoiding overlaps with all previously positioned nodes
    for (const { node, targetY } of targetYs) {
      node.y = findNonOverlappingY(node, targetY, positionedNodes);
      positionedNodes.push(node);
    }
  }

  // Make project node a vertical bar spanning all column 1 nodes
  const col1 = nodesByColumn.get(1) || [];
  if (projectNode) {
    if (col1.length > 0) {
      const sortedCol1 = [...col1].sort((a, b) => a.y - b.y);
      const firstY = sortedCol1[0].y;
      const lastNode = sortedCol1[sortedCol1.length - 1];
      const lastY = lastNode.y + lastNode.height;
      // Project bar spans from first to last node in column 1
      projectNode.y = firstY;
      projectNode.height = lastY - firstY;
    } else {
      projectNode.y = TREE_PADDING;
      projectNode.height = NODE_HEIGHT;
    }
  }

  // Store final positions
  for (const node of allNodes) {
    positions.set(node.id, {
      x: node.x,
      y: node.y,
      width: node.width,
      height: node.height,
      lines: node.lines,
    });
  }

  // Collect dependency relationships for drawing lines
  const projectId = projectNodes.length > 0 ? projectNodes[0].id : null;
  for (const node of allNodes) {
    const blockers = getLineBlockers(node);
    for (const blockerId of blockers) {
      // Determine relationship type
      const isProjectConnection = blockerId === projectId;
      const isValidates = node.nodeType === 'eval' && node.validates?.includes(blockerId);
      // Final gate eval (empty validates) connecting to project = validates style
      const isFinalGateToProject = node.nodeType === 'eval' && isProjectConnection &&
        (!node.validates || node.validates.length === 0);
      const isBlockedBy = !isProjectConnection && !isValidates && node.blockedBy?.includes(blockerId);

      let type: 'validates' | 'blockedBy' | 'project';
      if (isValidates || isFinalGateToProject) {
        type = 'validates';
      } else if (isBlockedBy) {
        type = 'blockedBy';
      } else {
        type = 'project'; // Default for project connections (tasks)
      }

      dependencyRelationships.push({
        from: blockerId,
        to: node.id,
        type,
      });
    }
  }

  // Compute bounds
  const allPositions = Array.from(positions.values());
  if (allPositions.length === 0) {
    return { positions, width: MIN_NODE_WIDTH, height: NODE_HEIGHT, evalsWithValidates, restructuredTree: [], crossTreeRelationships: [] };
  }

  const maxX = Math.max(...allPositions.map(p => p.x + p.width));
  const maxY = Math.max(...allPositions.map(p => p.y + p.height));

  // Build minimal restructured tree for compatibility (just root nodes, no children needed for column layout)
  const restructuredTree = roots.map(r => ({
    id: r.id,
    nodeType: r.nodeType,
    children: [],
  }));

  return {
    positions,
    width: maxX + TREE_PADDING,
    height: maxY + TREE_PADDING,
    evalsWithValidates,
    restructuredTree,
    crossTreeRelationships: dependencyRelationships,
  };
}

// =============================================================================
// Node Card Components
// =============================================================================

const DraftNodeCard: Component<{
  node: DraftNodeTree;
  position: NodePosition;
  diff: TreeDiff | null;
  showDelta: boolean;
  selected: boolean;
  onSelect: () => void;
  onDoubleClick: () => void;
  onContextMenu: (e: MouseEvent) => void;
}> = (props) => {
  const isNew = () =>
    props.showDelta && props.diff?.newNodes.some((n) => n.id === props.node.id);
  const isModified = () =>
    props.showDelta && props.diff?.modifiedNodes.some((m) => m.draftNode.id === props.node.id);
  const isEval = () => props.node.nodeType === 'eval';
  const isProject = () => props.node.nodeType === 'project';
  const isMultiLine = () => props.position.lines.length > 1;

  // ==========================================================================
  // Visual Hierarchy - Distinguished by SHAPE and BORDER only
  // NO color for delta status - colors reserved for live status only
  // ==========================================================================

  // PROJECT: The shepherd's lantern - amber tint, prominent border, rounded
  const projectStyles = () => ({
    bg: 'linear-gradient(135deg, rgba(212, 165, 116, 0.12) 0%, rgba(36, 36, 36, 0.95) 100%)',
    border: props.selected ? 'rgba(212, 165, 116, 0.7)' : 'rgba(212, 165, 116, 0.4)',
    borderWidth: '2px',
    borderStyle: 'solid',
    textColor: 'var(--wool-100)',
    radius: '8px',
  });

  // EVAL: The gate/checkpoint - DASHED border, dark sage green tint
  const evalStyles = () => ({
    bg: 'rgba(42, 45, 40, 0.9)',  // Very subtle green tint
    border: props.selected ? 'rgba(92, 120, 82, 0.6)' : 'rgba(70, 90, 65, 0.5)',  // sage-dark tones
    borderWidth: '1px',
    borderStyle: 'dashed',
    textColor: 'var(--wool-300)',
    radius: '5px',
  });

  // TASK: The sheep - neutral, solid border, standard rounded
  const taskStyles = () => ({
    bg: 'rgba(42, 40, 38, 0.85)',
    border: props.selected ? 'rgba(140, 135, 130, 0.5)' : 'rgba(80, 76, 72, 0.45)',
    borderWidth: '1px',
    borderStyle: 'solid',  // Solid = work task
    textColor: 'var(--wool-300)',
    radius: '5px',
  });

  const styles = () => isProject() ? projectStyles() : isEval() ? evalStyles() : taskStyles();

  // Project nodes render as vertical bars
  if (isProject()) {
    return (
      <div
        class={`absolute cursor-pointer group ${props.selected ? 'z-10' : ''}`}
        style={{
          left: `${props.position.x}px`,
          top: `${props.position.y}px`,
          width: `${props.position.width}px`,
          height: `${props.position.height}px`,
        }}
        onClick={() => props.onSelect()}
        onDblClick={() => props.onDoubleClick()}
        onContextMenu={(e) => props.onContextMenu(e)}
      >
        <div
          class="h-full w-full flex items-center justify-center relative"
          style={{
            background: styles().bg,
            border: `${styles().borderWidth} ${styles().borderStyle} ${styles().border}`,
            'border-radius': styles().radius,
            'box-shadow': props.selected ? '0 2px 8px rgba(0,0,0,0.3)' : undefined,
          }}
        >
          {/* Vertical text centered in bar */}
          <span
            class="text-[9px] font-semibold whitespace-nowrap"
            style={{
              color: styles().textColor,
              'writing-mode': 'vertical-rl',
              'text-orientation': 'mixed',
              transform: 'rotate(180deg)',
              'max-height': `${props.position.height - 8}px`,
              overflow: 'hidden',
              'text-overflow': 'ellipsis',
            }}
          >
            {props.node.name}
          </span>
        </div>
      </div>
    );
  }

  return (
    <div
      class={`absolute cursor-pointer group ${props.selected ? 'z-10' : ''}`}
      style={{
        left: `${props.position.x}px`,
        top: `${props.position.y}px`,
        width: `${props.position.width}px`,
        height: `${props.position.height}px`,
      }}
      onClick={() => props.onSelect()}
      onDblClick={() => props.onDoubleClick()}
      onContextMenu={(e) => props.onContextMenu(e)}
    >
      <div
        class={`h-full flex items-center gap-1.5 px-2 relative ${isMultiLine() ? 'flex-col justify-center !items-start py-1' : ''}`}
        style={{
          background: styles().bg,
          border: `${styles().borderWidth} ${styles().borderStyle} ${styles().border}`,
          'border-radius': styles().radius,
          'box-shadow': props.selected ? '0 2px 8px rgba(0,0,0,0.3)' : undefined,
        }}
      >
        {/* Name text - no icons, just text */}
        <div class={`flex-1 min-w-0 ${isMultiLine() ? 'flex flex-col gap-0.5' : ''}`}>
          <For each={props.position.lines}>
            {(line) => (
              <span
                class="text-[10px] truncate leading-tight block font-medium"
                style={{ color: styles().textColor }}
              >
                {line}
              </span>
            )}
          </For>
        </div>
      </div>
    </div>
  );
};

// Helper to recursively check if a node tree is completely done
const isTreeDone = (node: LiveNodeTree): boolean => {
  if (node.children.length > 0) {
    return node.children.every(child => isTreeDone(child));
  }
  return node.status === 'done';
};

const LiveNodeCard: Component<{
  node: LiveNodeTree;
  position: NodePosition;
  diff: TreeDiff | null;
  showDelta: boolean;
  selected: boolean;
  onSelect: () => void;
}> = (props) => {
  const isDeleted = () =>
    props.showDelta && props.diff?.deletedNodes.some((n) => n.id === props.node.id);
  const isEval = () => props.node.nodeType === 'eval';
  const isProject = () => props.node.nodeType === 'project';
  const isMultiLine = () => props.position.lines.length > 1;
  const isWorking = () => props.node.status === 'working';
  // Project nodes compute status from children for robustness
  const isDone = () => {
    if (isProject() && props.node.children.length > 0) {
      return props.node.children.every(child => isTreeDone(child));
    }
    return props.node.status === 'done';
  };
  const isFailed = () => props.node.status === 'failed';

  // ==========================================================================
  // Visual Hierarchy - Identical to DraftNodeCard
  // Status shown via subtle external glow only (no internal changes)
  // ==========================================================================

  // PROJECT: The shepherd's lantern - amber tint, prominent border, rounded
  const projectStyles = () => ({
    bg: 'linear-gradient(135deg, rgba(212, 165, 116, 0.12) 0%, rgba(36, 36, 36, 0.95) 100%)',
    border: props.selected ? 'rgba(212, 165, 116, 0.7)' : 'rgba(212, 165, 116, 0.4)',
    borderWidth: '2px',
    borderStyle: 'solid',
    textColor: 'var(--wool-100)',
    radius: '8px',
  });

  // EVAL: The gate/checkpoint - DASHED border, dark sage green tint
  const evalStyles = () => ({
    bg: 'rgba(42, 45, 40, 0.9)',  // Very subtle green tint
    border: props.selected ? 'rgba(92, 120, 82, 0.6)' : 'rgba(70, 90, 65, 0.5)',  // sage-dark tones
    borderWidth: '1px',
    borderStyle: 'dashed',
    textColor: 'var(--wool-300)',
    radius: '5px',
  });

  // TASK: The sheep - neutral, solid border, standard rounded
  const taskStyles = () => ({
    bg: 'rgba(42, 40, 38, 0.85)',
    border: props.selected ? 'rgba(140, 135, 130, 0.5)' : 'rgba(80, 76, 72, 0.45)',
    borderWidth: '1px',
    borderStyle: 'solid',  // Solid = work task
    textColor: 'var(--wool-300)',
    radius: '5px',
  });

  const styles = () => isProject() ? projectStyles() : isEval() ? evalStyles() : taskStyles();

  // Status glow - external indicator that doesn't affect card dimensions
  const statusGlow = () => {
    if (isWorking()) return '0 0 12px rgba(212, 165, 116, 0.4)';  // amber glow
    if (isDone()) return '0 0 8px rgba(125, 153, 112, 0.25)';     // subtle sage
    if (isFailed()) return '0 0 10px rgba(196, 92, 74, 0.35)';    // terra
    return undefined;  // pending = no glow
  };

  // Combined shadow: selection shadow + status glow
  const combinedShadow = () => {
    const shadows: string[] = [];
    if (props.selected) shadows.push('0 2px 8px rgba(0,0,0,0.3)');
    const glow = statusGlow();
    if (glow) shadows.push(glow);
    return shadows.length > 0 ? shadows.join(', ') : undefined;
  };

  // Project nodes render as vertical bars
  if (isProject()) {
    return (
      <div
        class={`absolute cursor-pointer ${props.selected ? 'z-10' : ''}`}
        style={{
          left: `${props.position.x}px`,
          top: `${props.position.y}px`,
          width: `${props.position.width}px`,
          height: `${props.position.height}px`,
        }}
        onClick={() => props.onSelect()}
      >
        <div
          class={`h-full w-full flex items-center justify-center relative ${isDeleted() ? 'opacity-40' : ''}`}
          style={{
            background: styles().bg,
            border: `${styles().borderWidth} ${styles().borderStyle} ${isDeleted() ? 'rgba(196, 92, 74, 0.4)' : styles().border}`,
            'border-radius': styles().radius,
            'box-shadow': combinedShadow(),
          }}
        >
          {/* Vertical text centered in bar */}
          <span
            class="text-[9px] font-semibold whitespace-nowrap"
            style={{
              color: styles().textColor,
              'writing-mode': 'vertical-rl',
              'text-orientation': 'mixed',
              transform: 'rotate(180deg)',
              'max-height': `${props.position.height - 8}px`,
              overflow: 'hidden',
              'text-overflow': 'ellipsis',
            }}
          >
            {props.node.name}
          </span>
        </div>

        {/* Status corner badge */}
        <Show when={isDone() || isFailed() || isWorking()}>
          <div
            class="absolute -top-1 -right-1 flex items-center justify-center rounded-full"
            style={{
              width: '14px',
              height: '14px',
              background: isDone() ? 'var(--sage)' : isFailed() ? 'var(--terra)' : 'var(--amber-500)',
              'box-shadow': '0 1px 3px rgba(0,0,0,0.3)',
            }}
          >
            <Show when={isDone()}>
              <svg class="w-2.5 h-2.5 text-pasture-900" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
                <path d="M20 6 9 17l-5-5" />
              </svg>
            </Show>
            <Show when={isFailed()}>
              <svg class="w-2.5 h-2.5 text-pasture-900" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
                <path d="M18 6 6 18M6 6l12 12" />
              </svg>
            </Show>
            <Show when={isWorking()}>
              <svg class="w-2.5 h-2.5 text-pasture-900 animate-spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
                <path d="M21 12a9 9 0 1 1-6.219-8.56" />
              </svg>
            </Show>
          </div>
        </Show>
      </div>
    );
  }

  return (
    <div
      class={`absolute cursor-pointer ${props.selected ? 'z-10' : ''}`}
      style={{
        left: `${props.position.x}px`,
        top: `${props.position.y}px`,
        width: `${props.position.width}px`,
        height: `${props.position.height}px`,
      }}
      onClick={() => props.onSelect()}
    >
      <div
        class={`h-full flex items-center gap-1.5 px-2 relative ${isDeleted() ? 'opacity-40' : ''} ${isMultiLine() ? 'flex-col justify-center !items-start py-1' : ''}`}
        style={{
          background: styles().bg,
          border: `${styles().borderWidth} ${styles().borderStyle} ${isDeleted() ? 'rgba(196, 92, 74, 0.4)' : styles().border}`,
          'border-radius': styles().radius,
          'box-shadow': combinedShadow(),
        }}
      >
        {/* Name text - no icons, just text */}
        <div class={`flex-1 min-w-0 ${isMultiLine() ? 'flex flex-col gap-0.5' : ''}`}>
          <For each={props.position.lines}>
            {(line) => (
              <span
                class="text-[10px] truncate leading-tight block font-medium"
                style={{ color: styles().textColor }}
              >
                {line}
              </span>
            )}
          </For>
        </div>
      </div>

      {/* Status corner badge */}
      <Show when={isDone() || isFailed() || isWorking()}>
        <div
          class="absolute -top-1 -right-1 flex items-center justify-center rounded-full"
          style={{
            width: '14px',
            height: '14px',
            background: isDone() ? 'var(--sage)' : isFailed() ? 'var(--terra)' : 'var(--amber-500)',
            'box-shadow': '0 1px 3px rgba(0,0,0,0.3)',
          }}
        >
          <Show when={isDone()}>
            <svg class="w-2.5 h-2.5 text-pasture-900" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
              <path d="M20 6 9 17l-5-5" />
            </svg>
          </Show>
          <Show when={isFailed()}>
            <svg class="w-2.5 h-2.5 text-pasture-900" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </Show>
          <Show when={isWorking()}>
            <svg class="w-2.5 h-2.5 text-pasture-900 animate-spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
              <path d="M21 12a9 9 0 1 1-6.219-8.56" />
            </svg>
          </Show>
        </div>
      </Show>

      {/* Deleted indicator */}
      <Show when={isDeleted()}>
        <div
          class="absolute left-0 top-1 bottom-1 w-0.5 rounded-full"
          style={{ background: 'var(--terra)' }}
        />
      </Show>
    </div>
  );
};

// =============================================================================
// Dependency Connectors (Orthogonal Routing)
// =============================================================================

const DependencyConnectors: Component<{
  positions: Map<string, NodePosition>;
  relationships: DependencyRelationship[];
}> = (props) => {
  const paths = createMemo(() => {
    const result: { from: NodePosition; to: NodePosition; type: 'validates' | 'blockedBy' | 'project' }[] = [];

    for (const rel of props.relationships) {
      const fromPos = props.positions.get(rel.from);
      const toPos = props.positions.get(rel.to);
      if (fromPos && toPos) {
        result.push({ from: fromPos, to: toPos, type: rel.type });
      }
    }

    return result;
  });

  return (
    <svg class="absolute inset-0 pointer-events-none overflow-visible" style={{ 'z-index': 0 }}>
      <For each={paths()}>
        {(edge) => {
          // Connect center-right of source to center-left of target
          const x1 = edge.from.x + edge.from.width;
          const x2 = edge.to.x;
          const y2 = edge.to.y + edge.to.height / 2;

          // For project connections, y1 should match y2 so line is horizontal
          // (project bar spans full height, so we connect at target's Y level)
          const isFromProjectBar = edge.type === 'project';
          const y1 = isFromProjectBar ? y2 : edge.from.y + edge.from.height / 2;

          // Orthogonal routing: right-angle lines
          // Route: horizontal at source Y level → vertical in gap before target → horizontal to target
          // This avoids crossing through nodes in intermediate columns
          const turnX = x2 - LEVEL_GAP / 2; // Turn point in the gap before target

          let pathD: string;
          if (Math.abs(y2 - y1) < 2) {
            // Same Y level - straight horizontal line
            pathD = `M ${x1} ${y1} L ${x2} ${y2}`;
          } else {
            // Different Y levels - go horizontal first, then vertical near target
            pathD = `M ${x1} ${y1} L ${turnX} ${y1} L ${turnX} ${y2} L ${x2} ${y2}`;
          }

          // Styling based on relationship type
          const isValidates = edge.type === 'validates';
          const isBlockedBy = edge.type === 'blockedBy';

          // Validates: sage green solid (eval checking task)
          // BlockedBy: terra/red dashed (task depends on task)
          // Default (project connections): neutral solid
          const strokeColor = isValidates
            ? 'rgb(70, 90, 65)'     // Dark sage for validates
            : isBlockedBy
            ? 'rgb(160, 82, 65)'    // Terra/red for blockedBy
            : 'rgb(90, 85, 80)';    // Neutral for project connections
          const strokeDash = isBlockedBy
            ? '4 3'  // Dashed for blockedBy
            : undefined; // Solid for validates and project

          return (
            <path
              d={pathD}
              fill="none"
              stroke={strokeColor}
              stroke-width={1}
              stroke-dasharray={strokeDash}
              opacity={0.6}
            />
          );
        }}
      </For>
    </svg>
  );
};


// =============================================================================
// Helper: Synthesize Live Tree with Project Hierarchy from Draft
//
// Since project nodes are UI-only and not stored in the live_nodes table,
// we rebuild the project structure from the draft tree when rendering live.
// =============================================================================

/**
 * Build live tree with project structure from draft tree.
 * Project nodes are synthesized; tasks/evals come from actual live nodes.
 */
function buildLiveTreeWithProjects(
  draftTree: DraftNodeTree[],
  liveNodes: LiveNodeTree[]
): LiveNodeTree[] {
  // Create a map of live nodes by their ID for quick lookup
  const liveById = new Map<string, LiveNodeTree>();
  for (const node of liveNodes) {
    liveById.set(node.id, node);
  }

  // Recursively build tree using draft structure
  const buildNode = (draft: DraftNodeTree): LiveNodeTree | null => {
    if (draft.nodeType === 'project') {
      // Project nodes: synthesize from children
      const children = draft.children
        .map(child => buildNode(child))
        .filter((n): n is LiveNodeTree => n !== null);

      if (children.length === 0) return null; // No live children yet

      // Compute status from children
      const allDone = children.every(c => c.status === 'done');
      const anyWorking = children.some(c => c.status === 'working');
      const anyFailed = children.some(c => c.status === 'failed');
      const status: LiveNodeStatus = anyFailed ? 'failed' : anyWorking ? 'working' : allDone ? 'done' : 'pending';

      return {
        id: draft.id,
        draftNodeId: draft.id,
        name: draft.name,
        nodeType: 'project',
        content: draft.content,
        status,
        validates: [],
        blockedBy: [],
        children,
        x: draft.x,
        y: draft.y,
        completedAt: null,
        lastCommitSha: null,
      };
    } else {
      // Task/eval: look up live node
      const live = liveById.get(draft.id);
      if (!live) return null; // Not dispatched yet

      // Recursively build children from draft structure
      const children = draft.children
        .map(child => buildNode(child))
        .filter((n): n is LiveNodeTree => n !== null);

      return {
        ...live,
        children,
      };
    }
  };

  return draftTree
    .map(root => buildNode(root))
    .filter((n): n is LiveNodeTree => n !== null);
}

// =============================================================================
// Main Component
// =============================================================================

export const SpecBoard: Component = () => {
  const project = useProject();
  const delta = useDelta();

  // Selection state
  const [selectedNodeId, setSelectedNodeId] = createSignal<string | null>(null);
  const [selectedLiveNodeId, setSelectedLiveNodeId] = createSignal<string | null>(null);

  // View focus state: 'both' shows side-by-side, 'live'/'draft' expands that section
  const [focusedView, setFocusedView] = createSignal<'both' | 'live' | 'draft'>('both');

  // Edit modal state
  const [editingNode, setEditingNode] = createSignal<DraftNodeTree | null>(null);
  const [viewingLiveNode, setViewingLiveNode] = createSignal<LiveNodeTree | null>(null);
  const [editForm, setEditForm] = createSignal({ name: '', content: '', validates: '', blockedBy: '' });

  // New node prompt
  const [showNewPrompt, setShowNewPrompt] = createSignal(false);
  const [newNodeParentId, setNewNodeParentId] = createSignal<string | null>(null);
  const [newNodeType, setNewNodeType] = createSignal<NodeType>('task');
  const [newNodeName, setNewNodeName] = createSignal('');
  let newNodeInputRef: HTMLInputElement | undefined;

  // Context menu
  const [contextMenu, setContextMenu] = createSignal<{
    x: number;
    y: number;
    node: DraftNodeTree | null;
    isBackground: boolean;
  } | null>(null);

  // Pan and zoom state (per side)
  const [draftZoom, setDraftZoom] = createSignal(1);
  const [liveZoom, setLiveZoom] = createSignal(1);
  const [draftPan, setDraftPan] = createSignal({ x: 0, y: 0 });
  const [livePan, setLivePan] = createSignal({ x: 0, y: 0 });
  const [isPanning, setIsPanning] = createSignal(false);
  const [panStart, setPanStart] = createSignal({ x: 0, y: 0 });
  const [activePanSide, setActivePanSide] = createSignal<'draft' | 'live' | null>(null);
  let canvasRef: HTMLDivElement | undefined;

  // Build live tree with project hierarchy from draft (project nodes are UI-only)
  const liveTreeWithProjects = createMemo(() =>
    buildLiveTreeWithProjects(delta.draftTree(), delta.liveTree())
  );

  // Computed layouts
  const draftLayout = createMemo(() => layoutTree(delta.draftTree(), 0));
  const liveLayout = createMemo(() => layoutTree(liveTreeWithProjects(), 0));

  // Check if we have a live tree (post-dispatch)
  const hasLiveTree = () => liveTreeWithProjects().length > 0;

  // Compute effective run status from live tree
  const liveRunStatus = createMemo(() => {
    const run = delta.projectRun();
    if (!run) return null;

    // Check if any live node is working (use synthesized tree with projects)
    const liveNodes = flattenLiveTree(liveTreeWithProjects());
    const hasWorkingNode = liveNodes.some(n => n.status === 'working');

    if (hasWorkingNode) return 'working';
    if (run.status === 'failed') return 'failed';
    if (run.status === 'paused') return 'paused';

    // All nodes done or pending - show idle
    const allDone = liveNodes.every(n => n.status === 'done');
    if (allDone && liveNodes.length > 0) return 'done';

    // Run is working but no nodes active yet - workers starting up
    // Only show "starting" if ALL nodes are still pending (no work started yet)
    const allPending = liveNodes.every(n => n.status === 'pending');
    if (run.status === 'working' && liveNodes.length > 0 && allPending) return 'starting';

    return 'idle';
  });

  // Layout constants
  const SECTION_GAP = 16;     // Gap between sections (horizontal)
  const TREE_LEFT_MARGIN = 24; // Left margin for tree root

  // ==========================================================================
  // Helpers
  // ==========================================================================

  const flattenDraftTree = (nodes: DraftNodeTree[]): DraftNodeTree[] => {
    const result: DraftNodeTree[] = [];
    const flatten = (node: DraftNodeTree) => {
      result.push(node);
      node.children.forEach(flatten);
    };
    nodes.forEach(flatten);
    return result;
  };

  const flattenLiveTree = (nodes: LiveNodeTree[]): LiveNodeTree[] => {
    const result: LiveNodeTree[] = [];
    const flatten = (node: LiveNodeTree) => {
      result.push(node);
      node.children.forEach(flatten);
    };
    nodes.forEach(flatten);
    return result;
  };

  const findParentId = (nodes: DraftNodeTree[], targetId: string): string | null => {
    for (const node of nodes) {
      if (node.children.some((c) => c.id === targetId)) {
        return node.id;
      }
      const found = findParentId(node.children, targetId);
      if (found !== null) return found;
    }
    return null;
  };

  // ==========================================================================
  // Handlers
  // ==========================================================================

  const handleDoubleClick = (node: DraftNodeTree) => {
    setEditForm({
      name: node.name,
      content: node.content,
      validates: node.validates.join(', '),
      blockedBy: node.blockedBy.join(', '),
    });
    setEditingNode(node);
  };

  const handleSaveEdit = async () => {
    const node = editingNode();
    if (!node) return;

    const form = editForm();
    await delta.updateDraftNode(node.id, {
      name: form.name,
      content: form.content,
      validates: form.validates
        .split(',')
        .map((s) => s.trim())
        .filter(Boolean),
      blockedBy: form.blockedBy
        .split(',')
        .map((s) => s.trim())
        .filter(Boolean),
    });
    setEditingNode(null);
  };

  const handleContextMenu = (e: MouseEvent, node: DraftNodeTree | null) => {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({ x: e.clientX, y: e.clientY, node, isBackground: node === null });
  };

  const hideContextMenu = () => setContextMenu(null);

  const handleAddChild = () => {
    const cm = contextMenu();
    if (cm && cm.node) {
      setNewNodeParentId(cm.node.id);
      setNewNodeType('task');
      setShowNewPrompt(true);
      hideContextMenu();
      setTimeout(() => newNodeInputRef?.focus(), 50);
    }
  };

  const handleAddSibling = () => {
    const cm = contextMenu();
    if (cm && cm.node) {
      setNewNodeParentId(findParentId(delta.draftTree(), cm.node.id));
      setNewNodeType('task');
      setShowNewPrompt(true);
      hideContextMenu();
      setTimeout(() => newNodeInputRef?.focus(), 50);
    }
  };

  const handleDelete = async () => {
    const cm = contextMenu();
    if (cm && cm.node) {
      const nodeType = cm.node.nodeType === 'eval' ? 'eval' : 'task';
      const hasChildren = cm.node.children.length > 0;
      const description = hasChildren
        ? `"${cm.node.name}" and all its children`
        : `"${cm.node.name}"`;
      const confirmed = await window.confirmDialog?.delete(description, nodeType);
      if (confirmed) {
        await delta.deleteDraftNode(cm.node.id);
      }
      hideContextMenu();
    }
  };

  const handleAddRootTask = () => {
    setNewNodeParentId(null);
    setNewNodeType('task');
    setShowNewPrompt(true);
    hideContextMenu();
    setTimeout(() => newNodeInputRef?.focus(), 50);
  };

  const handleAddRootEval = () => {
    setNewNodeParentId(null);
    setNewNodeType('eval');
    setShowNewPrompt(true);
    hideContextMenu();
    setTimeout(() => newNodeInputRef?.focus(), 50);
  };

  const handleCreateNode = async () => {
    const name = newNodeName().trim();
    if (!name) {
      setShowNewPrompt(false);
      return;
    }

    await delta.createDraftNode({
      parentId: newNodeParentId(),
      name,
      nodeType: newNodeType(),
    });

    setShowNewPrompt(false);
    setNewNodeName('');
  };

  const handleDispatch = async () => {
    // Guard against double-click - check both hasDiff and dispatchPending
    if (!delta.hasDiff() || delta.dispatchPending()) {
      if (!delta.hasDiff()) {
        window.toast?.info('No changes to dispatch');
      }
      return;
    }

    await delta.dispatch();
  };

  const handleResetTree = async () => {
    const confirmed = await window.confirmDialog?.show({
      title: 'Reset Tree',
      message: 'This will delete all tasks and evals, keeping only the project root. This cannot be undone.',
      confirmText: 'Reset',
      danger: true,
    });
    if (confirmed) {
      await delta.resetTree();
    }
    hideContextMenu();
  };

  const handleDeleteProject = async () => {
    const projectName = project.selectedProject()?.name || 'this project';
    const confirmed = await window.confirmDialog?.show({
      title: 'Delete Project',
      message: `Are you sure you want to delete "${projectName}"? This will remove the project and all its tasks. This cannot be undone.`,
      confirmText: 'Delete Project',
      danger: true,
    });
    if (confirmed) {
      const projectId = project.selectedProjectId();
      if (projectId) {
        await project.removeProject(projectId);
      }
    }
    hideContextMenu();
  };

  // Keyboard shortcuts
  createEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') return;

      if (e.key === 'Escape') {
        if (editingNode()) setEditingNode(null);
        else if (showNewPrompt()) setShowNewPrompt(false);
        else if (contextMenu()) hideContextMenu();
      }
    };

    document.addEventListener('keydown', handler);
    onCleanup(() => document.removeEventListener('keydown', handler));
  });

  // Detect which side the cursor is over
  const getSideFromEvent = (e: MouseEvent | WheelEvent): 'draft' | 'live' | null => {
    const target = e.target as HTMLElement;
    const section = target.closest('.tree-section');
    if (!section) return null;
    return section.classList.contains('draft-section') ? 'draft' : 'live';
  };

  // Calculate minimum zoom to fit tree within panel
  const calcFitZoom = (treeWidth: number, treeHeight: number, panelWidth: number, panelHeight: number): number => {
    if (treeWidth <= 0 || treeHeight <= 0) return 0.25;
    const padding = TREE_LEFT_MARGIN * 2; // Padding on both sides
    const availableWidth = Math.max(panelWidth - padding, 100);
    const availableHeight = Math.max(panelHeight - padding, 100);
    const fitZoom = Math.min(availableWidth / treeWidth, availableHeight / treeHeight);
    // Clamp to reasonable bounds (never smaller than would make tree 50px, never larger than 1)
    return Math.max(0.1, Math.min(1, fitZoom));
  };

  // Pan and zoom handlers (per side)
  // Figma-style: scroll up (negative deltaY) = zoom in, scroll down = zoom out
  // Zoom centers on cursor position
  const handleWheel = (e: WheelEvent) => {
    const side = getSideFromEvent(e);
    if (!side) return;

    e.preventDefault();
    const zoomFactor = e.deltaY < 0 ? 1.1 : 0.9;

    if (side === 'draft') {
      const oldZoom = draftZoom();
      const section = (e.target as HTMLElement).closest('.tree-section');
      const layout = draftLayout();
      // Calculate minimum zoom to fit tree in panel
      const rect = section?.getBoundingClientRect();
      const minZoom = rect ? calcFitZoom(layout.width, layout.height, rect.width, rect.height) : 0.25;
      const newZoom = Math.max(minZoom, Math.min(3, oldZoom * zoomFactor));
      if (section && rect) {
        // Cursor position relative to section
        const cursorX = e.clientX - rect.left - TREE_LEFT_MARGIN;
        const cursorY = e.clientY - rect.top - rect.height / 2;
        const currentPan = draftPan();
        // Point in content space under cursor
        const contentX = (cursorX - currentPan.x) / oldZoom;
        const contentY = (cursorY - currentPan.y) / oldZoom;
        // New pan to keep that point under cursor
        setDraftPan({
          x: cursorX - contentX * newZoom,
          y: cursorY - contentY * newZoom,
        });
      }
      setDraftZoom(newZoom);
    } else {
      const oldZoom = liveZoom();
      const section = (e.target as HTMLElement).closest('.tree-section');
      const layout = liveLayout();
      // Calculate minimum zoom to fit tree in panel
      const rect = section?.getBoundingClientRect();
      const minZoom = rect ? calcFitZoom(layout.width, layout.height, rect.width, rect.height) : 0.25;
      const newZoom = Math.max(minZoom, Math.min(3, oldZoom * zoomFactor));
      if (section && rect) {
        // Cursor position relative to section
        const cursorX = e.clientX - rect.left - TREE_LEFT_MARGIN;
        const cursorY = e.clientY - rect.top - rect.height / 2;
        const currentPan = livePan();
        // Point in content space under cursor
        const contentX = (cursorX - currentPan.x) / oldZoom;
        const contentY = (cursorY - currentPan.y) / oldZoom;
        // New pan to keep that point under cursor
        setLivePan({
          x: cursorX - contentX * newZoom,
          y: cursorY - contentY * newZoom,
        });
      }
      setLiveZoom(newZoom);
    }
  };

  const handleMouseDown = (e: MouseEvent) => {
    // Middle mouse button or alt+left click for panning
    if (e.button === 1 || (e.button === 0 && e.altKey)) {
      const side = getSideFromEvent(e);
      if (!side) return;

      e.preventDefault();
      setIsPanning(true);
      setActivePanSide(side);
      const pan = side === 'draft' ? draftPan() : livePan();
      setPanStart({ x: e.clientX - pan.x, y: e.clientY - pan.y });
    }
  };

  // Clamp pan to keep tree within reasonable bounds
  // Ensures at least some portion of tree is always visible
  const clampPan = (
    pan: { x: number; y: number },
    treeWidth: number,
    treeHeight: number,
    panelWidth: number,
    panelHeight: number,
    zoom: number
  ): { x: number; y: number } => {
    const scaledTreeWidth = treeWidth * zoom;
    const scaledTreeHeight = treeHeight * zoom;

    // Allow panning such that at least 20% of tree (or 50px) stays visible
    const minVisible = Math.max(50, Math.min(scaledTreeWidth, scaledTreeHeight) * 0.2);

    // X bounds: tree can't go further right than showing minVisible on left edge,
    // and can't go further left than showing minVisible on right edge
    const maxX = panelWidth - minVisible - TREE_LEFT_MARGIN;
    const minX = -(scaledTreeWidth - minVisible);

    // Y bounds: similar for vertical (remember y=0 is centered due to transform)
    const maxY = (panelHeight / 2) - minVisible;
    const minY = -(scaledTreeHeight / 2) + minVisible - (panelHeight / 2);

    return {
      x: Math.max(minX, Math.min(maxX, pan.x)),
      y: Math.max(minY, Math.min(maxY, pan.y)),
    };
  };

  const handleMouseMove = (e: MouseEvent) => {
    if (isPanning() && activePanSide()) {
      const rawPan = {
        x: e.clientX - panStart().x,
        y: e.clientY - panStart().y,
      };

      // Get panel dimensions for clamping
      const section = document.querySelector(
        activePanSide() === 'draft' ? '.draft-section' : '.live-section'
      );
      const rect = section?.getBoundingClientRect();

      if (activePanSide() === 'draft') {
        const layout = draftLayout();
        const clampedPan = rect
          ? clampPan(rawPan, layout.width, layout.height, rect.width, rect.height, draftZoom())
          : rawPan;
        setDraftPan(clampedPan);
      } else {
        const layout = liveLayout();
        const clampedPan = rect
          ? clampPan(rawPan, layout.width, layout.height, rect.width, rect.height, liveZoom())
          : rawPan;
        setLivePan(clampedPan);
      }
    }
  };

  const handleMouseUp = () => {
    setIsPanning(false);
    setActivePanSide(null);
  };

  const resetView = () => {
    setDraftZoom(1);
    setLiveZoom(1);
    setDraftPan({ x: 0, y: 0 });
    setLivePan({ x: 0, y: 0 });
    setFocusedView('both');
  };

  // ==========================================================================
  // Render
  // ==========================================================================

  const NoProjectSelected = () => (
    <div class="flex-1 flex flex-col items-center justify-center bg-pasture-900">
      <div
        class="w-16 h-16 mb-4 rounded-lg flex items-center justify-center"
        style={{
          background: 'rgba(212,165,116,0.05)',
          border: '1px solid rgba(212,165,116,0.1)',
        }}
      >
        <svg class="w-8 h-8 text-wool-700" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="1.25"
            d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z"
          />
        </svg>
      </div>
      <p class="text-sm text-wool-500 mb-4">No project selected</p>
      <button
        onClick={() => project.openProjectSetup()}
        class="px-3 py-1.5 rounded text-[12px] font-medium"
        style={{
          background: 'rgba(212,165,116,0.15)',
          border: '1px solid rgba(212,165,116,0.25)',
          color: 'var(--amber-400)',
        }}
      >
        New Project
      </button>
    </div>
  );

  const EmptyTreeState = () => (
    <div
      class="flex flex-col items-center justify-center h-full text-center"
      onContextMenu={(e) => handleContextMenu(e, null)}
    >
      <p class="text-[12px] text-wool-600 mb-1">No tasks yet</p>
      <p class="text-[11px] text-wool-700">Right-click to add</p>
    </div>
  );

  return (
    <Show when={project.selectedProject()} fallback={<NoProjectSelected />}>
      <div class="flex-1 flex flex-col overflow-hidden bg-pasture-900">
        {/* Header - minimal */}
        <div
          class="flex items-center justify-between px-3 py-2"
          style={{ 'border-bottom': '1px solid rgba(51, 51, 51, 0.5)' }}
        >
          <div class="flex items-center gap-2">
            {/* Project name shown here if needed */}
          </div>

          <div class="flex items-center gap-2">
            {/* Delta toggle */}
            <button
              onClick={() => delta.toggleDeltaIndicators()}
              class="p-1 rounded"
              style={{
                background: delta.showDeltaIndicators() ? 'rgba(212, 165, 116, 0.15)' : 'transparent',
                color: delta.showDeltaIndicators() ? 'var(--amber-400)' : 'var(--wool-600)',
              }}
              title="Toggle delta indicators"
            >
              <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="1.5"
                  d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2"
                />
              </svg>
            </button>

            {/* Diff summary */}
            <Show when={delta.hasDiff()}>
              <div class="flex items-center gap-1.5 px-1.5 py-0.5 rounded" style={{ background: 'rgba(36, 36, 36, 0.6)' }}>
                <Show when={(delta.diff()?.newNodes.length || 0) > 0}>
                  <span class="text-[10px] text-sage font-medium">+{delta.diff()?.newNodes.length}</span>
                </Show>
                <Show when={(delta.diff()?.modifiedNodes.length || 0) > 0}>
                  <span class="text-[10px] text-amber-400 font-medium">~{delta.diff()?.modifiedNodes.length}</span>
                </Show>
                <Show when={(delta.diff()?.deletedNodes.length || 0) > 0}>
                  <span class="text-[10px] text-terra font-medium">-{delta.diff()?.deletedNodes.length}</span>
                </Show>
              </div>
            </Show>

            {/* Dispatch button */}
            <button
              onClick={handleDispatch}
              disabled={!delta.hasDiff() || delta.dispatchPending()}
              class="px-2.5 py-1 rounded text-[11px] font-medium disabled:opacity-30 disabled:cursor-not-allowed"
              style={{
                background: delta.hasDiff() ? 'rgba(212, 165, 116, 0.2)' : 'rgba(36, 36, 36, 0.5)',
                border: delta.hasDiff() ? '1px solid rgba(212, 165, 116, 0.35)' : '1px solid rgba(51, 51, 51, 0.4)',
                color: delta.hasDiff() ? 'var(--amber-300)' : 'var(--wool-600)',
              }}
            >
              {delta.dispatchPending() ? 'Dispatching...' : 'Dispatch'}
            </button>
          </div>
        </div>

        {/* Canvas Area with Pan/Zoom */}
        <div
          ref={canvasRef}
          class="flex-1 overflow-hidden relative"
          style={{
            cursor: isPanning() ? 'grabbing' : 'default',
            // Subtle dot grid background
            'background-image': `radial-gradient(circle, rgba(90, 85, 80, 0.15) 1px, transparent 1px)`,
            'background-size': '24px 24px',
            'background-position': '12px 12px',
          }}
          onWheel={handleWheel}
          onMouseDown={handleMouseDown}
          onMouseMove={handleMouseMove}
          onMouseUp={handleMouseUp}
          onMouseLeave={handleMouseUp}
        >
          {/* Reset view button */}
          <div class="absolute bottom-3 right-3 z-20">
            <button
              onClick={resetView}
              class="flex items-center gap-1.5 px-2 py-1 rounded text-wool-400 hover:text-wool-200"
              style={{ background: 'rgba(30, 30, 30, 0.8)', border: '1px solid rgba(64, 64, 64, 0.4)' }}
              title="Reset view (zoom & pan)"
            >
              <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 8V4m0 0h4M4 4l5 5m11-1V4m0 0h-4m4 0l-5 5M4 16v4m0 0h4m-4 0l5-5m11 5v-4m0 4h-4m4 0l-5-5" />
              </svg>
              <span class="text-[10px]">Reset</span>
            </button>
          </div>

          <Show
            when={delta.draftTree().length > 0 || liveTreeWithProjects().length > 0}
            fallback={<EmptyTreeState />}
          >
            {/* Split view container - fills viewport */}
            <div
              class="absolute inset-0 flex"
              style={{ gap: `${SECTION_GAP}px`, padding: '8px' }}
              onContextMenu={(e) => {
                // Only show context menu for draft section (live is read-only)
                if ((e.target as HTMLElement).closest('.draft-section')) {
                  handleContextMenu(e, null);
                }
              }}
            >
              {/* Draft Section (LEFT, or FULL when no live tree) */}
              <Show when={delta.draftTree().length > 0 && focusedView() !== 'live'}>
                <div
                  class="tree-section draft-section flex-1 rounded-lg relative overflow-hidden"
                  style={{
                    // Only show split-view styling when live tree exists
                    border: hasLiveTree() ? '1px dashed rgba(212, 165, 116, 0.2)' : 'none',
                    background: hasLiveTree() ? 'rgba(36, 34, 30, 0.3)' : 'transparent',
                  }}
                >
                  {/* Section label - only show when split view (live tree exists) */}
                  <Show when={hasLiveTree()}>
                    <div
                      class="absolute z-10 text-[9px] font-medium uppercase tracking-wider px-2 py-0.5 rounded"
                      style={{
                        left: '8px',
                        top: '6px',
                        background: 'rgba(30, 30, 30, 0.8)',
                      }}
                    >
                      <button
                        onClick={() => setFocusedView(focusedView() === 'draft' ? 'both' : 'draft')}
                        class="flex items-center gap-1.5 hover:opacity-80"
                        title={focusedView() === 'draft' ? 'Show both' : 'Focus Draft'}
                      >
                        <span style={{ color: 'var(--amber-600)' }}>Draft</span>
                        <span class="text-wool-600 text-[8px]">{Math.round(draftZoom() * 100)}%</span>
                        <Show when={focusedView() !== 'draft'}>
                          <svg class="w-2.5 h-2.5 text-wool-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 8V4m0 0h4M4 4l5 5m11-1V4m0 0h-4m4 0l-5 5M4 16v4m0 0h4m-4 0l5-5m11 5l-5-5m5 5v-4m0 4h-4" />
                          </svg>
                        </Show>
                        <Show when={focusedView() === 'draft'}>
                          <svg class="w-2.5 h-2.5 text-wool-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 9V4.5M9 9H4.5M9 9L3.75 3.75M9 15v4.5M9 15H4.5M9 15l-5.25 5.25M15 9h4.5M15 9V4.5M15 9l5.25-5.25M15 15h4.5M15 15v4.5m0-4.5l5.25 5.25" />
                          </svg>
                        </Show>
                      </button>
                    </div>
                  </Show>

                  {/* Tree content - centered vertically, root at left */}
                  <div
                    class="absolute"
                    style={{
                      left: `${TREE_LEFT_MARGIN}px`,
                      top: '50%',
                      transform: `translate(${draftPan().x}px, calc(-50% + ${draftPan().y}px)) scale(${draftZoom()})`,
                      'transform-origin': '0 50%',
                    }}
                  >
                    <div class="relative" style={{ width: `${draftLayout().width}px`, height: `${draftLayout().height}px` }}>
                      {/* Dependency connectors (blocked_by and validates relationships) */}
                      <DependencyConnectors
                        positions={draftLayout().positions}
                        relationships={draftLayout().crossTreeRelationships}
                      />
                      <For each={flattenDraftTree(delta.draftTree())}>
                        {(node) => {
                          const pos = () => draftLayout().positions.get(node.id);
                          return (
                            <Show when={pos()}>
                              <DraftNodeCard
                                node={node}
                                position={pos()!}
                                diff={delta.diff()}
                                showDelta={delta.showDeltaIndicators()}
                                selected={selectedNodeId() === node.id}
                                onSelect={() => {
                                  setSelectedNodeId(node.id);
                                  setSelectedLiveNodeId(null);
                                  setViewingLiveNode(null);
                                }}
                                onDoubleClick={() => handleDoubleClick(node)}
                                onContextMenu={(e) => handleContextMenu(e, node)}
                              />
                            </Show>
                          );
                        }}
                      </For>
                    </div>
                  </div>
                </div>
              </Show>

              {/* Live Section (RIGHT) */}
              <Show when={hasLiveTree() && focusedView() !== 'draft'}>
                <div
                  class="tree-section live-section flex-1 rounded-lg relative overflow-hidden"
                  style={{
                    border: '1px dashed rgba(90, 85, 80, 0.25)',
                    background: 'rgba(30, 30, 30, 0.3)',
                  }}
                >
                  {/* Section label */}
                  <div
                    class="absolute z-10 flex items-center gap-1.5 text-[9px] font-medium uppercase tracking-wider px-2 py-0.5 rounded"
                    style={{
                      left: '8px',
                      top: '6px',
                      background: 'rgba(30, 30, 30, 0.8)',
                    }}
                  >
                    <button
                      onClick={() => setFocusedView(focusedView() === 'live' ? 'both' : 'live')}
                      class="flex items-center gap-1.5 hover:opacity-80"
                      title={focusedView() === 'live' ? 'Show both' : 'Focus Live'}
                    >
                      <span style={{ color: 'var(--wool-500)' }}>Live</span>
                      <span class="text-wool-600 text-[8px]">{Math.round(liveZoom() * 100)}%</span>
                      <Show when={liveRunStatus()}>
                        <span style={{ color: 'var(--wool-600)' }}>·</span>
                        <span
                          class={(liveRunStatus() === 'working' || liveRunStatus() === 'starting') ? 'animate-pulse' : ''}
                          style={{
                            color: liveRunStatus() === 'working' ? 'var(--amber-400)'
                              : liveRunStatus() === 'starting' ? 'var(--amber-300)'
                              : liveRunStatus() === 'done' ? 'var(--sage)'
                              : liveRunStatus() === 'failed' ? 'var(--terra)'
                              : liveRunStatus() === 'paused' ? 'var(--golden)'
                              : 'var(--wool-600)',
                          }}
                        >
                          {liveRunStatus() === 'starting' ? 'starting...' : liveRunStatus()}
                        </span>
                      </Show>
                      <Show when={focusedView() !== 'live'}>
                        <svg class="w-2.5 h-2.5 text-wool-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 8V4m0 0h4M4 4l5 5m11-1V4m0 0h-4m4 0l-5 5M4 16v4m0 0h4m-4 0l5-5m11 5l-5-5m5 5v-4m0 4h-4" />
                        </svg>
                      </Show>
                      <Show when={focusedView() === 'live'}>
                        <svg class="w-2.5 h-2.5 text-wool-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 9V4.5M9 9H4.5M9 9L3.75 3.75M9 15v4.5M9 15H4.5M9 15l-5.25 5.25M15 9h4.5M15 9V4.5M15 9l5.25-5.25M15 15h4.5M15 15v4.5m0-4.5l5.25 5.25" />
                        </svg>
                      </Show>
                    </button>
                  </div>

                  {/* Tree content - centered vertically, root at left */}
                  <div
                    class="absolute"
                    style={{
                      left: `${TREE_LEFT_MARGIN}px`,
                      top: '50%',
                      transform: `translate(${livePan().x}px, calc(-50% + ${livePan().y}px)) scale(${liveZoom()})`,
                      'transform-origin': '0 50%',
                    }}
                  >
                    <div class="relative" style={{ width: `${liveLayout().width}px`, height: `${liveLayout().height}px` }}>
                      {/* Dependency connectors (blocked_by and validates relationships) */}
                      <DependencyConnectors
                        positions={liveLayout().positions}
                        relationships={liveLayout().crossTreeRelationships}
                      />
                      <For each={flattenLiveTree(liveTreeWithProjects())}>
                        {(node) => {
                          const pos = () => liveLayout().positions.get(node.id);
                          return (
                            <Show when={pos()}>
                              <LiveNodeCard
                                node={node}
                                position={pos()!}
                                diff={delta.diff()}
                                showDelta={delta.showDeltaIndicators()}
                                selected={selectedLiveNodeId() === node.id}
                                onSelect={() => {
                                  setSelectedLiveNodeId(node.id);
                                  setSelectedNodeId(null);
                                  setViewingLiveNode(node);
                                }}
                              />
                            </Show>
                          );
                        }}
                      </For>
                    </div>
                  </div>
                </div>
              </Show>
            </div>
          </Show>
        </div>

        {/* Context Menu */}
        <Show when={contextMenu()}>
          <div
            class="fixed inset-0 z-[99]"
            onClick={hideContextMenu}
            onContextMenu={(e) => { e.preventDefault(); hideContextMenu(); }}
          />
          <div
            class="fixed z-[100] py-1 min-w-[140px] rounded overflow-hidden"
            style={{
              left: `${contextMenu()!.x}px`,
              top: `${contextMenu()!.y}px`,
              background: 'linear-gradient(180deg, #2d2d2d 0%, #262626 100%)',
              border: '1px solid rgba(64, 64, 64, 0.6)',
              'box-shadow': '0 4px 16px rgba(0,0,0,0.4)',
            }}
          >
            <Show when={contextMenu()!.node}>
              {/* Project node menu */}
              <Show when={contextMenu()!.node!.nodeType === 'project'}>
                <button class="w-full px-3 py-1.5 text-left text-[11px] text-wool-200 hover:bg-white/5" onClick={handleAddChild}>
                  Add Task
                </button>
                <button
                  class="w-full px-3 py-1.5 text-left text-[11px] text-sage hover:bg-sage/10"
                  onClick={() => {
                    const cm = contextMenu();
                    if (cm?.node) {
                      setNewNodeParentId(cm.node.id);
                      setNewNodeType('eval');
                      setShowNewPrompt(true);
                      hideContextMenu();
                      setTimeout(() => newNodeInputRef?.focus(), 50);
                    }
                  }}
                >
                  Add Eval
                </button>
                <button
                  class="w-full px-3 py-1.5 text-left text-[11px] text-wool-200 hover:bg-white/5"
                  onClick={() => { handleDoubleClick(contextMenu()!.node!); hideContextMenu(); }}
                >
                  Rename
                </button>
                <div class="h-px bg-white/10 my-1" />
                <button class="w-full px-3 py-1.5 text-left text-[11px] text-amber-400 hover:bg-amber-500/10" onClick={handleResetTree}>
                  Reset Tree
                </button>
                <button class="w-full px-3 py-1.5 text-left text-[11px] text-terra hover:bg-terra/10" onClick={handleDeleteProject}>
                  Delete Project
                </button>
              </Show>
              {/* Task/eval node menu */}
              <Show when={contextMenu()!.node!.nodeType !== 'project'}>
                <button class="w-full px-3 py-1.5 text-left text-[11px] text-wool-200 hover:bg-white/5" onClick={handleAddChild}>
                  Add child
                </button>
                <button class="w-full px-3 py-1.5 text-left text-[11px] text-wool-200 hover:bg-white/5" onClick={handleAddSibling}>
                  Add sibling
                </button>
                <button
                  class="w-full px-3 py-1.5 text-left text-[11px] text-wool-200 hover:bg-white/5"
                  onClick={() => { handleDoubleClick(contextMenu()!.node!); hideContextMenu(); }}
                >
                  Edit
                </button>
                <div class="h-px bg-white/10 my-1" />
                <button class="w-full px-3 py-1.5 text-left text-[11px] text-terra hover:bg-terra/10" onClick={handleDelete}>
                  Delete
                </button>
              </Show>
            </Show>
            {/* Background menu */}
            <Show when={contextMenu()!.isBackground}>
              <button class="w-full px-3 py-1.5 text-left text-[11px] text-wool-200 hover:bg-white/5" onClick={handleAddRootTask}>
                Add task
              </button>
              <button class="w-full px-3 py-1.5 text-left text-[11px] text-sage hover:bg-sage/10" onClick={handleAddRootEval}>
                Add eval
              </button>
            </Show>
          </div>
        </Show>

        {/* New Node Prompt */}
        <Show when={showNewPrompt()}>
          {(() => {
            const isEval = () => newNodeType() === 'eval';
            const typeLabel = () => (isEval() ? 'Eval' : 'Task');

            return (
              <div
                class="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
                onClick={(e) => {
                  if (e.target === e.currentTarget) setShowNewPrompt(false);
                }}
              >
                <div
                  class="w-[360px] rounded-lg shadow-xl"
                  style={{
                    background: 'var(--pasture-800)',
                    border: '1px solid var(--pasture-600)',
                  }}
                >
                  {/* Header */}
                  <div class="p-4 border-b border-pasture-600">
                    <div class="flex items-center gap-3">
                      <div
                        class="w-9 h-9 rounded-lg flex items-center justify-center flex-shrink-0"
                        style={{
                          background: isEval() ? 'rgba(125, 153, 112, 0.15)' : 'rgba(212, 165, 116, 0.12)',
                          border: `1px solid ${isEval() ? 'rgba(125, 153, 112, 0.25)' : 'rgba(212, 165, 116, 0.2)'}`,
                        }}
                      >
                        <Show when={isEval()} fallback={
                          <svg class="w-4 h-4" style={{ color: 'var(--amber-500)' }} fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" />
                          </svg>
                        }>
                          <svg class="w-4 h-4" style={{ color: 'var(--sage)' }} fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                          </svg>
                        </Show>
                      </div>
                      <div>
                        <h2 class="text-sm font-semibold text-wool-100">New {typeLabel()}</h2>
                        <p class="text-xs text-wool-500">
                          {isEval() ? 'Add a verification checkpoint' : 'Add a work item'}
                        </p>
                      </div>
                    </div>
                  </div>

                  {/* Content */}
                  <div class="p-4">
                    <form onSubmit={(e) => { e.preventDefault(); handleCreateNode(); }}>
                      <div class="space-y-1.5">
                        <label for="new-node-name" class="text-xs font-medium text-wool-300">Name</label>
                        <input
                          ref={newNodeInputRef}
                          id="new-node-name"
                          type="text"
                          value={newNodeName()}
                          onInput={(e) => setNewNodeName(e.currentTarget.value)}
                          onKeyDown={(e) => { if (e.key === 'Escape') setShowNewPrompt(false); }}
                          placeholder={isEval() ? 'e.g., API returns valid JSON' : 'e.g., Build authentication flow'}
                          class="w-full px-3 py-2 rounded-md text-sm bg-pasture-900 border text-wool-100 placeholder-wool-600 focus:outline-none focus:ring-2 focus:ring-amber-500/30"
                          style={{
                            'font-family': 'system-ui, -apple-system, sans-serif',
                            'border-color': isEval() ? 'rgba(125, 153, 112, 0.4)' : 'var(--pasture-600)',
                          }}
                        />
                        <p class="text-[11px] text-wool-500">Press Enter to create, Escape to cancel</p>
                      </div>
                    </form>
                  </div>

                  {/* Footer */}
                  <div class="px-4 py-3 border-t border-pasture-600 flex justify-end gap-2">
                    <button
                      onClick={() => setShowNewPrompt(false)}
                      class="px-3 py-1.5 rounded-md text-xs font-medium text-wool-400 hover:text-wool-200 hover:bg-white/5"
                    >
                      Cancel
                    </button>
                    <button
                      onClick={handleCreateNode}
                      disabled={!newNodeName().trim()}
                      class="px-3 py-1.5 rounded-md text-xs font-medium disabled:opacity-40"
                      style={{
                        background: isEval() ? 'rgba(125, 153, 112, 0.2)' : 'var(--amber-500)',
                        color: isEval() ? 'var(--sage)' : 'var(--pasture-900)',
                        border: isEval() ? '1px solid rgba(125, 153, 112, 0.3)' : 'none',
                      }}
                    >
                      Create {typeLabel()}
                    </button>
                  </div>
                </div>
              </div>
            );
          })()}
        </Show>

        {/* Edit Modal */}
        <Show when={editingNode()}>
          {(node) => {
            const nodeType = () => node().nodeType;
            const isEval = () => nodeType() === 'eval';
            const isProject = () => nodeType() === 'project';
            const typeLabel = () => (isEval() ? 'Eval' : isProject() ? 'Project' : 'Task');

            return (
              <div
                class="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
                onClick={(e) => { if (e.target === e.currentTarget) setEditingNode(null); }}
              >
                <div
                  class="w-[420px] rounded-lg shadow-xl"
                  style={{
                    background: 'var(--pasture-800)',
                    border: '1px solid var(--pasture-600)',
                  }}
                >
                  {/* Header */}
                  <div class="p-4 border-b border-pasture-600 flex items-start justify-between">
                    <div class="flex items-center gap-3">
                      <div
                        class="w-10 h-10 rounded-lg flex items-center justify-center flex-shrink-0"
                        style={{
                          background: isEval() ? 'rgba(125, 153, 112, 0.15)' : 'rgba(212, 165, 116, 0.12)',
                          border: `1px solid ${isEval() ? 'rgba(125, 153, 112, 0.25)' : 'rgba(212, 165, 116, 0.2)'}`,
                        }}
                      >
                        <Show when={isEval()} fallback={
                          <Show when={isProject()} fallback={
                            <svg class="w-5 h-5" style={{ color: 'var(--amber-500)' }} fill="none" stroke="currentColor" viewBox="0 0 24 24">
                              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" />
                            </svg>
                          }>
                            <svg class="w-5 h-5" style={{ color: 'var(--amber-500)' }} fill="none" stroke="currentColor" viewBox="0 0 24 24">
                              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
                            </svg>
                          </Show>
                        }>
                          <svg class="w-5 h-5" style={{ color: 'var(--sage)' }} fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                          </svg>
                        </Show>
                      </div>
                      <div>
                        <span
                          class="text-[10px] font-medium uppercase tracking-wider"
                          style={{ color: isEval() ? 'var(--sage)' : 'var(--amber-500)', opacity: 0.8 }}
                        >
                          {typeLabel()}
                        </span>
                        <h2 class="text-sm font-semibold text-wool-100 -mt-0.5">
                          {editForm().name || 'Untitled'}
                        </h2>
                      </div>
                    </div>
                    <button
                      onClick={() => setEditingNode(null)}
                      class="p-1 rounded text-wool-500 hover:text-wool-300 hover:bg-white/5"
                    >
                      <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
                      </svg>
                    </button>
                  </div>

                  {/* Content */}
                  <div class="p-4 space-y-4">
                    {/* Name field */}
                    <div class="space-y-1.5">
                      <label for="edit-name" class="text-xs font-medium text-wool-300">Name</label>
                      <input
                        id="edit-name"
                        type="text"
                        value={editForm().name}
                        onInput={(e) => setEditForm((f) => ({ ...f, name: e.currentTarget.value }))}
                        placeholder={`${typeLabel()} name...`}
                        class="w-full px-3 py-2 rounded-md text-sm bg-pasture-900 border border-pasture-600 text-wool-100 placeholder-wool-600 focus:outline-none focus:ring-2 focus:ring-amber-500/30"
                        style={{ 'font-family': 'system-ui, -apple-system, sans-serif' }}
                      />
                    </div>

                    {/* Content field */}
                    <div class="space-y-1.5">
                      <label for="edit-content" class="text-xs font-medium text-wool-300">
                        {isEval() ? 'Acceptance Criteria' : 'Description'}
                      </label>
                      <textarea
                        id="edit-content"
                        value={editForm().content}
                        onInput={(e) => setEditForm((f) => ({ ...f, content: e.currentTarget.value }))}
                        rows={4}
                        placeholder={isEval() ? 'What conditions must be met?' : 'What needs to be done?'}
                        class="w-full px-3 py-2 rounded-md text-sm bg-pasture-900 border border-pasture-600 text-wool-100 placeholder-wool-600 focus:outline-none focus:ring-2 focus:ring-amber-500/30 resize-none"
                        style={{ 'font-family': 'system-ui, -apple-system, sans-serif' }}
                      />
                      <p class="text-[11px] text-wool-500">
                        {isEval() ? 'Describe how to verify this requirement.' : 'Markdown supported.'}
                      </p>
                    </div>

                    {/* Validates field (eval only) */}
                    <Show when={isEval()}>
                      <div class="space-y-1.5">
                        <label for="edit-validates" class="text-xs font-medium" style={{ color: 'var(--sage)' }}>
                          Validates Tasks
                        </label>
                        <input
                          id="edit-validates"
                          type="text"
                          value={editForm().validates}
                          onInput={(e) => setEditForm((f) => ({ ...f, validates: e.currentTarget.value }))}
                          placeholder="task-id-1, task-id-2"
                          class="w-full px-3 py-2 rounded-md text-sm font-mono bg-pasture-900 text-wool-100 placeholder-wool-600 focus:outline-none focus:ring-2 focus:ring-sage/30"
                          style={{ 'border': '1px solid rgba(125, 153, 112, 0.4)' }}
                        />
                        <p class="text-[11px] text-wool-500">
                          Comma-separated task IDs this eval validates.
                        </p>
                      </div>
                    </Show>

                    {/* Blocked By field (task only) */}
                    <Show when={!isEval()}>
                      <div class="space-y-1.5">
                        <label for="edit-blocked-by" class="text-xs font-medium" style={{ color: 'var(--amber-500)' }}>
                          Blocked By
                        </label>
                        <input
                          id="edit-blocked-by"
                          type="text"
                          value={editForm().blockedBy}
                          onInput={(e) => setEditForm((f) => ({ ...f, blockedBy: e.currentTarget.value }))}
                          placeholder="task-id-1, task-id-2"
                          class="w-full px-3 py-2 rounded-md text-sm font-mono bg-pasture-900 text-wool-100 placeholder-wool-600 focus:outline-none focus:ring-2 focus:ring-amber-500/30"
                          style={{ 'border': '1px solid rgba(212, 165, 116, 0.4)' }}
                        />
                        <p class="text-[11px] text-wool-500">
                          Comma-separated task IDs that must complete before this task.
                        </p>
                      </div>
                    </Show>
                  </div>

                  {/* Footer */}
                  <div class="px-4 py-3 border-t border-pasture-600 flex justify-end gap-2">
                    <button
                      onClick={() => setEditingNode(null)}
                      class="px-3 py-1.5 rounded-md text-xs font-medium text-wool-400 hover:text-wool-200 hover:bg-white/5"
                    >
                      Cancel
                    </button>
                    <button
                      onClick={handleSaveEdit}
                      class="px-3 py-1.5 rounded-md text-xs font-medium"
                      style={{
                        background: isEval() ? 'rgba(125, 153, 112, 0.2)' : 'var(--amber-500)',
                        color: isEval() ? 'var(--sage)' : 'var(--pasture-900)',
                        border: isEval() ? '1px solid rgba(125, 153, 112, 0.3)' : 'none',
                      }}
                    >
                      Save Changes
                    </button>
                  </div>
                </div>
              </div>
            );
          }}
        </Show>

        {/* Live Node Detail Modal (Read-only) */}
        <Show when={viewingLiveNode()}>
          {(node) => {
            const nodeType = () => node().nodeType;
            const isEval = () => nodeType() === 'eval';
            const isProject = () => nodeType() === 'project';
            const typeLabel = () => (isEval() ? 'Eval' : isProject() ? 'Project' : 'Task');

            const statusLabel = () => {
              switch (node().status) {
                case 'pending': return 'Pending';
                case 'working': return 'Working';
                case 'done': return 'Done';
                case 'failed': return 'Failed';
                default: return node().status;
              }
            };

            const statusColor = () => {
              switch (node().status) {
                case 'pending': return 'var(--wool-500)';
                case 'working': return 'var(--amber-500)';
                case 'done': return 'var(--sage)';
                case 'failed': return 'var(--terra)';
                default: return 'var(--wool-500)';
              }
            };

            return (
              <div
                class="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
                onClick={(e) => { if (e.target === e.currentTarget) { setViewingLiveNode(null); setSelectedLiveNodeId(null); } }}
              >
                <div
                  class="w-[420px] rounded-lg shadow-xl"
                  style={{
                    background: 'var(--pasture-800)',
                    border: '1px solid var(--pasture-600)',
                  }}
                >
                  {/* Header */}
                  <div class="p-4 border-b border-pasture-600 flex items-start justify-between">
                    <div class="flex items-center gap-3">
                      <div
                        class="w-10 h-10 rounded-lg flex items-center justify-center flex-shrink-0"
                        style={{
                          background: isEval() ? 'rgba(125, 153, 112, 0.15)' : 'rgba(212, 165, 116, 0.12)',
                          border: `1px solid ${isEval() ? 'rgba(125, 153, 112, 0.25)' : 'rgba(212, 165, 116, 0.2)'}`,
                        }}
                      >
                        <Show when={isEval()} fallback={
                          <Show when={isProject()} fallback={
                            <svg class="w-5 h-5" style={{ color: 'var(--amber-500)' }} fill="none" stroke="currentColor" viewBox="0 0 24 24">
                              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" />
                            </svg>
                          }>
                            <svg class="w-5 h-5" style={{ color: 'var(--amber-500)' }} fill="none" stroke="currentColor" viewBox="0 0 24 24">
                              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
                            </svg>
                          </Show>
                        }>
                          <svg class="w-5 h-5" style={{ color: 'var(--sage)' }} fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                          </svg>
                        </Show>
                      </div>
                      <div>
                        <div class="flex items-center gap-2">
                          <span
                            class="text-[10px] font-medium uppercase tracking-wider"
                            style={{ color: isEval() ? 'var(--sage)' : 'var(--amber-500)', opacity: 0.8 }}
                          >
                            {typeLabel()}
                          </span>
                          <span class="text-[9px] font-medium px-1.5 py-0.5 rounded" style={{ color: statusColor(), background: `color-mix(in srgb, ${statusColor()} 15%, transparent)` }}>
                            {statusLabel()}
                          </span>
                        </div>
                        <h2 class="text-sm font-semibold text-wool-100 -mt-0.5">
                          {node().name || 'Untitled'}
                        </h2>
                      </div>
                    </div>
                    <button
                      onClick={() => { setViewingLiveNode(null); setSelectedLiveNodeId(null); }}
                      class="p-1 rounded text-wool-500 hover:text-wool-300 hover:bg-white/5"
                    >
                      <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
                      </svg>
                    </button>
                  </div>

                  {/* Content */}
                  <div class="p-4 space-y-4">
                    {/* Description/Content (read-only) */}
                    <Show when={node().content}>
                      <div class="space-y-1.5">
                        <label class="text-xs font-medium text-wool-300">
                          {isEval() ? 'Acceptance Criteria' : 'Description'}
                        </label>
                        <div
                          class="w-full px-3 py-2 rounded-md text-sm bg-pasture-900/50 border border-pasture-700 text-wool-200 whitespace-pre-wrap"
                          style={{ 'min-height': '60px' }}
                        >
                          {node().content}
                        </div>
                      </div>
                    </Show>

                    {/* Validates (eval only) */}
                    <Show when={isEval() && node().validates.length > 0}>
                      <div class="space-y-1.5">
                        <label class="text-xs font-medium" style={{ color: 'var(--sage)' }}>
                          Validates Tasks
                        </label>
                        <div class="flex flex-wrap gap-1.5">
                          <For each={node().validates}>
                            {(taskId) => (
                              <span
                                class="px-2 py-0.5 rounded text-xs font-mono"
                                style={{ background: 'rgba(125, 153, 112, 0.15)', color: 'var(--sage)', border: '1px solid rgba(125, 153, 112, 0.3)' }}
                              >
                                {taskId}
                              </span>
                            )}
                          </For>
                        </div>
                      </div>
                    </Show>

                    {/* Commit SHA (if completed) */}
                    <Show when={node().lastCommitSha}>
                      <div class="space-y-1.5">
                        <label class="text-xs font-medium text-wool-300">Last Commit</label>
                        <div class="flex items-center gap-2">
                          <code class="px-2 py-1 rounded text-xs font-mono bg-pasture-900/50 border border-pasture-700 text-wool-300">
                            {node().lastCommitSha?.slice(0, 7)}
                          </code>
                        </div>
                      </div>
                    </Show>

                    {/* Completed at (if done) */}
                    <Show when={node().completedAt}>
                      <div class="space-y-1.5">
                        <label class="text-xs font-medium text-wool-300">Completed</label>
                        <div class="text-xs text-wool-400">
                          {new Date(node().completedAt!).toLocaleString()}
                        </div>
                      </div>
                    </Show>

                    {/* Node ID */}
                    <div class="space-y-1.5">
                      <label class="text-xs font-medium text-wool-500">Node ID</label>
                      <code class="block px-2 py-1 rounded text-[10px] font-mono bg-pasture-900/30 border border-pasture-700/50 text-wool-500 truncate">
                        {node().id}
                      </code>
                    </div>
                  </div>

                  {/* Footer */}
                  <div class="px-4 py-3 border-t border-pasture-600 flex justify-end">
                    <button
                      onClick={() => { setViewingLiveNode(null); setSelectedLiveNodeId(null); }}
                      class="px-3 py-1.5 rounded-md text-xs font-medium text-wool-400 hover:text-wool-200 hover:bg-white/5"
                    >
                      Close
                    </button>
                  </div>
                </div>
              </div>
            );
          }}
        </Show>

        {/* Loading overlay */}
        <Show when={delta.loading()}>
          <div class="absolute inset-0 bg-black/30 flex items-center justify-center">
            <div
              class="w-6 h-6 rounded-full animate-spin"
              style={{ border: '2px solid rgba(212, 165, 116, 0.2)', 'border-top-color': 'var(--amber-500)' }}
            />
          </div>
        </Show>
      </div>
    </Show>
  );
};

export default SpecBoard;
