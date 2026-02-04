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
import { invoke } from '@tauri-apps/api/core';
import { useProject, useRoute } from '../../stores';
import { useDelta } from '../../stores/delta-context';
import { Icon } from '../shared';
import { CanvasToolbar } from '../layout/CanvasToolbar';
import { TaskEditorModal } from './TaskEditorModal';
import { DeliveryDialog } from './DeliveryDialog';
import { ForkRouteDialog } from './ForkRouteDialog';
import { MarkdownContent } from '../docs/MarkdownContent';
import { computeElkLayout, type LayoutInputNode, type ElkLayoutResult } from '../../lib/elk-layout';
import type {
  DraftNodeTree,
  LiveNodeTree,
  TreeDiff,
  NodeType,
  LiveNodeStatus,
} from '../../lib/types';

// =============================================================================
// Layout Constants
// =============================================================================

// UI-only constants (not used for layout computation)
const DIVIDER_WIDTH = 40;

// Text rendering constants (must match what Rust uses)
const CHAR_WIDTH = 5.8;
const TEXT_PADDING = 20;

interface NodePosition {
  x: number;
  y: number;
  width: number;
  height: number;
  lines: string[]; // Wrapped text lines
}

// =============================================================================
// Helper: Wrap text to fit within a given width
// =============================================================================

/** Wrap text to fit within the given pixel width (from Rust layout) */
function wrapTextToWidth(name: string, width: number): string[] {
  // Derive max chars from width: width = chars * CHAR_WIDTH + TEXT_PADDING
  const maxChars = Math.floor((width - TEXT_PADDING) / CHAR_WIDTH);
  const charsPerLine = Math.max(8, maxChars); // Minimum 8 chars

  // If name fits on one line, use single line
  if (name.length <= charsPerLine) {
    return [name];
  }

  // Wrap text into multiple lines
  const words = name.split(/\s+/);
  const lines: string[] = [];
  let currentLine = '';

  for (const word of words) {
    const testLine = currentLine ? `${currentLine} ${word}` : word;
    if (testLine.length <= charsPerLine) {
      currentLine = testLine;
    } else {
      if (currentLine) lines.push(currentLine);
      // If a single word is too long, truncate it
      currentLine = word.length > charsPerLine ? word.slice(0, charsPerLine - 1) + '…' : word;
    }
  }
  if (currentLine) lines.push(currentLine);

  // Cap at 2 lines max
  if (lines.length > 2) {
    lines.length = 2;
    lines[1] = lines[1].slice(0, -1) + '…';
  }

  return lines;
}

// =============================================================================
// Layout Types
// =============================================================================

/** Computed edge route with waypoints */
interface EdgeRoute {
  from: string;
  to: string;
  type: 'blockedBy' | 'validates' | 'hierarchy';
  waypoints: [number, number][]; // Full path through all waypoints
}

interface LayoutTreeResult {
  positions: Map<string, NodePosition>;
  width: number;
  height: number;
  evalsWithValidates: { id: string; validates: string[] }[];
  edgeRoutes: EdgeRoute[];
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
  const isContainer = () => !isEval() && props.node.children.length > 0;
  const isMultiLine = () => props.position.lines.length > 1;

  // ==========================================================================
  // Visual Hierarchy - Distinguished by SHAPE, BORDER, and GRADIENT
  // NO color for delta status - colors reserved for live status only
  // ==========================================================================

  // EVAL: The gate/checkpoint - DASHED border, sage-tinted (green) to match validates edges
  const evalStyles = () => ({
    bg: 'var(--node-eval-bg)',
    border: props.selected ? 'var(--node-eval-border-selected)' : 'var(--node-eval-border)',
    borderWidth: '1px',
    borderStyle: 'dashed',
    textColor: 'var(--wool-300)',
    radius: '5px',
    boxShadow: props.selected
      ? '0 4px 12px rgba(0,0,0,0.4), inset 0 1px 0 rgba(255,255,255,0.03)'
      : '0 2px 4px rgba(0,0,0,0.25)',
  });

  // CONTAINER: Organizational grouping - ghost style, recedes visually
  const containerStyles = () => ({
    bg: 'transparent',
    border: props.selected ? 'var(--wool-500)' : 'var(--wool-700)',
    borderWidth: '1px',
    borderStyle: 'dashed',
    textColor: 'var(--wool-500)',
    radius: '5px',
    boxShadow: 'none',
  });

  // TASK: The sheep - warmer, more grounded gradient
  const taskStyles = () => ({
    bg: 'var(--node-task-bg)',
    border: props.selected ? 'var(--node-task-border-selected)' : 'var(--node-task-border)',
    borderWidth: '1px',
    borderStyle: 'solid',
    textColor: 'var(--wool-300)',
    radius: '5px',
    boxShadow: props.selected
      ? '0 4px 12px rgba(0,0,0,0.4), inset 0 1px 0 rgba(255,255,255,0.03)'
      : '0 2px 4px rgba(0,0,0,0.25), inset 0 1px 0 rgba(255,255,255,0.02)',
  });

