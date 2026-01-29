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
const MAX_NODE_WIDTH = 140;
const NODE_HEIGHT = 26;
const NODE_LINE_HEIGHT = 12;
const SIBLING_GAP = 6;      // Vertical gap between siblings
const LEVEL_GAP = 32;       // Horizontal gap between parent and children
const TREE_PADDING = 24;
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
// Horizontal Tree Layout Algorithm
//
// Root on LEFT, children flow RIGHT. Siblings stack VERTICALLY.
// This is far more space-efficient for wide trees with many branches.
// =============================================================================

interface LayoutNode {
  id: string;
  name: string;
  children: LayoutNode[];
  x: number;       // Horizontal position (depth)
  y: number;       // Vertical position (sibling order)
  width: number;
  height: number;
  lines: string[];
  subtreeHeight: number;  // Total height of this subtree
}

function layoutTree<T extends { id: string; name: string; children: T[] }>(
  roots: T[],
  _startX: number = 0
): { positions: Map<string, NodePosition>; width: number; height: number } {
  const positions = new Map<string, NodePosition>();

  if (roots.length === 0) {
    return { positions, width: MIN_NODE_WIDTH, height: NODE_HEIGHT };
  }

  // Convert input tree to layout nodes
  function toLayoutNode(node: T): LayoutNode {
    const dims = calcNodeDimensions(node.name);
    return {
      id: node.id,
      name: node.name,
      children: node.children.map(c => toLayoutNode(c)),
      x: 0,
      y: 0,
      width: dims.width,
      height: dims.height,
      lines: dims.lines,
      subtreeHeight: 0,
    };
  }

  // First pass: compute subtree heights (post-order)
  function computeSubtreeHeights(node: LayoutNode): number {
    if (node.children.length === 0) {
      node.subtreeHeight = node.height;
      return node.subtreeHeight;
    }

    let totalChildrenHeight = 0;
    for (const child of node.children) {
      totalChildrenHeight += computeSubtreeHeights(child);
    }
    // Add gaps between children
    totalChildrenHeight += (node.children.length - 1) * SIBLING_GAP;

    // Subtree height is max of node height and children's total height
    node.subtreeHeight = Math.max(node.height, totalChildrenHeight);
    return node.subtreeHeight;
  }

  // Second pass: assign positions (pre-order)
  function assignPositions(node: LayoutNode, depth: number, topY: number): void {
    // X position based on depth (horizontal)
    node.x = TREE_PADDING + depth * (MAX_NODE_WIDTH + LEVEL_GAP);

    if (node.children.length === 0) {
      // Leaf node: center vertically in its allocated space
      node.y = topY + node.subtreeHeight / 2 - node.height / 2;
    } else {
      // Internal node: position children first, then center parent
      let childY = topY;

      // If children total height < subtree height, center children
      let totalChildrenHeight = 0;
      for (const child of node.children) {
        totalChildrenHeight += child.subtreeHeight;
      }
      totalChildrenHeight += (node.children.length - 1) * SIBLING_GAP;

      if (totalChildrenHeight < node.subtreeHeight) {
        childY = topY + (node.subtreeHeight - totalChildrenHeight) / 2;
      }

      for (const child of node.children) {
        assignPositions(child, depth + 1, childY);
        childY += child.subtreeHeight + SIBLING_GAP;
      }

      // Center parent vertically among its children
      const firstChildCenter = node.children[0].y + node.children[0].height / 2;
      const lastChild = node.children[node.children.length - 1];
      const lastChildCenter = lastChild.y + lastChild.height / 2;
      const childrenMidpoint = (firstChildCenter + lastChildCenter) / 2;

      node.y = childrenMidpoint - node.height / 2;
    }

    // Store position
    positions.set(node.id, {
      x: node.x,
      y: node.y,
      width: node.width,
      height: node.height,
      lines: node.lines,
    });
  }

  // Layout all root trees
  const layoutRoots = roots.map(r => toLayoutNode(r));

  // Compute heights for all roots
  for (const root of layoutRoots) {
    computeSubtreeHeights(root);
  }

  // Assign positions, stacking roots vertically
  let currentY = TREE_PADDING;
  for (const root of layoutRoots) {
    assignPositions(root, 0, currentY);
    currentY += root.subtreeHeight + SIBLING_GAP * 2;
  }

  // Compute bounds
  const allPositions = Array.from(positions.values());
  if (allPositions.length === 0) {
    return { positions, width: MIN_NODE_WIDTH, height: NODE_HEIGHT };
  }

  const maxX = Math.max(...allPositions.map(p => p.x + p.width));
  const maxY = Math.max(...allPositions.map(p => p.y + p.height));

  return {
    positions,
    width: maxX + TREE_PADDING,
    height: maxY + TREE_PADDING,
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

  // Style based on node type: Project (amber), Task (neutral), Eval (sage/dashed)
  const getStyles = () => {
    if (isProject()) {
      return {
        bg: 'rgba(45, 42, 38, 0.95)',
        border: props.selected ? 'rgba(212, 165, 116, 0.6)' : 'rgba(212, 165, 116, 0.3)',
        borderStyle: 'solid',
        textColor: 'var(--wool-100)',
        accent: 'var(--amber-500)',
      };
    }
    if (isEval()) {
      // Eval nodes: sage tint, dashed border
      const baseBorder = isNew() ? 'rgba(125, 153, 112, 0.6)'
        : isModified() ? 'rgba(125, 153, 112, 0.5)'
        : props.selected ? 'rgba(125, 153, 112, 0.5)'
        : 'rgba(125, 153, 112, 0.3)';
      return {
        bg: isNew() || isModified() ? 'rgba(125, 153, 112, 0.12)' : 'rgba(125, 153, 112, 0.06)',
        border: baseBorder,
        borderStyle: 'dashed',
        textColor: 'var(--wool-200)',
        accent: 'var(--sage)',
      };
    }
    // Task nodes: standard styling
    const baseBorder = isNew() ? 'rgba(125, 153, 112, 0.6)'
      : isModified() ? 'rgba(212, 165, 116, 0.6)'
      : props.selected ? 'rgba(212, 165, 116, 0.5)'
      : 'rgba(64, 64, 64, 0.4)';
    return {
      bg: isNew() ? 'rgba(125, 153, 112, 0.08)'
        : isModified() ? 'rgba(212, 165, 116, 0.08)'
        : 'rgba(36, 36, 36, 0.9)',
      border: baseBorder,
      borderStyle: 'solid',
      textColor: 'var(--wool-200)',
      accent: 'var(--amber-500)',
    };
  };

  const styles = () => getStyles();
  const isMultiLine = () => props.position.lines.length > 1;

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
        class={`h-full flex gap-1.5 px-2 ${isMultiLine() ? 'flex-col justify-center py-1' : 'items-center'}`}
        style={{
          background: styles().bg,
          border: `1px ${styles().borderStyle} ${styles().border}`,
          'border-radius': isEval() ? '4px' : '6px',
          'box-shadow': props.selected ? '0 2px 8px rgba(0,0,0,0.25)' : undefined,
        }}
      >
        {/* Left accent bar for type (only for project and eval) */}
        <Show when={isProject() || isEval()}>
          <div
            class="absolute left-0 top-1.5 bottom-1.5 w-0.5 rounded-full"
            style={{ background: styles().accent, opacity: 0.7 }}
          />
        </Show>

        {/* Eval checkmark icon */}
        <Show when={isEval() && !isMultiLine()}>
          <svg class="w-3 h-3 flex-shrink-0" style={{ color: 'var(--sage)', opacity: 0.7 }} fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2.5" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
        </Show>

        {/* Name lines */}
        <div class={`flex-1 min-w-0 ${isMultiLine() ? 'flex flex-col gap-0.5' : ''}`}>
          <For each={props.position.lines}>
            {(line, i) => (
              <div class="flex items-center gap-1.5">
                <Show when={isEval() && isMultiLine() && i() === 0}>
                  <svg class="w-3 h-3 flex-shrink-0" style={{ color: 'var(--sage)', opacity: 0.7 }} fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2.5" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                  </svg>
                </Show>
                <span
                  class="text-[10px] font-medium truncate leading-tight"
                  style={{ color: styles().textColor }}
                >
                  {line}
                </span>
              </div>
            )}
          </For>
        </div>
      </div>
    </div>
  );
};

