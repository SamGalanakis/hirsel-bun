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
import { useProject } from '../../stores';
import { useDelta } from '../../stores/delta-context';
import { generateSheepSvg } from '../../lib/sheep-avatar';
import { WorkerDetailModal } from '../runs/WorkerDetailModal';
import { computeElkLayout, type LayoutInputNode, type ElkLayoutResult } from '../../lib/elk-layout';
import type {
  DraftNodeTree,
  LiveNodeTree,
  TreeDiff,
  NodeType,
  LiveNodeStatus,
  WorkerDisplay,
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
  type: 'blockedBy' | 'validates';
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
  const isMultiLine = () => props.position.lines.length > 1;

  // ==========================================================================
  // Visual Hierarchy - Distinguished by SHAPE, BORDER, and GRADIENT
  // NO color for delta status - colors reserved for live status only
  // ==========================================================================

  // EVAL: The gate/checkpoint - DASHED border, sage-tinted (green) to match validates edges
  const evalStyles = () => ({
    bg: 'linear-gradient(135deg, rgba(38, 45, 40, 0.95) 0%, rgba(32, 38, 34, 0.98) 100%)',
    border: props.selected ? 'rgba(100, 140, 90, 0.6)' : 'rgba(70, 100, 65, 0.5)',
    borderWidth: '1px',
    borderStyle: 'dashed',
    textColor: 'var(--wool-300)',
    radius: '5px',
    boxShadow: props.selected
      ? '0 4px 12px rgba(0,0,0,0.4), inset 0 1px 0 rgba(255,255,255,0.03)'
      : '0 2px 4px rgba(0,0,0,0.25)',
  });

  // TASK: The sheep - warmer, more grounded gradient
  const taskStyles = () => ({
    bg: 'linear-gradient(135deg, rgba(45, 42, 38, 0.95) 0%, rgba(38, 35, 32, 0.98) 100%)',
    border: props.selected ? 'rgba(140, 130, 115, 0.5)' : 'rgba(90, 85, 78, 0.4)',
    borderWidth: '1px',
    borderStyle: 'solid',
    textColor: 'var(--wool-300)',
    radius: '5px',
    boxShadow: props.selected
      ? '0 4px 12px rgba(0,0,0,0.4), inset 0 1px 0 rgba(255,255,255,0.03)'
      : '0 2px 4px rgba(0,0,0,0.25), inset 0 1px 0 rgba(255,255,255,0.02)',
  });

  const styles = () => isEval() ? evalStyles() : taskStyles();

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
  const isMultiLine = () => props.position.lines.length > 1;
  const isWorking = () => props.node.status === 'working';
  const isDone = () => props.node.status === 'done';
  const isFailed = () => props.node.status === 'failed';

  // ==========================================================================
  // Visual Hierarchy - Identical to DraftNodeCard
  // Status shown via subtle external glow only (no internal changes)
  // ==========================================================================

  // EVAL: The gate/checkpoint - DASHED border, sage-tinted (green) to match validates edges
  const evalStyles = () => ({
    bg: 'linear-gradient(135deg, rgba(38, 45, 40, 0.95) 0%, rgba(32, 38, 34, 0.98) 100%)',
    border: props.selected ? 'rgba(100, 140, 90, 0.6)' : 'rgba(70, 100, 65, 0.5)',
    borderWidth: '1px',
    borderStyle: 'dashed',
    textColor: 'var(--wool-300)',
    radius: '5px',
    boxShadow: props.selected
      ? '0 4px 12px rgba(0,0,0,0.4), inset 0 1px 0 rgba(255,255,255,0.03)'
      : '0 2px 4px rgba(0,0,0,0.25)',
  });

  // TASK: The sheep - warmer, more grounded gradient
  const taskStyles = () => ({
    bg: 'linear-gradient(135deg, rgba(45, 42, 38, 0.95) 0%, rgba(38, 35, 32, 0.98) 100%)',
    border: props.selected ? 'rgba(140, 130, 115, 0.5)' : 'rgba(90, 85, 78, 0.4)',
    borderWidth: '1px',
    borderStyle: 'solid',
    textColor: 'var(--wool-300)',
    radius: '5px',
    boxShadow: props.selected
      ? '0 4px 12px rgba(0,0,0,0.4), inset 0 1px 0 rgba(255,255,255,0.03)'
      : '0 2px 4px rgba(0,0,0,0.25), inset 0 1px 0 rgba(255,255,255,0.02)',
  });

  const styles = () => isEval() ? evalStyles() : taskStyles();

  // Status glow - external indicator that doesn't affect card dimensions
  const statusGlow = () => {
    if (isWorking()) return '0 0 12px rgba(212, 165, 116, 0.4)';  // amber glow
    if (isDone()) return '0 0 8px rgba(125, 153, 112, 0.25)';     // subtle sage
    if (isFailed()) return '0 0 10px rgba(196, 92, 74, 0.35)';    // terra
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
  return (
    <svg class="absolute inset-0 pointer-events-none overflow-visible" style={{ 'z-index': 0 }}>
      {/* Arrowhead markers */}
      <defs>
        <marker
          id="arrow-blockedBy"
          viewBox="0 0 10 10"
          refX="9"
          refY="5"
          markerWidth="6"
          markerHeight="6"
          orient="auto-start-reverse"
        >
          <path d="M 0 0 L 10 5 L 0 10 z" fill="rgb(145, 85, 70)" />
        </marker>
        <marker
          id="arrow-validates"
          viewBox="0 0 10 10"
          refX="9"
          refY="5"
          markerWidth="5"
          markerHeight="5"
          orient="auto-start-reverse"
        >
          <path d="M 0 0 L 10 5 L 0 10 z" fill="rgb(85, 115, 75)" />
        </marker>
      </defs>

      <For each={props.edgeRoutes}>
        {(edge) => {
          const { waypoints, type } = edge;
          if (waypoints.length < 2) return null;

          // Build SVG path through all waypoints
          const pathD = waypoints
            .map((pt, i) => `${i === 0 ? 'M' : 'L'} ${pt[0]} ${pt[1]}`)
            .join(' ');

          // Styling based on relationship type
          const isValidates = type === 'validates';

          // BlockedBy: terra/red lines (dependency flows to target)
          // Validates: sage/green lines (validation flows to target)
          const strokeColor = isValidates
            ? 'rgb(85, 115, 75)'     // Sage/green for validates
            : 'rgb(145, 85, 70)';    // Terra/red for blockedBy
          const strokeWidth = isValidates ? 1 : 1.25;
          const markerId = isValidates ? 'arrow-validates' : 'arrow-blockedBy';

          return (
            <path
              d={pathD}
              fill="none"
              stroke={strokeColor}
              stroke-width={strokeWidth}
              marker-end={`url(#${markerId})`}
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
 */
function buildLiveTreeFromDraft(
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
    // Look up corresponding live node
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
  };

  // Build tree for each root
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

  // Worker state
  const [workers, setWorkers] = createSignal<WorkerDisplay[]>([]);
  const [selectedWorker, setSelectedWorker] = createSignal<WorkerDisplay | null>(null);
  let workerScrollRef: HTMLDivElement | undefined;

  // Build live tree with project hierarchy from draft (project nodes are UI-only)
  const liveTreeWithProjects = createMemo(() =>
    buildLiveTreeFromDraft(delta.draftTree(), delta.liveTree())
  );

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
    const trees = liveTreeWithProjects();
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
    transformLayout(liveLayoutResult(), liveTreeWithProjects())
  );

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

  // Fetch workers when a project run is active
  createEffect(() => {
    const run = delta.projectRun();
    if (!run) {
      setWorkers([]);
      return;
    }

    const fetchWorkers = async () => {
      try {
        const result = await invoke<WorkerDisplay[]>('get_workers', { runName: run.runName });
        setWorkers(result);
      } catch (e) {
        console.warn('Failed to fetch workers:', e);
      }
    };

    // Initial fetch
    fetchWorkers();

    // Poll every 2 seconds while run is active
    const interval = setInterval(fetchWorkers, 2000);
    onCleanup(() => clearInterval(interval));
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

  const handleAttachWorker = async () => {
    const worker = selectedWorker();
    const run = delta.projectRun();
    if (!worker || !run) return;

    try {
      await invoke('attach_worker', { runName: run.runName, workerName: worker.name });
      setSelectedWorker(null);
    } catch (e) {
      console.error('Failed to attach to worker:', e);
      window.toast?.error(`Failed to attach: ${e}`);
    }
  };

  // Keyboard shortcuts
  createEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') return;

      if (e.key === 'Escape') {
        if (selectedWorker()) setSelectedWorker(null);
        else if (editingNode()) setEditingNode(null);
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
          {/* Left spacer for balance */}
          <div class="flex-1" />

          {/* Center: Worker Carousel */}
          <Show when={workers().length > 0}>
            <div
              class="flex items-center gap-2 px-2.5 py-1.5 rounded-full"
              style={{
                background: 'rgba(36, 36, 36, 0.5)',
                border: '1px solid rgba(64, 64, 64, 0.4)',
              }}
            >
              {/* Flock label */}
              <span class="text-[9px] text-wool-500 font-medium uppercase tracking-wider">
                Flock
              </span>

              {/* Worker avatars carousel */}
              <div
                ref={workerScrollRef}
                class="flex items-center gap-1.5 overflow-x-auto scrollbar-none"
                style={{ 'max-width': 'min(320px, 40vw)' }}
              >
                <For each={workers()}>
                  {(worker) => {
                    const isWorking = () => worker.status === 'working';
                    const isError = () => worker.status === 'error';
                    const isHitl = () => worker.hitlWaiting;

                    return (
                      <button
                        onClick={() => setSelectedWorker(worker)}
                        class="relative flex-shrink-0 rounded-full transition-all hover:scale-110 focus:outline-none focus:ring-1 focus:ring-amber-500/50"
                        style={{
                          width: '32px',
                          height: '32px',
                          'box-shadow': isWorking()
                            ? '0 0 12px rgba(212, 165, 116, 0.5)'
                            : isError()
                            ? '0 0 10px rgba(196, 92, 74, 0.5)'
                            : isHitl()
                            ? '0 0 10px rgba(201, 162, 39, 0.5)'
                            : undefined,
                        }}
                        title={`${worker.name} · ${worker.status}${worker.currentTask ? ` · ${worker.currentTask}` : ''}`}
                      >
                        <div
                          innerHTML={generateSheepSvg(worker.sheepConfig, 32, worker.status)}
                          class="w-full h-full"
                        />
                        {/* HITL indicator dot */}
                        <Show when={isHitl()}>
                          <div
                            class="absolute -top-0.5 -right-0.5 w-2.5 h-2.5 rounded-full animate-pulse"
                            style={{ background: 'var(--golden)' }}
                          />
                        </Show>
                      </button>
                    );
                  }}
                </For>
              </div>

              {/* Status indicator */}
              <Show when={liveRunStatus()}>
                <div
                  class="flex items-center gap-1.5 pl-2"
                  style={{ 'border-left': '1px solid rgba(64, 64, 64, 0.5)' }}
                >
                  <div
                    class={`w-1.5 h-1.5 rounded-full ${
                      liveRunStatus() === 'working' || liveRunStatus() === 'starting'
                        ? 'animate-pulse'
                        : ''
                    }`}
                    style={{
                      background:
                        liveRunStatus() === 'working'
                          ? 'var(--amber-500)'
                          : liveRunStatus() === 'starting'
                          ? 'var(--amber-400)'
                          : liveRunStatus() === 'done'
                          ? 'var(--sage)'
                          : liveRunStatus() === 'failed'
                          ? 'var(--terra)'
                          : liveRunStatus() === 'paused'
                          ? 'var(--golden)'
                          : 'var(--wool-600)',
                    }}
                  />
                  <span
                    class="text-[9px] font-medium"
                    style={{
                      color:
                        liveRunStatus() === 'working'
                          ? 'var(--amber-400)'
                          : liveRunStatus() === 'starting'
                          ? 'var(--amber-300)'
                          : liveRunStatus() === 'done'
                          ? 'var(--sage)'
                          : liveRunStatus() === 'failed'
                          ? 'var(--terra)'
                          : liveRunStatus() === 'paused'
                          ? 'var(--golden)'
                          : 'var(--wool-600)',
                    }}
                  >
                    {liveRunStatus()}
                  </span>
                </div>
              </Show>
            </div>
          </Show>

          {/* Right side: Controls */}
          <div class="flex-1 flex items-center justify-end gap-2">
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
                      <DependencyConnectors edgeRoutes={liveLayout().edgeRoutes} />
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

        {/* Edit Modal */}
        <Show when={editingNode()}>
          {(node) => {
            const nodeType = () => node().nodeType;
            const isEval = () => nodeType() === 'eval';
            const typeLabel = () => (isEval() ? 'Eval' : 'Task');

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

        {/* Worker Detail Modal */}
        <Show when={selectedWorker()}>
          {(worker) => (
            <WorkerDetailModal
              worker={worker()}
              metricsAvailable={true}
              runName={delta.projectRun()?.runName || ''}
              onClose={() => setSelectedWorker(null)}
              onAttach={handleAttachWorker}
            />
          )}
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