  const styles = () => isEval() ? evalStyles() : isContainer() ? containerStyles() : taskStyles();

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
        class={`h-full flex items-center justify-center px-2 relative ${isMultiLine() ? 'flex-col py-1' : ''}`}
        style={{
          background: styles().bg,
          border: `${styles().borderWidth} ${styles().borderStyle} ${styles().border}`,
          'border-radius': styles().radius,
          'box-shadow': styles().boxShadow,
        }}
      >
        {/* Name text - centered */}
        <div class={`min-w-0 ${isMultiLine() ? 'flex flex-col gap-0.5 items-center' : ''}`}>
          <For each={props.position.lines}>
            {(line) => (
              <span
                class="text-[10px] truncate leading-tight block font-medium text-center"
                style={{ color: styles().textColor }}
              >
                {line}
              </span>
            )}
          </For>
        </div>
      </div>
      {/* Connection anchor indicators (visible on hover) */}
      <div class="absolute left-1/2 -bottom-1 w-1.5 h-1.5 rounded-full bg-wool-600/50 -translate-x-1/2 opacity-0 group-hover:opacity-100 transition-opacity" />
      <div class="absolute left-1/2 -top-1 w-1.5 h-1.5 rounded-full bg-wool-600/50 -translate-x-1/2 opacity-0 group-hover:opacity-100 transition-opacity" />
    </div>
  );
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
  // Container: task with children (organizational grouping - cannot be claimed)
  const isContainer = () => !isEval() && props.node.children.length > 0;
  const isMultiLine = () => props.position.lines.length > 1;
  const isWorking = () => props.node.status === 'working';
  const isDone = () => props.node.status === 'done';
  const isAwaitingEval = () => props.node.status === 'awaiting_eval';
  const isValidated = () => props.node.status === 'validated';
  const isNeedsRepair = () => props.node.status === 'needs_repair';
  const isComplete = () => isDone() || isValidated() || isAwaitingEval();
  const isFailed = () => props.node.status === 'failed';

  // ==========================================================================
  // Visual Hierarchy - Identical to DraftNodeCard
  // Status shown via subtle external glow only (no internal changes)
  // ==========================================================================

  // EVAL: The gate/checkpoint - DASHED border, sage-tinted (green) to match validates edges
  const evalStyles = () => ({
    bg: 'var(--node-eval-bg)',
    border: props.selected ? 'var(--node-eval-border-selected)' : 'var(--node-eval-border)',
    borderWidth: '1px',
    borderStyle: 'dashed',
    textColor: 'var(--wool-300)',
    radius: '5px',
    boxShadow: props.selected
      ? '0 4px 12px rgba(0,0,0,0.4), inset 0 1px 0 rgba(255,255,255,0.03)'
      : '0 2px 4px rgba(0,0,0,0.25)',
  });

  // CONTAINER: Organizational grouping - ghost style, recedes visually
  const containerStyles = () => ({
    bg: 'transparent',
    border: props.selected ? 'var(--wool-500)' : 'var(--wool-700)',
    borderWidth: '1px',
    borderStyle: 'dashed',
    textColor: 'var(--wool-500)',
    radius: '5px',
    boxShadow: 'none',
  });

  // TASK: The sheep - warmer, more grounded gradient
  const taskStyles = () => ({
    bg: 'var(--node-task-bg)',
    border: props.selected ? 'var(--node-task-border-selected)' : 'var(--node-task-border)',
    borderWidth: '1px',
    borderStyle: 'solid',
    textColor: 'var(--wool-300)',
    radius: '5px',
    boxShadow: props.selected
      ? '0 4px 12px rgba(0,0,0,0.4), inset 0 1px 0 rgba(255,255,255,0.03)'
      : '0 2px 4px rgba(0,0,0,0.25), inset 0 1px 0 rgba(255,255,255,0.02)',
  });

  const styles = () => isEval() ? evalStyles() : isContainer() ? containerStyles() : taskStyles();

  // Status glow - external indicator that doesn't affect card dimensions
  const statusGlow = () => {
    if (isWorking()) return 'var(--glow-working)';
    if (isValidated()) return 'var(--glow-validated, var(--glow-done))';
    if (isDone() || isAwaitingEval()) return 'var(--glow-done)';
    if (isNeedsRepair()) return 'var(--glow-needs-repair, 0 0 8px rgba(201, 162, 39, 0.4))';
    if (isFailed()) return 'var(--glow-failed)';
    return undefined;  // pending = no glow
  };

  // Combined shadow: base shadow + status glow
  const combinedShadow = () => {
    const shadows: string[] = [styles().boxShadow];
    const glow = statusGlow();
    if (glow) shadows.push(glow);
    return shadows.join(', ');
  };

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
    >
      <div
        class={`h-full flex items-center justify-center px-2 relative ${isDeleted() ? 'opacity-40' : ''} ${isMultiLine() ? 'flex-col py-1' : ''}`}
        style={{
          background: styles().bg,
          border: `${styles().borderWidth} ${styles().borderStyle} ${isDeleted() ? 'rgba(196, 92, 74, 0.4)' : styles().border}`,
          'border-radius': styles().radius,
          'box-shadow': combinedShadow(),
        }}
      >
        {/* Name text - centered */}
        <div class={`min-w-0 ${isMultiLine() ? 'flex flex-col gap-0.5 items-center' : ''}`}>
          <For each={props.position.lines}>
            {(line) => (
              <span
                class="text-[10px] truncate leading-tight block font-medium text-center"
                style={{ color: styles().textColor }}
              >
                {line}
              </span>
            )}
          </For>
        </div>
      </div>

      {/* Status corner badge */}
      <Show when={isComplete() || isFailed() || isWorking() || isNeedsRepair()}>
        <div
          class="absolute -top-1 -right-1 flex items-center justify-center rounded-full"
          style={{
            width: '14px',
            height: '14px',
            background: isComplete() ? 'var(--sage)' : isFailed() ? 'var(--terra)' : isNeedsRepair() ? 'var(--golden)' : 'var(--amber-500)',
            'box-shadow': '0 1px 3px rgba(0,0,0,0.3)',
          }}
        >
          {/* Done or awaiting eval: single check */}
          <Show when={isDone() || isAwaitingEval()}>
            <svg class="w-2.5 h-2.5 text-pasture-900" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
              <path d="M20 6 9 17l-5-5" />
            </svg>
          </Show>
          {/* Validated: double check (verified) */}
          <Show when={isValidated()}>
            <svg class="w-2.5 h-2.5 text-pasture-900" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
              <path d="M18 6 7 17l-5-5" />
              <path d="m22 10-7.5 7.5L13 16" />
            </svg>
          </Show>
          {/* Needs repair: warning/wrench */}
          <Show when={isNeedsRepair()}>
            <svg class="w-2.5 h-2.5 text-pasture-900" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
              <path d="M12 9v4M12 17h.01" />
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

      {/* Connection anchor indicators (visible on hover) */}
      <div class="absolute left-1/2 -bottom-1 w-1.5 h-1.5 rounded-full bg-wool-600/50 -translate-x-1/2 opacity-0 group-hover:opacity-100 transition-opacity" />
      <div class="absolute left-1/2 -top-1 w-1.5 h-1.5 rounded-full bg-wool-600/50 -translate-x-1/2 opacity-0 group-hover:opacity-100 transition-opacity" />
    </div>
  );
};