const LiveNodeCard: Component<{
  node: LiveNodeTree;
  position: NodePosition;
  diff: TreeDiff | null;
  showDelta: boolean;
}> = (props) => {
  const isDeleted = () =>
    props.showDelta && props.diff?.deletedNodes.some((n) => n.id === props.node.id);
  const isEval = () => props.node.nodeType === 'eval';
  const isProject = () => props.node.nodeType === 'project';

  const statusStyles: Record<LiveNodeStatus, { border: string; bg: string; dot: string }> = {
    pending: {
      border: 'rgba(90, 85, 80, 0.4)',
      bg: 'rgba(30, 30, 30, 0.8)',
      dot: 'var(--wool-600)',
    },
    working: {
      border: 'rgba(212, 165, 116, 0.5)',
      bg: 'rgba(212, 165, 116, 0.08)',
      dot: 'var(--amber-500)',
    },
    done: {
      border: 'rgba(125, 153, 112, 0.5)',
      bg: 'rgba(125, 153, 112, 0.08)',
      dot: 'var(--sage)',
    },
    failed: {
      border: 'rgba(196, 92, 74, 0.5)',
      bg: 'rgba(196, 92, 74, 0.08)',
      dot: 'var(--terra)',
    },
  };

  const baseStyle = () => statusStyles[props.node.status] || statusStyles.pending;
  const isWorking = () => props.node.status === 'working';
  const isMultiLine = () => props.position.lines.length > 1;

  // Eval nodes get dashed border and sage tint overlay
  const getBorderStyle = () => isEval() ? 'dashed' : 'solid';
  const getBg = () => {
    if (isDeleted()) return 'rgba(196, 92, 74, 0.08)';
    if (isEval()) {
      // Blend eval sage with status color
      const statusBg = baseStyle().bg;
      return props.node.status === 'pending' ? 'rgba(125, 153, 112, 0.04)' : statusBg;
    }
    return baseStyle().bg;
  };

  return (
    <div
      class="absolute"
      style={{
        left: `${props.position.x}px`,
        top: `${props.position.y}px`,
        width: `${props.position.width}px`,
        height: `${props.position.height}px`,
      }}
    >
      <div
        class={`h-full flex gap-1.5 px-2 ${isDeleted() ? 'opacity-40' : ''} ${isMultiLine() ? 'flex-col justify-center py-1' : 'items-center'}`}
        style={{
          background: getBg(),
          border: `1px ${getBorderStyle()} ${isDeleted() ? 'rgba(196, 92, 74, 0.4)' : baseStyle().border}`,
          'border-radius': isEval() ? '4px' : '6px',
        }}
      >
        {/* Left accent for project/eval */}
        <Show when={isProject() || isEval()}>
          <div
            class="absolute left-0 top-1.5 bottom-1.5 w-0.5 rounded-full"
            style={{
              background: isEval() ? 'var(--sage)' : 'var(--amber-500)',
              opacity: 0.5,
            }}
          />
        </Show>

        {/* Status dot for single line */}
        <Show when={!isMultiLine()}>
          <div class="relative flex-shrink-0">
            <div
              class={`w-1.5 h-1.5 rounded-full ${isWorking() ? 'animate-pulse' : ''}`}
              style={{ background: baseStyle().dot }}
            />
          </div>
        </Show>

        {/* Eval icon (shown alongside status dot) */}
        <Show when={isEval() && !isMultiLine()}>
          <svg class="w-2.5 h-2.5 flex-shrink-0 -ml-0.5" style={{ color: 'var(--sage)', opacity: 0.6 }} fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2.5" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
        </Show>

        {/* Name lines */}
        <div class={`flex-1 min-w-0 ${isMultiLine() ? 'flex flex-col gap-0.5' : ''}`}>
          <For each={props.position.lines}>
            {(line, i) => (
              <div class="flex items-center gap-1.5">
                <Show when={isMultiLine() && i() === 0}>
                  <div class="flex items-center gap-1">
                    <div
                      class={`w-1.5 h-1.5 rounded-full ${isWorking() ? 'animate-pulse' : ''}`}
                      style={{ background: baseStyle().dot }}
                    />
                    <Show when={isEval()}>
                      <svg class="w-2.5 h-2.5 flex-shrink-0" style={{ color: 'var(--sage)', opacity: 0.6 }} fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2.5" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                      </svg>
                    </Show>
                  </div>
                </Show>
                <span class="text-[10px] font-medium text-wool-300 truncate leading-tight">
                  {line}
                </span>
              </div>
            )}
          </For>
        </div>
      </div>

      {/* Left edge indicator for deleted */}
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
// Tree Connectors (SVG)
// =============================================================================

const TreeConnectors: Component<{
  positions: Map<string, NodePosition>;
  tree: { id: string; children: { id: string; children: any[] }[] }[];
  color: string;
  dashed?: boolean;
}> = (props) => {
  const paths = createMemo(() => {
    const result: { from: NodePosition; to: NodePosition }[] = [];

    function collectEdges(node: { id: string; children: { id: string; children: any[] }[] }) {
      const parentPos = props.positions.get(node.id);
      if (!parentPos) return;

      for (const child of node.children) {
        const childPos = props.positions.get(child.id);
        if (childPos) {
          result.push({ from: parentPos, to: childPos });
        }
        collectEdges(child);
      }
    }

    for (const root of props.tree) {
      collectEdges(root);
    }

    return result;
  });

  return (
    <svg class="absolute inset-0 pointer-events-none overflow-visible" style={{ 'z-index': 0 }}>
      <For each={paths()}>
        {(edge) => {
          // Horizontal tree: parent right edge → child left edge
          const x1 = edge.from.x + edge.from.width;
          const y1 = edge.from.y + edge.from.height / 2;
          const x2 = edge.to.x;
          const y2 = edge.to.y + edge.to.height / 2;
          const midX = (x1 + x2) / 2;

          return (
            <path
              d={`M ${x1} ${y1} C ${midX} ${y1}, ${midX} ${y2}, ${x2} ${y2}`}
              fill="none"
              stroke={props.color}
              stroke-width="1"
              stroke-dasharray={props.dashed ? '3 2' : undefined}
              opacity="0.4"
            />
          );
        }}
      </For>
    </svg>
  );
};

// =============================================================================
// Main Component
// =============================================================================

export const SpecBoard: Component = () => {
  const project = useProject();
  const delta = useDelta();

  // Selection state
  const [selectedNodeId, setSelectedNodeId] = createSignal<string | null>(null);

  // Edit modal state
  const [editingNode, setEditingNode] = createSignal<DraftNodeTree | null>(null);
  const [editForm, setEditForm] = createSignal({ name: '', content: '', validates: '' });

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

  // Pan and zoom state
  const [zoom, setZoom] = createSignal(1);
  const [pan, setPan] = createSignal({ x: 0, y: 0 });
  const [isPanning, setIsPanning] = createSignal(false);
  const [panStart, setPanStart] = createSignal({ x: 0, y: 0 });
  let canvasRef: HTMLDivElement | undefined;

  // Computed layouts
  const draftLayout = createMemo(() => layoutTree(delta.draftTree(), 0));
  const liveLayout = createMemo(() => layoutTree(delta.liveTree(), 0));

  // Check if we have a live tree (post-dispatch)
  const hasLiveTree = () => delta.liveTree().length > 0;

  // Section box padding for labels
  const SECTION_PADDING = 28; // Space for label at top
  const SECTION_GAP = 16;     // Gap between sections (horizontal)

  // Total canvas dimensions (side-by-side: Live left, Draft right)
  const canvasDimensions = createMemo(() => {
    const draft = draftLayout();
    const live = liveLayout();

    if (!hasLiveTree()) {
      // Single tree (draft only) - add section box when there's content
      const hasDraft = delta.draftTree().length > 0;
      return {
        width: draft.width + (hasDraft ? TREE_PADDING : 0),
        height: Math.max(draft.height, NODE_HEIGHT) + (hasDraft ? SECTION_PADDING : 0),
        // Draft positioning
        draftBoxLeft: 0,
        draftBoxWidth: draft.width + TREE_PADDING,
        draftContentLeft: 0,
        draftContentTop: hasDraft ? SECTION_PADDING : 0,
        draftHeight: draft.height,
        // No live
        liveBoxLeft: 0,
        liveBoxWidth: 0,
        liveContentLeft: 0,
        liveContentTop: 0,
        liveHeight: 0,
        showBothSections: false,
      };
    }

    // Both trees present - side by side (Live LEFT, Draft RIGHT)
    const liveBoxWidth = live.width + TREE_PADDING;
    const draftBoxWidth = draft.width + TREE_PADDING;
    const totalWidth = liveBoxWidth + SECTION_GAP + draftBoxWidth;
    const maxHeight = Math.max(live.height, draft.height) + SECTION_PADDING;

    return {
      width: totalWidth,
      height: maxHeight,
      // Live section (LEFT)
      liveBoxLeft: 0,
      liveBoxWidth: liveBoxWidth,
      liveContentLeft: 0,
      liveContentTop: SECTION_PADDING,
      liveHeight: live.height,
      // Draft section (RIGHT)
      draftBoxLeft: liveBoxWidth + SECTION_GAP,
      draftBoxWidth: draftBoxWidth,
      draftContentLeft: liveBoxWidth + SECTION_GAP,
      draftContentTop: SECTION_PADDING,
      draftHeight: draft.height,
      showBothSections: true,
    };
  });

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
    if (!delta.hasDiff()) {
      window.toast?.info('No changes to dispatch');
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

  // Pan and zoom handlers
  const handleWheel = (e: WheelEvent) => {
    e.preventDefault();
    const delta = e.deltaY > 0 ? 0.9 : 1.1;
    const newZoom = Math.max(0.25, Math.min(2, zoom() * delta));

    // Zoom toward cursor position
    if (canvasRef) {
      const rect = canvasRef.getBoundingClientRect();
      const cursorX = e.clientX - rect.left;
      const cursorY = e.clientY - rect.top;

      const currentPan = pan();
      const scale = newZoom / zoom();

      setPan({
        x: cursorX - (cursorX - currentPan.x) * scale,
        y: cursorY - (cursorY - currentPan.y) * scale,
      });
    }

    setZoom(newZoom);
  };

  const handleMouseDown = (e: MouseEvent) => {
    // Middle mouse button or space+left click for panning
    if (e.button === 1 || (e.button === 0 && e.altKey)) {
      e.preventDefault();
      setIsPanning(true);
      setPanStart({ x: e.clientX - pan().x, y: e.clientY - pan().y });
    }
  };

  const handleMouseMove = (e: MouseEvent) => {
    if (isPanning()) {
      setPan({
        x: e.clientX - panStart().x,
        y: e.clientY - panStart().y,
      });
    }
  };

  const handleMouseUp = () => {
    setIsPanning(false);
  };

  const resetView = () => {
    setZoom(1);
    setPan({ x: 0, y: 0 });
  };

  const zoomIn = () => setZoom(z => Math.min(2, z * 1.2));
  const zoomOut = () => setZoom(z => Math.max(0.25, z / 1.2));

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
            {/* Run status if active */}
            <Show when={delta.projectRun()}>
              <span class="text-[10px] text-wool-600">Run:</span>
              <span class="text-[10px] text-amber-400 font-mono">{delta.projectRun()?.runName}</span>
              <span
                class="text-[8px] px-1.5 py-0.5 rounded font-medium uppercase"
                style={{
                  background:
                    delta.projectRun()?.status === 'working'
                      ? 'rgba(212, 165, 116, 0.15)'
                      : delta.projectRun()?.status === 'paused'
                        ? 'rgba(90, 85, 80, 0.2)'
                        : 'rgba(196, 92, 74, 0.15)',
                  color:
                    delta.projectRun()?.status === 'working'
                      ? 'var(--amber-400)'
                      : delta.projectRun()?.status === 'paused'
                        ? 'var(--wool-500)'
                        : 'var(--terra)',
                }}
              >
                {delta.projectRun()?.status}
              </span>
            </Show>
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
          {/* Zoom controls */}
          <div class="absolute bottom-3 right-3 z-20 flex items-center gap-1 px-1 py-0.5 rounded"
               style={{ background: 'rgba(30, 30, 30, 0.8)', border: '1px solid rgba(64, 64, 64, 0.4)' }}>
            <button onClick={zoomOut} class="p-1 text-wool-400 hover:text-wool-200" title="Zoom out">
              <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M20 12H4" />
              </svg>
            </button>
            <span class="text-[10px] text-wool-500 min-w-[36px] text-center">{Math.round(zoom() * 100)}%</span>
            <button onClick={zoomIn} class="p-1 text-wool-400 hover:text-wool-200" title="Zoom in">
              <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
              </svg>
            </button>
            <button onClick={resetView} class="p-1 text-wool-400 hover:text-wool-200 ml-1" title="Reset view">
              <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 8V4m0 0h4M4 4l5 5m11-1V4m0 0h-4m4 0l-5 5M4 16v4m0 0h4m-4 0l5-5m11 5v-4m0 4h-4m4 0l-5-5" />
              </svg>
            </button>
          </div>

          <Show
            when={delta.draftTree().length > 0 || delta.liveTree().length > 0}
            fallback={<EmptyTreeState />}
          >
            {/* Transformed canvas container */}
            <div
              class="absolute origin-top-left"
              style={{
                transform: `translate(${pan().x}px, ${pan().y}px) scale(${zoom()})`,
                width: `${canvasDimensions().width}px`,
                height: `${canvasDimensions().height}px`,
              }}
              onContextMenu={(e) => {
                if ((e.target as HTMLElement).classList.contains('absolute')) {
                  handleContextMenu(e, null);
                }
              }}
            >
              {/* Live Tree Section (LEFT when both present) */}
              <Show when={hasLiveTree()}>
                {/* Section box */}
                <div
                  class="absolute rounded-lg"
                  style={{
                    left: `${canvasDimensions().liveBoxLeft}px`,
                    top: '0',
                    width: `${canvasDimensions().liveBoxWidth}px`,
                    height: `${canvasDimensions().height}px`,
                    border: '1px dashed rgba(90, 85, 80, 0.25)',
                    background: 'rgba(30, 30, 30, 0.3)',
                  }}
                >
                  {/* Section label */}
                  <div
                    class="absolute text-[9px] font-medium uppercase tracking-wider px-2 py-0.5 rounded"
                    style={{
                      left: '8px',
                      top: '6px',
                      color: 'var(--wool-500)',
                      background: 'rgba(30, 30, 30, 0.8)',
                    }}
                  >
                    Live
                  </div>
                </div>
                {/* Tree content */}
                <div
                  class="absolute"
                  style={{
                    left: `${canvasDimensions().liveContentLeft}px`,
                    top: `${canvasDimensions().liveContentTop}px`,
                    width: `${liveLayout().width}px`,
                    height: `${canvasDimensions().liveHeight}px`,
                  }}
                >
                  <TreeConnectors
                    positions={liveLayout().positions}
                    tree={delta.liveTree()}
                    color="rgb(90, 85, 80)"
                    dashed
                  />
                  <For each={flattenLiveTree(delta.liveTree())}>
                    {(node) => {
                      const pos = () => liveLayout().positions.get(node.id);
                      return (
                        <Show when={pos()}>
                          <LiveNodeCard
                            node={node}
                            position={pos()!}
                            diff={delta.diff()}
                            showDelta={delta.showDeltaIndicators()}
                          />
                        </Show>
                      );
                    }}
                  </For>
                </div>
              </Show>

              {/* Draft Tree Section (RIGHT when both present, or full width) */}
              <Show when={delta.draftTree().length > 0}>
                {/* Section box */}
                <div
                  class="absolute rounded-lg"
                  style={{
                    left: `${canvasDimensions().draftBoxLeft}px`,
                    top: '0',
                    width: `${canvasDimensions().draftBoxWidth}px`,
                    height: `${canvasDimensions().height}px`,
                    border: '1px dashed rgba(212, 165, 116, 0.2)',
                    background: 'rgba(36, 34, 30, 0.3)',
                  }}
                >
                  {/* Section label */}
                  <div
                    class="absolute text-[9px] font-medium uppercase tracking-wider px-2 py-0.5 rounded"
                    style={{
                      left: '8px',
                      top: '6px',
                      color: 'var(--amber-600)',
                      background: 'rgba(30, 30, 30, 0.8)',
                    }}
                  >
                    Draft
                  </div>
                </div>
                {/* Tree content */}
                <div
                  class="absolute"
                  style={{
                    left: `${canvasDimensions().draftContentLeft}px`,
                    top: `${canvasDimensions().draftContentTop}px`,
                    width: `${draftLayout().width}px`,
                    height: `${canvasDimensions().draftHeight}px`,
                  }}
                >
                  <TreeConnectors
                    positions={draftLayout().positions}
                    tree={delta.draftTree()}
                    color="rgb(212, 165, 116)"
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
                            onSelect={() => setSelectedNodeId(node.id)}
                            onDoubleClick={() => handleDoubleClick(node)}
                            onContextMenu={(e) => handleContextMenu(e, node)}
                          />
                        </Show>
                      );
                    }}
                  </For>
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
          <dialog
            open
            class="fixed inset-0 z-[100] m-0 h-full w-full max-w-none max-h-none bg-black/50 flex items-center justify-center"
            onClick={() => setShowNewPrompt(false)}
          >
            <div
              class="w-64 rounded-lg overflow-hidden"
              style={{
                background: 'linear-gradient(180deg, #2a2825 0%, #1f1d1a 100%)',
                border: '1px solid rgba(64, 64, 64, 0.5)',
                'box-shadow': '0 16px 48px rgba(0,0,0,0.5)',
              }}
              onClick={(e) => e.stopPropagation()}
            >
              <div class="px-3 py-2.5 border-b border-white/5">
                <h3 class="text-[13px] font-semibold text-wool-100">
                  New {newNodeType() === 'eval' ? 'Eval' : 'Task'}
                </h3>
              </div>
              <div class="p-3">
                <form onSubmit={(e) => { e.preventDefault(); handleCreateNode(); }}>
                  <input
                    ref={newNodeInputRef}
                    type="text"
                    value={newNodeName()}
                    onInput={(e) => setNewNodeName(e.currentTarget.value)}
                    onKeyDown={(e) => { if (e.key === 'Escape') setShowNewPrompt(false); }}
                    placeholder={`${newNodeType() === 'eval' ? 'Eval' : 'Task'} name...`}
                    class="w-full px-2.5 py-1.5 rounded bg-pasture-800 border border-pasture-600/50 text-[12px] text-wool-100 placeholder-wool-600 focus:outline-none focus:border-amber-500/40"
                  />
                </form>
              </div>
              <div class="px-3 py-2.5 border-t border-white/5 flex justify-end gap-2">
                <button
                  onClick={() => setShowNewPrompt(false)}
                  class="px-2.5 py-1 rounded text-[11px] font-medium text-wool-500 hover:text-wool-300 hover:bg-white/5"
                >
                  Cancel
                </button>
                <button
                  onClick={handleCreateNode}
                  disabled={!newNodeName().trim()}
                  class="px-2.5 py-1 rounded text-[11px] font-medium disabled:opacity-30"
                  style={{
                    background: newNodeType() === 'eval' ? 'rgba(125, 153, 112, 0.2)' : 'rgba(212, 165, 116, 0.2)',
                    border: newNodeType() === 'eval' ? '1px solid rgba(125, 153, 112, 0.3)' : '1px solid rgba(212, 165, 116, 0.3)',
                    color: newNodeType() === 'eval' ? 'var(--sage)' : 'var(--amber-400)',
                  }}
                >
                  Create
                </button>
              </div>
            </div>
          </dialog>
        </Show>

        {/* Edit Modal */}
        <Show when={editingNode()}>
          <dialog
            open
            class="fixed inset-0 z-[100] m-0 h-full w-full max-w-none max-h-none bg-black/50 flex items-center justify-center"
            onClick={() => setEditingNode(null)}
          >
            <div
              class="w-80 rounded-lg overflow-hidden"
              style={{
                background: 'linear-gradient(180deg, #2a2825 0%, #1f1d1a 100%)',
                border: '1px solid rgba(64, 64, 64, 0.5)',
                'box-shadow': '0 16px 48px rgba(0,0,0,0.5)',
              }}
              onClick={(e) => e.stopPropagation()}
            >
              <div class="px-3 py-2.5 border-b border-white/5">
                <h3 class="text-[13px] font-semibold text-wool-100">
                  Edit {editingNode()!.nodeType === 'eval' ? 'Eval' : editingNode()!.nodeType === 'project' ? 'Project' : 'Task'}
                </h3>
              </div>
              <div class="p-3 space-y-2.5">
                <div>
                  <label class="block text-[10px] font-medium text-wool-500 mb-1 uppercase tracking-wide">Name</label>
                  <input
                    type="text"
                    value={editForm().name}
                    onInput={(e) => setEditForm((f) => ({ ...f, name: e.currentTarget.value }))}
                    class="w-full px-2.5 py-1.5 rounded bg-pasture-800 border border-pasture-600/50 text-[12px] text-wool-100 focus:outline-none focus:border-amber-500/40"
                  />
                </div>
                <div>
                  <label class="block text-[10px] font-medium text-wool-500 mb-1 uppercase tracking-wide">Content</label>
                  <textarea
                    value={editForm().content}
                    onInput={(e) => setEditForm((f) => ({ ...f, content: e.currentTarget.value }))}
                    rows={3}
                    placeholder="Description..."
                    class="w-full px-2.5 py-1.5 rounded bg-pasture-800 border border-pasture-600/50 text-[12px] text-wool-100 placeholder-wool-600 focus:outline-none focus:border-amber-500/40 resize-none"
                  />
                </div>
                <Show when={editingNode()!.nodeType === 'eval'}>
                  <div>
                    <label class="block text-[10px] font-medium text-sage/70 mb-1 uppercase tracking-wide">Validates</label>
                    <input
                      type="text"
                      value={editForm().validates}
                      onInput={(e) => setEditForm((f) => ({ ...f, validates: e.currentTarget.value }))}
                      placeholder="task-1, task-2"
                      class="w-full px-2.5 py-1.5 rounded bg-pasture-800 border border-sage/30 text-[12px] text-wool-100 placeholder-wool-600 focus:outline-none focus:border-sage/50 font-mono"
                    />
                  </div>
                </Show>
              </div>
              <div class="px-3 py-2.5 border-t border-white/5 flex justify-end gap-2">
                <button
                  onClick={() => setEditingNode(null)}
                  class="px-2.5 py-1 rounded text-[11px] font-medium text-wool-500 hover:text-wool-300 hover:bg-white/5"
                >
                  Cancel
                </button>
                <button
                  onClick={handleSaveEdit}
                  class="px-2.5 py-1 rounded text-[11px] font-medium"
                  style={{
                    background: 'rgba(212, 165, 116, 0.2)',
                    border: '1px solid rgba(212, 165, 116, 0.3)',
                    color: 'var(--amber-400)',
                  }}
                >
                  Save
                </button>
              </div>
            </div>
          </dialog>
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