// =============================================================================
// Dependency Connectors (Polyline through waypoints)
// =============================================================================

const DependencyConnectors: Component<{
  edgeRoutes: EdgeRoute[];
}> = (props) => {
  // Separate hierarchy edges (render first, behind) from dependency edges
  const hierarchyEdges = () => props.edgeRoutes.filter(e => e.type === 'hierarchy');
  const dependencyEdges = () => props.edgeRoutes.filter(e => e.type !== 'hierarchy');

  return (
    <svg class="absolute inset-0 pointer-events-none overflow-visible" style={{ 'z-index': 0 }}>
      {/* Arrowhead markers for blocked-by edges */}
      <defs>
        <marker
          id="arrowhead-blocked"
          markerWidth="6"
          markerHeight="5"
          refX="5"
          refY="2.5"
          orient="auto"
          markerUnits="strokeWidth"
        >
          <polygon points="0,0 6,2.5 0,5" fill="var(--edge-blocked-by)" />
        </marker>
        <marker
          id="arrowhead-validates"
          markerWidth="6"
          markerHeight="5"
          refX="5"
          refY="2.5"
          orient="auto"
          markerUnits="strokeWidth"
        >
          <polygon points="0,0 6,2.5 0,5" fill="var(--edge-validates)" />
        </marker>
      </defs>

      {/* Hierarchy edges: subtle, structural - rendered first (behind) */}
      <For each={hierarchyEdges()}>
        {(edge) => {
          const { waypoints } = edge;
          if (waypoints.length < 2) return null;

          const pathD = waypoints
            .map((pt, i) => `${i === 0 ? 'M' : 'L'} ${pt[0]} ${pt[1]}`)
            .join(' ');

          return (
            <path
              d={pathD}
              fill="none"
              stroke="var(--edge-hierarchy)"
              stroke-width="1"
              stroke-dasharray="2 3"
              stroke-linecap="round"
              opacity="0.4"
            />
          );
        }}
      </For>

      {/* Dependency edges: blockedBy and validates - rendered on top */}
      <For each={dependencyEdges()}>
        {(edge) => {
          const { waypoints, type } = edge;
          if (waypoints.length < 2) return null;

          const pathD = waypoints
            .map((pt, i) => `${i === 0 ? 'M' : 'L'} ${pt[0]} ${pt[1]}`)
            .join(' ');

          const isValidates = type === 'validates';
          const isBlocked = type === 'blockedBy';

          // BlockedBy: terra/red lines (task depends on task) - with arrowhead
          // Validates: sage/green lines (eval validates task) - with arrowhead
          const strokeColor = isValidates
            ? 'var(--edge-validates)'
            : 'var(--edge-blocked-by)';
          const strokeWidth = isValidates ? 1 : 1.25;
          const markerId = isValidates ? 'url(#arrowhead-validates)' : 'url(#arrowhead-blocked)';

          return (
            <path
              d={pathD}
              fill="none"
              stroke={strokeColor}
              stroke-width={strokeWidth}
              marker-end={markerId}
            />
          );
        }}
      </For>
    </svg>
  );
};


// =============================================================================
// Helper: Build Live Tree from Draft Structure
//
// Uses draft tree structure to organize live nodes into a hierarchical tree.
// =============================================================================

/**
 * Build live tree structure from draft tree, matching live nodes to their draft IDs.
 * Also includes worker-added tasks (nodes with source !== 'spec') as children of their parent.
 */
function buildLiveTreeFromDraft(
  draftTree: DraftNodeTree[],
  liveNodes: LiveNodeTree[]
): LiveNodeTree[] {
  // Recursively flatten all live nodes into a map by draftNodeId
  const liveByDraftId = new Map<string, LiveNodeTree>();
  // Also track all live nodes by their own ID for finding worker-added children
  const liveById = new Map<string, LiveNodeTree>();
  // Track children by parent live node ID (for worker-added tasks)
  const childrenByParentId = new Map<string, LiveNodeTree[]>();

  const flattenLive = (nodes: LiveNodeTree[]) => {
    for (const node of nodes) {
      liveById.set(node.id, node);
      if (node.draftNodeId) {
        liveByDraftId.set(node.draftNodeId, node);
      }
      // Track original children by parent ID
      if (node.children.length > 0) {
        childrenByParentId.set(node.id, node.children);
        flattenLive(node.children);
      }
    }
  };
  flattenLive(liveNodes);

  // Recursively build tree using draft structure
  const buildNode = (draft: DraftNodeTree): LiveNodeTree | null => {
    // Look up corresponding live node by draft ID
    const live = liveByDraftId.get(draft.id);
    if (!live) return null; // Not dispatched yet

    // Recursively build children from draft structure
    const draftChildren = draft.children
      .map(child => buildNode(child))
      .filter((n): n is LiveNodeTree => n !== null);

    // Also include worker/system-added children (live nodes with source !== 'spec')
    const originalChildren = childrenByParentId.get(live.id) || [];
    const workerAddedChildren = originalChildren.filter(child => child.source !== 'spec');

    return {
      ...live,
      children: [...draftChildren, ...workerAddedChildren],
    };
  };

  // Build tree for each root in draft structure
  const treeFromDraft = draftTree
    .map(root => buildNode(root))
    .filter((n): n is LiveNodeTree => n !== null);

  // Also include root-level live nodes that have no draftNodeId (like scope task)
  // These are system/worker-added root nodes that aren't in the draft tree.
  // We iterate over the original liveNodes roots to find orphans.
  const orphanRoots = liveNodes.filter(root => !root.draftNodeId);

  return [...treeFromDraft, ...orphanRoots];
}

// =============================================================================
// Main Component
// =============================================================================

export const SpecBoard: Component = () => {
  const project = useProject();
  const delta = useDelta();
  const route = useRoute();

  // Selection state
  const [selectedNodeId, setSelectedNodeId] = createSignal<string | null>(null);
  const [selectedLiveNodeId, setSelectedLiveNodeId] = createSignal<string | null>(null);

  // View focus state: 'both' shows side-by-side, 'live'/'draft' expands that section
  const [focusedView, setFocusedView] = createSignal<'both' | 'live' | 'draft'>('both');

  // Edit modal state
  const [editingNode, setEditingNode] = createSignal<DraftNodeTree | null>(null);
  const [viewingLiveNode, setViewingLiveNode] = createSignal<LiveNodeTree | null>(null);

  // Delivery dialog state
  const [showDeliveryDialog, setShowDeliveryDialog] = createSignal(false);

  // IDE loading state
  const [ideLoading, setIdeLoading] = createSignal(false);

  // Fork route dialog state
  const [showForkDialog, setShowForkDialog] = createSignal(false);

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


  // Live task filter: controls which nodes are visible in the live tree
  // 'spec-tasks' = tasks from the spec, 'worker-tasks' = worker-added tasks, 'deleted-nodes' = deleted nodes
  const [liveFilters, setLiveFilters] = createSignal<string[]>(['spec-tasks', 'worker-tasks']);

  // Granularity filter: controls depth level of live tree
  // 'all' = show all nodes, '2' = root + immediate children, '1' = root nodes only
  const [granularity, setGranularity] = createSignal<'all' | '2' | '1'>('all');

  // Build live tree with project hierarchy from draft (project nodes are UI-only)
  const liveTreeWithProjects = createMemo(() => {
    // Guard: ensure we have a selected project before accessing delta state
    if (!project.selectedProject()) return [];
    return buildLiveTreeFromDraft(delta.draftTree(), delta.liveTree());
  });

  // Get draft node IDs to identify deleted nodes (in live but not in draft)
  const draftNodeIds = createMemo(() => {
    const ids = new Set<string>();
    const collectIds = (nodes: DraftNodeTree[]) => {
      for (const node of nodes) {
        ids.add(node.id);
        collectIds(node.children);
      }
    };
    collectIds(delta.draftTree());
    return ids;
  });

  // Filter live tree based on selected filters and granularity
  const filteredLiveTree = createMemo(() => {
    const trees = liveTreeWithProjects();
    const filters = liveFilters();
    const showSpecTasks = filters.includes('spec-tasks');
    const showWorkerTasks = filters.includes('worker-tasks');
    const showDeletedNodes = filters.includes('deleted-nodes');
    const draftIds = draftNodeIds();
    const maxDepth = granularity() === 'all' ? Infinity : granularity() === '2' ? 2 : 1;

    // Filter recursively based on node type and depth
    const filterTree = (node: LiveNodeTree, depth: number = 1): LiveNodeTree | null => {
      const isWorkerAdded = node.source !== 'spec';
      const isDeleted = node.source === 'spec' && !draftIds.has(node.id);
      const isSpecTask = node.source === 'spec' && draftIds.has(node.id);

      // Granularity: don't recurse beyond maxDepth
      const filteredChildren = depth < maxDepth
        ? node.children
            .map((c) => filterTree(c, depth + 1))
            .filter((n): n is LiveNodeTree => n !== null)
        : [];

      // Determine if this node should be shown
      let shouldShow = false;
      if (isDeleted && showDeletedNodes) shouldShow = true;
      if (isWorkerAdded && showWorkerTasks) shouldShow = true;
      if (isSpecTask && showSpecTasks) shouldShow = true;

      // Also include if any children passed the filter
      if (shouldShow || filteredChildren.length > 0) {
        return { ...node, children: filteredChildren };
      }
      return null;
    };

    return trees.map((t) => filterTree(t, 1)).filter((n): n is LiveNodeTree => n !== null);
  });

  // Layout state (computed via ELK.js in frontend)
  const [draftLayoutResult, setDraftLayoutResult] = createSignal<ElkLayoutResult | null>(null);
  const [liveLayoutResult, setLiveLayoutResult] = createSignal<ElkLayoutResult | null>(null);

  // Convert tree nodes to ELK input format
  const treeToLayoutNodes = (trees: (DraftNodeTree | LiveNodeTree)[]): LayoutInputNode[] => {
    const convert = (node: DraftNodeTree | LiveNodeTree): LayoutInputNode => ({
      id: node.id,
      name: node.name,
      nodeType: node.nodeType,
      blockedBy: node.blockedBy,
      validates: node.validates,
      children: node.children.map(convert),
    });
    return trees.map(convert);
  };

  // Compute draft layout when tree changes (using ELK.js)
  createEffect(() => {
    // Guard: ensure we have a selected project
    if (!project.selectedProject()) {
      setDraftLayoutResult(null);
      return;
    }
    const trees = delta.draftTree();
    if (trees.length === 0) {
      setDraftLayoutResult(null);
      return;
    }
    // Compute layout using ELK.js
    const layoutNodes = treeToLayoutNodes(trees);
    computeElkLayout(layoutNodes)
      .then(setDraftLayoutResult)
      .catch(e => console.warn('Failed to compute draft layout:', e));
  });

  // Compute live layout when tree changes (using ELK.js)
  createEffect(() => {
    // Guard: ensure we have a selected project
    if (!project.selectedProject()) {
      setLiveLayoutResult(null);
      return;
    }
    const trees = filteredLiveTree();
    if (trees.length === 0) {
      setLiveLayoutResult(null);
      return;
    }
    // Compute layout using ELK.js
    const layoutNodes = treeToLayoutNodes(trees);
    computeElkLayout(layoutNodes)
      .then(setLiveLayoutResult)
      .catch(e => console.warn('Failed to compute live layout:', e));
  });

  // Transform ELK layout to LayoutTreeResult format for rendering
  const transformLayout = (
    elkLayout: ElkLayoutResult | null,
    trees: DraftNodeTree[] | LiveNodeTree[]
  ): LayoutTreeResult => {
    const positions = new Map<string, NodePosition>();
    const edgeRoutes: EdgeRoute[] = [];
    const evalsWithValidates: { id: string; validates: string[] }[] = [];

    if (!elkLayout || trees.length === 0) {
      return {
        positions,
        width: 0,
        height: 0,
        evalsWithValidates,
        edgeRoutes,
      };
    }

    // Build position map, adding text wrapping info
    for (const pos of elkLayout.positions) {
      if (pos.isDummy) continue; // Skip dummy nodes

      // Find the node to get its name for text wrapping
      const node = findNodeById(trees, pos.id);
      const name = node?.name || '';

      positions.set(pos.id, {
        x: pos.x,
        y: pos.y,
        width: pos.width,
        height: pos.height,
        lines: wrapTextToWidth(name, pos.width),
      });
    }

    // Transform ELK edges to our format - pass waypoints through directly
    for (const edge of elkLayout.edges) {
      if (edge.waypoints.length < 2) continue;

      edgeRoutes.push({
        from: edge.fromId,
        to: edge.toId,
        type: edge.edgeType,
        waypoints: edge.waypoints,
      });
    }

    // Collect evals with validates
    const flatNodes = flattenTree(trees);
    for (const node of flatNodes) {
      if (node.nodeType === 'eval' && 'validates' in node && node.validates && node.validates.length > 0) {
        evalsWithValidates.push({ id: node.id, validates: node.validates as string[] });
      }
    }

    return {
      positions,
      width: elkLayout.width,
      height: elkLayout.height,
      evalsWithValidates,
      edgeRoutes,
    };
  };

  // Helper to find node by ID in tree
  const findNodeById = (trees: (DraftNodeTree | LiveNodeTree)[], id: string): DraftNodeTree | LiveNodeTree | null => {
    for (const root of trees) {
      if (root.id === id) return root;
      const found = findInChildren(root.children, id);
      if (found) return found;
    }
    return null;
  };

  const findInChildren = (children: (DraftNodeTree | LiveNodeTree)[], id: string): DraftNodeTree | LiveNodeTree | null => {
    for (const child of children) {
      if (child.id === id) return child;
      const found = findInChildren(child.children, id);
      if (found) return found;
    }
    return null;
  };

  // Helper to flatten tree
  const flattenTree = (trees: (DraftNodeTree | LiveNodeTree)[]): (DraftNodeTree | LiveNodeTree)[] => {
    const result: (DraftNodeTree | LiveNodeTree)[] = [];
    const flatten = (node: DraftNodeTree | LiveNodeTree) => {
      result.push(node);
      for (const child of node.children) {
        flatten(child);
      }
    };
    for (const tree of trees) {
      flatten(tree);
    }
    return result;
  };

  // Computed layouts from Rust
  const draftLayout = createMemo(() =>
    transformLayout(draftLayoutResult(), delta.draftTree())
  );

  const liveLayout = createMemo(() =>
    transformLayout(liveLayoutResult(), filteredLiveTree())
  );

  // Check if we have a live tree (post-dispatch)
  const hasLiveTree = () => liveTreeWithProjects().length > 0;


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
    setEditingNode(node);
  };

  const handleSaveEdit = async (updates: {
    name: string;
    content: string;
    validates: string[];
    blockedBy: string[];
  }) => {
    const node = editingNode();
    if (!node) return;

    await delta.updateDraftNode(node.id, updates);
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

  const handleOpenInIde = async () => {
    const run = delta.projectRun();
    if (!run || ideLoading()) return;

    setIdeLoading(true);
    try {
      const result = await invoke<{
        success: boolean;
        ideUsed: string;
        pathOpened: string;
        wasDownloaded: boolean;
      }>('open_in_ide', { runName: run.runName });
      if (result.success) {
        window.toast?.success(`Opened in ${result.ideUsed}`);
      }
    } catch (e) {
      window.toast?.error(`Failed to open IDE: ${e}`);
    } finally {
      setIdeLoading(false);
    }
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

  // Listen for radial menu actions
  createEffect(() => {
    const handleRadialDispatch = () => handleDispatch();
    const handleRadialDeliver = () => setShowDeliveryDialog(true);
    const handleRadialOpenIde = () => handleOpenInIde();
    const handleOpenForkDialog = () => setShowForkDialog(true);

    window.addEventListener('radial-dispatch', handleRadialDispatch);
    window.addEventListener('radial-deliver', handleRadialDeliver);
    window.addEventListener('radial-open-ide', handleRadialOpenIde);
    window.addEventListener('open-fork-dialog', handleOpenForkDialog);

    onCleanup(() => {
      window.removeEventListener('radial-dispatch', handleRadialDispatch);
      window.removeEventListener('radial-deliver', handleRadialDeliver);
      window.removeEventListener('radial-open-ide', handleRadialOpenIde);
      window.removeEventListener('open-fork-dialog', handleOpenForkDialog);
    });
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
  // Trackpad/mouse support:
  // - Pinch-to-zoom (ctrlKey on wheel) or Ctrl+scroll = zoom centered on cursor
  // - Two-finger scroll / regular scroll = pan
  // - Middle mouse / Alt+click drag = pan (legacy)
  const handleWheel = (e: WheelEvent) => {
    const side = getSideFromEvent(e);
    if (!side) return;

    e.preventDefault();

    // Pinch-to-zoom gesture (trackpad) or Ctrl+scroll (mouse) = zoom
    if (e.ctrlKey) {
      const zoomFactor = e.deltaY < 0 ? 1.1 : 0.9;

      if (side === 'draft') {
        const oldZoom = draftZoom();
        const section = (e.target as HTMLElement).closest('.tree-section');
        const layout = draftLayout();
        const rect = section?.getBoundingClientRect();
        const minZoom = rect ? calcFitZoom(layout.width, layout.height, rect.width, rect.height) : 0.25;
        const newZoom = Math.max(minZoom, Math.min(3, oldZoom * zoomFactor));
        if (section && rect) {
          const cursorX = e.clientX - rect.left - TREE_LEFT_MARGIN;
          const cursorY = e.clientY - rect.top - rect.height / 2;
          const currentPan = draftPan();
          const contentX = (cursorX - currentPan.x) / oldZoom;
          const contentY = (cursorY - currentPan.y) / oldZoom;
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
        const rect = section?.getBoundingClientRect();
        const minZoom = rect ? calcFitZoom(layout.width, layout.height, rect.width, rect.height) : 0.25;
        const newZoom = Math.max(minZoom, Math.min(3, oldZoom * zoomFactor));
        if (section && rect) {
          const cursorX = e.clientX - rect.left - TREE_LEFT_MARGIN;
          const cursorY = e.clientY - rect.top - rect.height / 2;
          const currentPan = livePan();
          const contentX = (cursorX - currentPan.x) / oldZoom;
          const contentY = (cursorY - currentPan.y) / oldZoom;
          setLivePan({
            x: cursorX - contentX * newZoom,
            y: cursorY - contentY * newZoom,
          });
        }
        setLiveZoom(newZoom);
      }
    } else {
      // Regular scroll = pan (two-finger swipe on trackpad, scroll wheel on mouse)
      const section = (e.target as HTMLElement).closest('.tree-section');
      const rect = section?.getBoundingClientRect();

      if (side === 'draft') {
        const layout = draftLayout();
        const rawPan = {
          x: draftPan().x - e.deltaX,
          y: draftPan().y - e.deltaY,
        };
        const clampedPan = rect
          ? clampPan(rawPan, layout.width, layout.height, rect.width, rect.height, draftZoom())
          : rawPan;
        setDraftPan(clampedPan);
      } else {
        const layout = liveLayout();
        const rawPan = {
          x: livePan().x - e.deltaX,
          y: livePan().y - e.deltaY,
        };
        const clampedPan = rect
          ? clampPan(rawPan, layout.width, layout.height, rect.width, rect.height, liveZoom())
          : rawPan;
        setLivePan(clampedPan);
      }
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

  // Switch view and reset zoom/pan for clean centered view
  const switchView = (view: 'draft' | 'live' | 'both') => {
    setFocusedView(view);
    setDraftZoom(1);
    setLiveZoom(1);

    // Wait for DOM to update with new panel sizes, then center trees
    requestAnimationFrame(() => {
      // Center draft tree
      if (view === 'draft' || view === 'both') {
        const draftSection = document.querySelector('.draft-section');
        if (draftSection) {
          const rect = draftSection.getBoundingClientRect();
          const layout = draftLayout();
          // Center horizontally: (panelWidth - treeWidth) / 2 - TREE_LEFT_MARGIN
          const centerX = Math.max(0, (rect.width - layout.width) / 2 - TREE_LEFT_MARGIN);
          setDraftPan({ x: centerX, y: 0 });
        } else {
          setDraftPan({ x: 0, y: 0 });
        }
      }

      // Center live tree
      if (view === 'live' || view === 'both') {
        const liveSection = document.querySelector('.live-section');
        if (liveSection) {
          const rect = liveSection.getBoundingClientRect();
          const layout = liveLayout();
          const centerX = Math.max(0, (rect.width - layout.width) / 2 - TREE_LEFT_MARGIN);
          setLivePan({ x: centerX, y: 0 });
        } else {
          setLivePan({ x: 0, y: 0 });
        }
      }
    });
  };

  // Keyboard shortcuts for view switching (d=Draft, l=Live, b=Both)
  createEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') return;

      const key = e.key.toLowerCase();
      if (key === 'd') {
        switchView('draft');
      } else if (key === 'l' && hasLiveTree()) {
        switchView('live');
      } else if (key === 'b' && hasLiveTree()) {
        switchView('both');
      }
    };

    document.addEventListener('keydown', handler);
    onCleanup(() => document.removeEventListener('keydown', handler));
  });

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
      <div class="flex-1 flex flex-col overflow-hidden">
        {/* Canvas Toolbar - workers, status, live tree filters */}
        <CanvasToolbar
          granularity={granularity}
          setGranularity={setGranularity}
          liveFilters={liveFilters}
          setLiveFilters={setLiveFilters}
          hasLiveTree={hasLiveTree()}
        />

        <div class="flex-1 flex flex-col overflow-hidden bg-pasture-900">
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
          {/* Top-right: View controls - Draft/Live/Both */}
          <div
            class="absolute top-3 right-3 z-20 flex items-center gap-0.5 p-1 rounded-lg"
            style={{
              background: 'rgba(30, 30, 30, 0.9)',
              border: '1px solid rgba(64, 64, 64, 0.5)',
              'box-shadow': '0 2px 8px rgba(0,0,0,0.3)',
            }}
          >
            {/* Draft button */}
            <button
              onClick={() => switchView('draft')}
              class="px-2.5 py-1 rounded text-[10px] font-medium transition-all"
              style={{
                background: focusedView() === 'draft' ? 'rgba(212, 165, 116, 0.25)' : 'transparent',
                color: focusedView() === 'draft' ? 'var(--amber-400)' : 'var(--wool-500)',
              }}
              title="Draft view (D)"
            >
              Draft
            </button>

            {/* Live button - only show when live tree exists */}
            <Show when={hasLiveTree()}>
              <button
                onClick={() => switchView('live')}
                class="px-2.5 py-1 rounded text-[10px] font-medium transition-all"
                style={{
                  background: focusedView() === 'live' ? 'rgba(138, 133, 128, 0.2)' : 'transparent',
                  color: focusedView() === 'live' ? 'var(--wool-300)' : 'var(--wool-500)',
                }}
                title="Live view (L)"
              >
                Live
              </button>

              {/* Both button */}
              <button
                onClick={() => switchView('both')}
                class="px-2.5 py-1 rounded text-[10px] font-medium transition-all"
                style={{
                  background: focusedView() === 'both' ? 'rgba(138, 133, 128, 0.2)' : 'transparent',
                  color: focusedView() === 'both' ? 'var(--wool-300)' : 'var(--wool-500)',
                }}
                title="Both views (B)"
              >
                Both
              </button>
            </Show>
          </div>

          {/* Bottom-right: Zoom indicator */}
          <Show when={hasLiveTree()}>
            <div
              class="absolute bottom-3 right-3 z-20 flex items-center gap-2 px-2.5 py-1 rounded text-[10px] font-medium"
              style={{ background: 'rgba(30, 30, 30, 0.85)', border: '1px solid rgba(64, 64, 64, 0.4)' }}
            >
              <span style={{ color: 'var(--amber-600)' }}>{Math.round(draftZoom() * 100)}%</span>
              <span class="text-wool-700">·</span>
              <span class="text-wool-500">{Math.round(liveZoom() * 100)}%</span>
            </div>
          </Show>

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
                      <DependencyConnectors edgeRoutes={draftLayout().edgeRoutes} />
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
                      <DependencyConnectors edgeRoutes={liveLayout().edgeRoutes} />
                      <For each={flattenLiveTree(filteredLiveTree())}>
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
            {/* Task/eval node menu */}
            <Show when={contextMenu()!.node}>
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
                          {isEval() ? 'Add an eval' : 'Add a work item'}
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

        {/* Edit Modal (CodeMirror-based) */}
        <Show when={editingNode()}>
          {(node) => (
            <TaskEditorModal
              node={node()}
              projectId={project.selectedProject()!.id}
              onSave={handleSaveEdit}
              onClose={() => setEditingNode(null)}
            />
          )}
        </Show>

        {/* Live Node Detail Modal (Read-only) */}
        <Show when={viewingLiveNode()}>
          {(node) => {
            const nodeType = () => node().nodeType;
            const isEval = () => nodeType() === 'eval';
            const typeLabel = () => (isEval() ? 'Eval' : 'Task');

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
                  class="w-[560px] max-h-[85vh] flex flex-col rounded-lg shadow-xl"
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
                          <svg class="w-5 h-5" style={{ color: 'var(--amber-500)' }} fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" />
                          </svg>
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
                  <div class="p-4 space-y-4 overflow-y-auto flex-1">
                    {/* Description/Content (read-only, rendered markdown) */}
                    <Show when={node().content}>
                      <div class="space-y-1.5">
                        <label class="text-xs font-medium text-wool-300">
                          {isEval() ? 'Acceptance Criteria' : 'Description'}
                        </label>
                        <div
                          class="w-full px-4 py-3 rounded-md bg-pasture-900/50 border border-pasture-700 overflow-auto"
                          style={{ 'max-height': '400px' }}
                        >
                          <MarkdownContent content={node().content} compact />
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

                    {/* Relationships Section */}
                    <div class="space-y-2 pt-2 border-t border-pasture-700/50">
                      <label class="text-xs font-medium text-wool-400">Relationships</label>

                      {/* Parent */}
                      <Show when={node().parentId}>
                        <div class="flex items-center gap-2 text-[11px]">
                          <span class="text-wool-500 w-16">Parent:</span>
                          <code class="px-1.5 py-0.5 rounded bg-pasture-900/50 text-wool-300 font-mono">
                            {node().parentId}
                          </code>
                        </div>
                      </Show>

                      {/* Children */}
                      <Show when={node().children.length > 0}>
                        <div class="flex items-start gap-2 text-[11px]">
                          <span class="text-wool-500 w-16 pt-0.5">Children:</span>
                          <div class="flex flex-wrap gap-1">
                            <For each={node().children}>
                              {(child) => (
                                <code class="px-1.5 py-0.5 rounded bg-pasture-900/50 text-wool-300 font-mono">
                                  {child.id}
                                </code>
                              )}
                            </For>
                          </div>
                        </div>
                      </Show>

                      {/* Blocked By */}
                      <Show when={node().blockedBy.length > 0}>
                        <div class="flex items-start gap-2 text-[11px]">
                          <span class="text-wool-500 w-16 pt-0.5">Blocked:</span>
                          <div class="flex flex-wrap gap-1">
                            <For each={node().blockedBy}>
                              {(blockerId) => (
                                <code class="px-1.5 py-0.5 rounded text-terra/80 font-mono" style={{ background: 'rgba(196, 112, 96, 0.1)' }}>
                                  {blockerId}
                                </code>
                              )}
                            </For>
                          </div>
                        </div>
                      </Show>

                      {/* Claimed By */}
                      <Show when={node().claimedBy}>
                        <div class="flex items-center gap-2 text-[11px]">
                          <span class="text-wool-500 w-16">Worker:</span>
                          <code class="px-1.5 py-0.5 rounded bg-amber-500/10 text-amber-400 font-mono">
                            {node().claimedBy}
                          </code>
                        </div>
                      </Show>
                    </div>

                    {/* Node ID */}
                    <div class="space-y-1.5 pt-2 border-t border-pasture-700/50">
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


        {/* Delivery Dialog */}
        <Show when={showDeliveryDialog()}>
          <DeliveryDialog
            onClose={() => setShowDeliveryDialog(false)}
            defaultBranch="main"
          />
        </Show>

        {/* Fork Route Dialog */}
        <Show when={showForkDialog()}>
          <ForkRouteDialog onClose={() => setShowForkDialog(false)} />
        </Show>
      </div>
    </Show>
  );
};

export default SpecBoard;
