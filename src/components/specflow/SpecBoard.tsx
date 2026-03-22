/**
 * SpecBoard - Canvas-based Unified Board Tree
 *
 * Renders a single unified board tree where nodes have:
 * - kind: spec | task | eval
 * - status: draft | pending | working | done | awaiting_check | validated | needs_repair | failed
 * - source: user | plan | worker | system
 *
 * Draft spec nodes are editable. Dispatched nodes show status via glows/badges.
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
import { invoke } from '../../lib/invoke';
import { on as onEvent } from '../../lib/events';
import { useProject, useRoute } from '../../stores';
import { useDelta } from '../../stores/delta-context';
import { Icon, Markdown } from '../shared';
import { amber, sage, terra } from '../../lib/theme-colors';
import { CanvasToolbar } from '../layout/CanvasToolbar';
import { TaskEditorModal } from './TaskEditorModal';
import { DeliveryDialog } from './DeliveryDialog';
import { ForkRouteDialog } from './ForkRouteDialog';
import { computeElkLayout, type LayoutInputNode, type ElkLayoutResult } from '../../lib/elk-layout';
import { NodeFinder, type NodeFinderItem } from './NodeFinder';
import { buildFocusIndices, computeFocusSet } from '../../lib/specboard/focus';
import {
  CHAR_WIDTH,
  TEXT_PADDING,
  type NodePosition,
  type EdgeRoute,
  type LayoutTreeResult,
  wrapTextToWidth,
  buildSmoothPath,
} from '../../lib/specboard/layout';
import { contentYToRelative, panForContentPoint, viewportCenterPoint, zoomAroundViewportPoint } from '../../lib/specboard/navigation';
import type {
  BoardNodeTree,
  NodeKind,
  BoardNodeStatus,
} from '../../lib/types';

// =============================================================================
// Node Card Component (unified — renders based on kind + status)
// =============================================================================

const BoardNodeCard: Component<{
  node: BoardNodeTree;
  position: NodePosition;
  selected: boolean;
  dimmed: boolean;
  collapsed: boolean;
  hiddenCounts: {
    working: number;
    pending: number;
    done: number;
    needsRepair: number;
    failed: number;
    checks: number;
  } | null;
  onSelect: () => void;
  onDoubleClick: () => void;
  onContextMenu: (e: MouseEvent) => void;
  onEnterScope: () => void;
  onToggleCollapse: () => void;
  nodeMap: Map<string, BoardNodeTree>;
}> = (props) => {
  const isDraft = () => props.node.status === 'draft';
  const isCheck = () => props.node.kind === 'check';
  const isFeature = () => props.node.kind === 'feature';
  const isPlan = () => props.node.kind === 'plan';
  const isRoot = () => props.node.parentId === null && props.node.kind === 'feature';
  const isFeatureNode = () => props.node.kind === 'feature' && !isRoot();
  const isBeingPlanned = () => isFeature() && props.node.blockedBy.some(id => {
    const blocker = props.nodeMap.get(id);
    return blocker && blocker.kind === 'plan' && blocker.status !== 'done' && blocker.status !== 'failed';
  });
  const isMultiLine = () => props.position.lines.length > 1;
  const isWorking = () => props.node.status === 'working';
  const isDone = () => props.node.status === 'done';
  const isAwaitingCheck = () => props.node.status === 'awaiting_check';
  const isValidated = () => props.node.status === 'validated';
  const isNeedsRepair = () => props.node.status === 'needs_repair';
  const isComplete = () => isDone() || isValidated() || isAwaitingCheck();
  const isFailed = () => props.node.status === 'failed';
  const isClaimable = () => {
    if (props.node.status !== 'pending') return false;
    if (props.node.claimedBy) return false;
    if (props.node.children.length > 0) return false;

    // Check nodes with no explicit validates targets are blocked
    // until all non-check siblings (or all non-check nodes if root-level) are done
    if (isCheck() && props.node.validates.length === 0) {
      const isWorkDone = (s: string) => s === 'done' || s === 'validated' || s === 'awaiting_check';
      for (const [, n] of props.nodeMap) {
        if (n.id === props.node.id || n.kind === 'check' || n.status === 'draft') continue;
        // If has same parent (sibling) or root-level check (all nodes are scope)
        const sameScope = props.node.parentId === null || n.parentId === props.node.parentId;
        if (sameScope && !isWorkDone(n.status)) return false;
      }
    }

    if (props.node.blockedBy.length === 0) return true;
    return props.node.blockedBy.every(id => {
      const blocker = props.nodeMap.get(id);
      return blocker && (blocker.status === 'done' || blocker.status === 'validated' || blocker.status === 'awaiting_check');
    });
  };
  // CHECK: dashed border, sage-tinted
  const checkStyles = () => ({
    bg: 'var(--node-check-bg)',
    border: props.selected ? 'var(--node-check-border-selected)' : 'var(--node-check-border)',
    borderWidth: '1px',
    borderStyle: 'dashed',
    textColor: 'var(--wool-300)',
    radius: '5px',
    boxShadow: props.selected
      ? '0 4px 12px rgba(0,0,0,0.4), inset 0 1px 0 rgba(255,255,255,0.03)'
      : '0 2px 4px rgba(0,0,0,0.25)',
  });

  // ROOT (project origin): warm amber cornerstone
  const rootStyles = () => ({
    bg: 'var(--node-root-bg)',
    border: props.selected ? 'var(--node-root-border-selected)' : 'var(--node-root-border)',
    borderWidth: '1.5px',
    borderStyle: 'solid',
    textColor: 'var(--wool-100)',
    radius: '6px',
    boxShadow: props.selected ? 'var(--node-root-glow-selected)' : 'var(--node-root-glow)',
  });

  // FEATURE (kind=feature, non-root): warm solid card with accent
  const featureStyles = () => ({
    bg: 'var(--node-feature-bg)',
    border: props.selected ? 'var(--node-feature-border-selected)' : 'var(--node-feature-border)',
    borderWidth: '1px',
    borderStyle: 'solid',
    textColor: 'var(--wool-200)',
    radius: '5px',
    boxShadow: props.selected
      ? '0 3px 10px rgba(0,0,0,0.35), inset 0 1px 0 rgba(255,255,255,0.02)'
      : '0 1px 3px rgba(0,0,0,0.2)',
  });

  // TASK / SPEC leaf: solid card
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

  // PLAN: dimmer, dash-dot border
  const planStyles = () => ({
    bg: 'var(--node-task-bg)',
    border: props.selected ? 'var(--node-task-border-selected)' : 'var(--wool-700)',
    borderWidth: '1px',
    borderStyle: 'dashed',
    textColor: 'var(--wool-400)',
    radius: '5px',
    boxShadow: props.selected
      ? '0 4px 12px rgba(0,0,0,0.4), inset 0 1px 0 rgba(255,255,255,0.03)'
      : '0 1px 3px rgba(0,0,0,0.2)',
  });

  const styles = () => isCheck() ? checkStyles() : isPlan() ? planStyles() : isRoot() ? rootStyles() : isFeatureNode() ? featureStyles() : taskStyles();

  // Status glow — only for dispatched (non-draft) nodes
  const statusGlow = () => {
    if (isDraft()) return undefined;
    if (isWorking()) return 'var(--glow-working)';
    if (isValidated()) return 'var(--glow-validated, var(--glow-done))';
    if (isDone() || isAwaitingCheck()) return 'var(--glow-done)';
    if (isNeedsRepair()) return 'var(--glow-needs-repair, 0 0 8px rgba(201, 162, 39, 0.4))';
    if (isFailed()) return 'var(--glow-failed)';
    if (isClaimable()) return 'var(--glow-claimable)';
    return undefined;
  };

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
        opacity: props.dimmed ? 0.18 : 1,
        filter: props.dimmed ? 'saturate(0.85)' : undefined,
        transition: 'opacity 140ms ease-out, filter 140ms ease-out',
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
          'box-shadow': combinedShadow(),
          transform: props.selected ? 'translateY(-1px)' : undefined,
          transition: 'transform 160ms ease-out, box-shadow 160ms ease-out',
        }}
      >
        {/* Collapse chevron (features only) */}
        <Show when={isFeature() && !isDraft()}>
          <button
            type="button"
            class="absolute left-1 top-1 w-5 h-5 rounded-none flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity"
            style={{ background: 'rgba(0,0,0,0.25)', border: '1px solid rgba(255,255,255,0.06)' }}
            title={props.collapsed ? 'Expand' : 'Collapse'}
            onClick={(e) => { e.stopPropagation(); props.onToggleCollapse(); }}
          >
            <Icon name={props.collapsed ? 'chevron-right' : 'chevron-down'} class="w-3 h-3 text-wool-500" />
          </button>
        </Show>

        {/* Enter scope (features only) */}
        <Show when={isFeature() && !isDraft()}>
          <button
            type="button"
            class="absolute right-1 top-1 w-5 h-5 rounded-none flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity"
            style={{ background: 'rgba(0,0,0,0.25)', border: '1px solid rgba(255,255,255,0.06)' }}
            title="Focus this milestone"
            onClick={(e) => { e.stopPropagation(); props.onEnterScope(); }}
          >
            <Icon name="corner-down-right" class="w-3 h-3 text-wool-500" />
          </button>
        </Show>

        {/* Root accent bar */}
        <Show when={isRoot()}>
          <div class="absolute inset-x-0 top-0 h-[1.5px] rounded-t" style={{ background: 'linear-gradient(90deg, transparent 10%, var(--amber-600) 50%, transparent 90%)' }} />
        </Show>
        {/* Feature left accent bar */}
        <Show when={isFeatureNode()}>
          <div class="absolute inset-y-0 left-0 w-[1.5px] rounded-l"
            style={{ background: 'linear-gradient(180deg, transparent 15%, var(--amber-700) 50%, transparent 85%)' }} />
        </Show>
        {/* Name text */}
        <div class={`min-w-0 ${isMultiLine() ? 'flex flex-col gap-0.5 items-center' : ''}`}>
          <For each={props.position.lines}>
            {(line) => (
              <span
                class={`truncate leading-tight block text-center ${
                  isRoot() ? 'text-[11px] font-semibold italic'
                  : isFeatureNode() ? 'text-[11px] font-medium'
                  : 'text-[10px] font-medium'
                }`}
                style={{ color: styles().textColor }}
              >
                {line}
              </span>
            )}
          </For>
        </div>


        {/* Planning indicator (feature blocked by active plan node) */}
        <Show when={isBeingPlanned() && !props.collapsed}>
          <div
            class="absolute -bottom-2 left-1/2 -translate-x-1/2 flex items-center gap-1 px-2 py-0.5 rounded-none text-[9px] font-medium"
            style={{
              background: 'rgba(20,20,22,0.92)',
              border: '1px solid rgba(255,255,255,0.1)',
              color: 'var(--wool-400)',
              'box-shadow': '0 4px 12px rgba(0,0,0,0.3)',
            }}
          >
            <Icon name="list-tree" class="w-2.5 h-2.5" />
            <span>Planning</span>
          </div>
        </Show>

        {/* Collapsed counts chips */}
        <Show when={props.collapsed && props.hiddenCounts}>
          {(counts) => (
            <div
              class="absolute -bottom-2 left-1/2 -translate-x-1/2 flex items-center gap-1 px-1.5 py-0.5 rounded-none"
              style={{
                background: 'rgba(20,20,22,0.92)',
                border: '1px solid rgba(255,255,255,0.08)',
                'box-shadow': '0 6px 18px rgba(0,0,0,0.35)',
              }}
            >
              <Show when={counts().working > 0}>
                <span class="px-1.5 py-0.5 rounded-none text-[9px] font-medium" style={{ color: 'var(--amber-400)', background: 'rgba(212,165,116,0.12)' }}>
                  W {counts().working}
                </span>
              </Show>
              <Show when={counts().pending > 0}>
                <span class="px-1.5 py-0.5 rounded-none text-[9px] font-medium" style={{ color: 'var(--wool-400)', background: 'rgba(255,255,255,0.06)' }}>
                  P {counts().pending}
                </span>
              </Show>
              <Show when={counts().done > 0}>
                <span class="px-1.5 py-0.5 rounded-none text-[9px] font-medium" style={{ color: 'var(--sage-light, var(--sage))', background: 'rgba(125,153,112,0.14)' }}>
                  D {counts().done}
                </span>
              </Show>
              <Show when={counts().needsRepair > 0}>
                <span class="px-1.5 py-0.5 rounded-none text-[9px] font-medium" style={{ color: 'var(--golden-light, var(--golden))', background: 'rgba(201,162,39,0.14)' }}>
                  R {counts().needsRepair}
                </span>
              </Show>
              <Show when={counts().failed > 0}>
                <span class="px-1.5 py-0.5 rounded-none text-[9px] font-medium" style={{ color: 'var(--terra-light, var(--terra))', background: 'rgba(196,92,74,0.14)' }}>
                  F {counts().failed}
                </span>
              </Show>
              <Show when={counts().checks > 0}>
                <span class="px-1.5 py-0.5 rounded-none text-[9px] font-medium" style={{ color: 'var(--sage)', background: 'rgba(85,115,75,0.12)' }}>
                  C {counts().checks}
                </span>
              </Show>
            </div>
          )}
        </Show>

      </div>

      {/* Status corner badge — only for non-draft nodes */}
      <Show when={!isDraft() && (isComplete() || isFailed() || isWorking() || isNeedsRepair())}>
        <div
          class="absolute -top-1 -right-1 flex items-center justify-center rounded-none"
          style={{
            width: '14px',
            height: '14px',
            background: isComplete() ? 'var(--sage)' : isFailed() ? 'var(--terra)' : isNeedsRepair() ? 'var(--golden)' : 'var(--amber-500)',
            'box-shadow': '0 1px 3px rgba(0,0,0,0.3)',
          }}
        >
          <Show when={isDone() || isAwaitingCheck()}>
            <svg class="w-2.5 h-2.5 text-pasture-900" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
              <path d="M20 6 9 17l-5-5" />
            </svg>
          </Show>
          <Show when={isValidated()}>
            <svg class="w-2.5 h-2.5 text-pasture-900" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
              <path d="M18 6 7 17l-5-5" />
              <path d="m22 10-7.5 7.5L13 16" />
            </svg>
          </Show>
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

      {/* Connection anchors */}
      <div class="absolute left-1/2 -bottom-1 w-1.5 h-1.5 rounded-none bg-wool-600/50 -translate-x-1/2 opacity-0 group-hover:opacity-100 transition-opacity" />
      <div class="absolute left-1/2 -top-1 w-1.5 h-1.5 rounded-none bg-wool-600/50 -translate-x-1/2 opacity-0 group-hover:opacity-100 transition-opacity" />
    </div>
  );
};

// =============================================================================
// Dependency Connectors
// =============================================================================

const DependencyConnectors: Component<{
  edgeRoutes: EdgeRoute[];
  edgeToggles: () => { hierarchy: boolean; blockedBy: boolean; validates: boolean; resolves: boolean };
  focusSet: () => Set<string> | null;
  selectedNodeId: () => string | null;
}> = (props) => {
  const shouldShowEdge = (e: EdgeRoute) => {
    const toggles = props.edgeToggles();
    const focus = props.focusSet();
    const selected = props.selectedNodeId();

    // Hide edges outside focus lens.
    if (focus) {
      if (!focus.has(e.from) || !focus.has(e.to)) return false;
    }

    if (e.type === 'hierarchy') return toggles.hierarchy;

    const toggleOn =
      e.type === 'blockedBy' ? toggles.blockedBy
      : e.type === 'validates' ? toggles.validates
      : toggles.resolves;

    // Selection override: if dependency toggles are off, still show deps touching selected.
    if (!toggleOn && selected) {
      return e.from === selected || e.to === selected;
    }
    return toggleOn;
  };

  const hierarchyEdges = () => props.edgeRoutes.filter(e => e.type === 'hierarchy' && shouldShowEdge(e));
  const dependencyEdges = () => props.edgeRoutes.filter(e => e.type !== 'hierarchy' && shouldShowEdge(e));

  return (
    <svg class="absolute inset-0 pointer-events-none overflow-visible" style={{ 'z-index': 0 }}>
      <defs>
        <marker id="arrowhead-blocked" markerWidth="6" markerHeight="5" refX="5" refY="2.5" orient="auto" markerUnits="strokeWidth">
          <polygon points="0,0 6,2.5 0,5" fill="var(--edge-blocked-by)" />
        </marker>
        <marker id="arrowhead-validates" markerWidth="6" markerHeight="5" refX="5" refY="2.5" orient="auto" markerUnits="strokeWidth">
          <polygon points="0,0 6,2.5 0,5" fill="var(--edge-validates)" />
        </marker>
        <marker id="arrowhead-resolves" markerWidth="6" markerHeight="5" refX="5" refY="2.5" orient="auto" markerUnits="strokeWidth">
          <polygon points="0,0 6,2.5 0,5" fill="var(--edge-resolves)" />
        </marker>
      </defs>

      <For each={hierarchyEdges()}>
        {(edge) => {
          const { waypoints } = edge;
          if (waypoints.length < 2) return null;
          const pathD = buildSmoothPath(waypoints);
          return (
            <path d={pathD} fill="none" stroke="var(--edge-hierarchy)" stroke-width="1.2" stroke-linecap="round" opacity="0.5" />
          );
        }}
      </For>

      <For each={dependencyEdges()}>
        {(edge) => {
          const { waypoints, type } = edge;
          if (waypoints.length < 2) return null;
          const pathD = buildSmoothPath(waypoints);
          const isValidates = type === 'validates';
          const isResolves = type === 'resolves';
          const strokeColor = isValidates ? 'var(--edge-validates)' : isResolves ? 'var(--edge-resolves)' : 'var(--edge-blocked-by)';
          const strokeWidth = isValidates ? 0.75 : isResolves ? 1 : 1.25;
          const strokeOpacity = isValidates ? 0.7 : 1;
          const markerId = isValidates ? 'url(#arrowhead-validates)' : isResolves ? 'url(#arrowhead-resolves)' : 'url(#arrowhead-blocked)';
          return (
            <path d={pathD} fill="none" stroke={strokeColor} stroke-width={strokeWidth} opacity={strokeOpacity} marker-end={markerId} />
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
  const route = useRoute();

  // Selection state
  const [selectedNodeId, setSelectedNodeId] = createSignal<string | null>(null);
  const [scopeRootId, setScopeRootId] = createSignal<string | null>(null);
  const [pendingJumpNodeId, setPendingJumpNodeId] = createSignal<string | null>(null);

  // Edit modal state
  const [editingNode, setEditingNode] = createSignal<BoardNodeTree | null>(null);
  const [viewingNode, setViewingNode] = createSignal<BoardNodeTree | null>(null);

  // Delivery dialog state
  const [showDeliveryDialog, setShowDeliveryDialog] = createSignal(false);

  // IDE loading state
  const [ideLoading, setIdeLoading] = createSignal(false);

  // Fork route dialog state
  const [showForkDialog, setShowForkDialog] = createSignal(false);

  // New node prompt
  const [showNewPrompt, setShowNewPrompt] = createSignal(false);
  const [newNodeParentId, setNewNodeParentId] = createSignal<string | null>(null);
  const [newNodeKind, setNewNodeKind] = createSignal<NodeKind>('feature');
  const [newNodeName, setNewNodeName] = createSignal('');
  let newNodeInputRef: HTMLInputElement | undefined;

  // Context menu
  const [contextMenu, setContextMenu] = createSignal<{
    x: number;
    y: number;
    node: BoardNodeTree | null;
    isBackground: boolean;
  } | null>(null);

  // Pan and zoom state
  const [zoom, setZoom] = createSignal(1);
  const [pan, setPan] = createSignal({ x: 0, y: 0 });
  const [isPanning, setIsPanning] = createSignal(false);
  const [panStart, setPanStart] = createSignal({ x: 0, y: 0 });
  const [autoFitDone, setAutoFitDone] = createSignal(false);
  let canvasRef: HTMLDivElement | undefined;

  // Live task filter
  const [liveFilters, setLiveFilters] = createSignal<string[]>(['spec-tasks', 'worker-tasks', 'plan-tasks']);
  const [granularity, setGranularity] = createSignal<'all' | '2'>('all');

  // Edge toggles (default: only hierarchy)
  const [edgeToggles, setEdgeToggles] = createSignal({
    hierarchy: true,
    blockedBy: false,
    validates: false,
    resolves: false,
  });

  // Feature collapse state (store as array for easy serialization)
  const [collapsedFeatureIds, setCollapsedFeatureIds] = createSignal<string[]>([]);

  // Node finder (search + jump)
  const [finderOpen, setFinderOpen] = createSignal(false);
  const [finderQuery, setFinderQuery] = createSignal('');

  // Reset auto-fit when scope changes (new "world" size)
  createEffect(() => {
    scopeRootId();
    setAutoFitDone(false);
  });

  // Re-fit after view-shaping filters change (depth, show filters, collapse).
  createEffect(() => {
    granularity();
    liveFilters().join('|');
    collapsedFeatureIds().join('|');
    setAutoFitDone(false);
  });

  // Scope + filter tree based on filters and granularity (no feature collapse here)
  const scopedAndFilteredTree = createMemo(() => {
    const allTrees = delta.boardTree();
    const scopeId = scopeRootId();

    const findInTrees = (nodes: BoardNodeTree[], id: string): BoardNodeTree | null => {
      for (const n of nodes) {
        if (n.id === id) return n;
        const found = findInTrees(n.children, id);
        if (found) return found;
      }
      return null;
    };

    const scopedTrees = scopeId ? (() => {
      const found = findInTrees(allTrees, scopeId);
      return found ? [found] : allTrees;
    })() : allTrees;

    const filters = liveFilters();
    const showSpecTasks = filters.includes('spec-tasks');
    const showWorkerTasks = filters.includes('worker-tasks');
    const showPlanTasks = filters.includes('plan-tasks');
    const maxDepth = granularity() === 'all' ? Infinity : 2;

    const filterTree = (node: BoardNodeTree, depth: number = 1): BoardNodeTree | null => {
      const isUserNode = node.source === 'user';
      const isWorkerAdded = node.source === 'worker' || node.source === 'plan';
      const isPlanNode = node.kind === 'plan';

      const filteredChildren = depth < maxDepth
        ? node.children
            .map((c) => filterTree(c, depth + 1))
            .filter((n): n is BoardNodeTree => n !== null)
        : [];

      let shouldShow = false;
      if (isPlanNode) {
        shouldShow = showPlanTasks;
      } else if (isUserNode && showSpecTasks) {
        shouldShow = true;
      } else if (isWorkerAdded && showWorkerTasks) {
        shouldShow = true;
      } else if (node.source === 'system') {
        shouldShow = showWorkerTasks;
      }

      if (shouldShow || filteredChildren.length > 0) {
        return { ...node, children: filteredChildren };
      }
      return null;
    };

    return scopedTrees.map((t) => filterTree(t, 1)).filter((n): n is BoardNodeTree => n !== null);
  });

  const hiddenCountsByFeatureId = createMemo(() => {
    const counts = new Map<string, {
      working: number;
      pending: number;
      done: number;
      needsRepair: number;
      failed: number;
      checks: number;
    }>();

    const tallyStatus = (status: BoardNodeStatus) => {
      if (status === 'working') return 'working';
      if (status === 'pending') return 'pending';
      if (status === 'needs_repair') return 'needsRepair';
      if (status === 'failed') return 'failed';
      // done-ish
      return 'done';
    };

    const walk = (node: BoardNodeTree): {
      working: number;
      pending: number;
      done: number;
      needsRepair: number;
      failed: number;
      checks: number;
    } => {
      const acc = { working: 0, pending: 0, done: 0, needsRepair: 0, failed: 0, checks: 0 };
      for (const child of node.children) {
        const childAcc = walk(child);
        acc.working += childAcc.working;
        acc.pending += childAcc.pending;
        acc.done += childAcc.done;
        acc.needsRepair += childAcc.needsRepair;
        acc.failed += childAcc.failed;
        acc.checks += childAcc.checks;
      }

      // Count this node (excluding feature itself for feature chips)
      if (node.kind === 'check') acc.checks += 1;
      const bucket = tallyStatus(node.status);
      acc[bucket] += 1;

      if (node.kind === 'feature') {
        // Exclude the feature node itself from hidden counts.
        const bucketSelf = tallyStatus(node.status);
        acc[bucketSelf] -= 1;
        counts.set(node.id, {
          working: Math.max(0, acc.working),
          pending: Math.max(0, acc.pending),
          done: Math.max(0, acc.done),
          needsRepair: Math.max(0, acc.needsRepair),
          failed: Math.max(0, acc.failed),
          checks: Math.max(0, acc.checks),
        });
      }

      return acc;
    };

    for (const root of scopedAndFilteredTree()) walk(root);
    return counts;
  });

  // Apply per-feature collapse for rendering + layout
  const filteredTree = createMemo(() => {
    const collapsed = new Set(collapsedFeatureIds());
    const cloneWithCollapse = (node: BoardNodeTree): BoardNodeTree => {
      if (node.kind === 'feature' && node.status !== 'draft' && collapsed.has(node.id)) {
        return { ...node, children: [] };
      }
      return { ...node, children: node.children.map(cloneWithCollapse) };
    };
    return scopedAndFilteredTree().map(cloneWithCollapse);
  });

  // Map of all nodes by ID
  const nodeMap = createMemo(() => {
    const map = new Map<string, BoardNodeTree>();
    const collect = (nodes: BoardNodeTree[]) => {
      for (const n of nodes) {
        map.set(n.id, n);
        collect(n.children);
      }
    };
    collect(filteredTree());
    return map;
  });

  const uncollapsedNodeMap = createMemo(() => {
    const map = new Map<string, BoardNodeTree>();
    const collect = (nodes: BoardNodeTree[]) => {
      for (const n of nodes) {
        map.set(n.id, n);
        collect(n.children);
      }
    };
    collect(scopedAndFilteredTree());
    return map;
  });

  const scopeName = createMemo(() => {
    const scopeId = scopeRootId();
    if (!scopeId) return null;
    // Prefer the uncollapsed version for name stability.
    const findInTrees = (nodes: BoardNodeTree[], id: string): BoardNodeTree | null => {
      for (const n of nodes) {
        if (n.id === id) return n;
        const found = findInTrees(n.children, id);
        if (found) return found;
      }
      return null;
    };
    const found = findInTrees(delta.boardTree(), scopeId);
    return found?.name || null;
  });

  const focusIndices = createMemo(() => buildFocusIndices(nodeMap()));
  const focusSet = createMemo(() => {
    const selected = selectedNodeId();
    if (!selected) return null;
    const set = computeFocusSet(selected, nodeMap(), focusIndices());
    return set.size > 0 ? set : null;
  });

  const isCollapsed = (id: string) => collapsedFeatureIds().includes(id);
  const toggleCollapse = (id: string) => {
    setCollapsedFeatureIds((prev) => prev.includes(id) ? prev.filter(x => x !== id) : [...prev, id]);
  };

  const expandToNode = (nodeId: string) => {
    const map = uncollapsedNodeMap();
    const collapsed = new Set(collapsedFeatureIds());
    let cur = map.get(nodeId);
    let changed = false;
    while (cur?.parentId) {
      const parent = map.get(cur.parentId);
      if (!parent) break;
      if (parent.kind === 'feature' && parent.status !== 'draft' && collapsed.has(parent.id)) {
        collapsed.delete(parent.id);
        changed = true;
      }
      cur = parent;
    }
    if (changed) setCollapsedFeatureIds([...collapsed]);
  };

  const finderItems = createMemo<NodeFinderItem[]>(() => {
    const q = finderQuery().trim().toLowerCase();
    const items: NodeFinderItem[] = [];
    for (const [, node] of uncollapsedNodeMap()) {
      if (node.status === 'draft') continue; // finder is for dispatched navigation
      const name = (node.name || '').toLowerCase();
      if (!q || name.includes(q)) {
        items.push({
          id: node.id,
          name: node.name,
          kind: node.kind,
          status: node.status,
          claimedBy: node.claimedBy,
        });
      }
    }
    // Simple “best effort” ordering: prefix matches first, then alpha.
    items.sort((a, b) => {
      if (!q) return a.name.localeCompare(b.name);
      const ap = a.name.toLowerCase().startsWith(q) ? 0 : 1;
      const bp = b.name.toLowerCase().startsWith(q) ? 0 : 1;
      if (ap !== bp) return ap - bp;
      return a.name.localeCompare(b.name);
    });
    return items.slice(0, 80);
  });

  // Layout state
  const [layoutResult, setLayoutResult] = createSignal<ElkLayoutResult | null>(null);

  // Convert tree nodes to ELK input format
  const treeToLayoutNodes = (trees: BoardNodeTree[]): LayoutInputNode[] => {
    const convert = (node: BoardNodeTree, isRoot: boolean): LayoutInputNode => ({
      id: node.id,
      name: node.name,
      kind: node.kind,
      isRoot,
      blockedBy: node.blockedBy,
      validates: node.validates,
      resolves: node.resolves,
      children: node.children.map(c => convert(c, false)),
    });
    return trees.map(t => convert(t, true));
  };

  // Compute layout when tree changes
  createEffect(() => {
    if (!project.selectedProject()) {
      setLayoutResult(null);
      setAutoFitDone(false);
      return;
    }
    const trees = filteredTree();
    if (trees.length === 0) {
      setLayoutResult(null);
      setAutoFitDone(false);
      return;
    }
    const layoutNodes = treeToLayoutNodes(trees);
    computeElkLayout(layoutNodes)
      .then(setLayoutResult)
      .catch(e => console.warn('Failed to compute layout:', e));
  });

  // Auto-fit once after we get a layout for the current view.
  createEffect(() => {
    const lr = layoutResult();
    const trees = filteredTree();
    if (!lr || trees.length === 0) return;
    if (autoFitDone()) return;
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        fitToContent();
        setAutoFitDone(true);
      });
    });
  });

  // Transform ELK layout to rendering format
  const transformLayout = (elkLayout: ElkLayoutResult | null, trees: BoardNodeTree[]): LayoutTreeResult => {
    const positions = new Map<string, NodePosition>();
    const edgeRoutes: EdgeRoute[] = [];
    const checksWithValidates: { id: string; validates: string[] }[] = [];

    if (!elkLayout || trees.length === 0) {
      return { positions, width: 0, height: 0, checksWithValidates, edgeRoutes };
    }

    for (const pos of elkLayout.positions) {
      if (pos.isDummy) continue;
      const node = findNodeById(trees, pos.id);
      const name = node?.name || '';
      positions.set(pos.id, {
        x: pos.x,
        y: pos.y,
        width: pos.width,
        height: pos.height,
        lines: wrapTextToWidth(name, pos.width),
        layer: pos.layer,
      });
    }

    for (const edge of elkLayout.edges) {
      if (edge.waypoints.length < 2) continue;
      edgeRoutes.push({
        from: edge.fromId,
        to: edge.toId,
        type: edge.edgeType,
        waypoints: edge.waypoints,
      });
    }

    const flatNodes = flattenTree(trees);
    for (const node of flatNodes) {
      if (node.kind === 'check' && node.validates && node.validates.length > 0) {
        checksWithValidates.push({ id: node.id, validates: node.validates });
      }
    }

    return {
      positions,
      width: elkLayout.width,
      height: elkLayout.height,
      checksWithValidates,
      edgeRoutes,
    };
  };

  const findNodeById = (trees: BoardNodeTree[], id: string): BoardNodeTree | null => {
    for (const root of trees) {
      if (root.id === id) return root;
      const found = findInChildren(root.children, id);
      if (found) return found;
    }
    return null;
  };

  const findInChildren = (children: BoardNodeTree[], id: string): BoardNodeTree | null => {
    for (const child of children) {
      if (child.id === id) return child;
      const found = findInChildren(child.children, id);
      if (found) return found;
    }
    return null;
  };

  const flattenTree = (trees: BoardNodeTree[]): BoardNodeTree[] => {
    const result: BoardNodeTree[] = [];
    const flatten = (node: BoardNodeTree) => {
      result.push(node);
      for (const child of node.children) flatten(child);
    };
    for (const tree of trees) flatten(tree);
    return result;
  };

  const layout = createMemo(() => transformLayout(layoutResult(), filteredTree()));

  const hasDispatchedNodes = () => delta.hasDispatchedNodes();

  const TREE_LEFT_MARGIN = 24;

  // ==========================================================================
  // Helpers
  // ==========================================================================

  const findParentId = (nodes: BoardNodeTree[], targetId: string): string | null => {
    for (const node of nodes) {
      if (node.children.some((c) => c.id === targetId)) return node.id;
      const found = findParentId(node.children, targetId);
      if (found !== null) return found;
    }
    return null;
  };

  // ==========================================================================
  // Handlers
  // ==========================================================================

  const handleDoubleClick = (node: BoardNodeTree) => {
    if (node.status === 'draft') {
      setEditingNode(node);
    } else {
      setViewingNode(node);
    }
  };

  const handleFinderSelect = (nodeId: string) => {
    expandToNode(nodeId);
    setFinderOpen(false);
    setFinderQuery('');
    setSelectedNodeId(nodeId);
    setPendingJumpNodeId(nodeId);
  };

  const handleSaveEdit = async (updates: {
    name: string;
    content: string;
    validates: string[];
    validatedBy: string[];
    blockedBy: string[];
  }) => {
    const node = editingNode();
    if (!node) return;
    await delta.updateBoardNode(node.id, updates);
    setEditingNode(null);
  };

  const handleContextMenu = (e: MouseEvent, node: BoardNodeTree | null) => {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({ x: e.clientX, y: e.clientY, node, isBackground: node === null });
  };

  const hideContextMenu = () => setContextMenu(null);

  const handleAddChild = () => {
    const cm = contextMenu();
    if (cm && cm.node) {
      setNewNodeParentId(cm.node.id);
      setNewNodeKind('task');
      setShowNewPrompt(true);
      hideContextMenu();
      setTimeout(() => newNodeInputRef?.focus(), 50);
    }
  };

  const handleAddSibling = () => {
    const cm = contextMenu();
    if (cm && cm.node) {
      setNewNodeParentId(findParentId(delta.boardTree(), cm.node.id));
      setNewNodeKind('task');
      setShowNewPrompt(true);
      hideContextMenu();
      setTimeout(() => newNodeInputRef?.focus(), 50);
    }
  };

  const handleDelete = async () => {
    const cm = contextMenu();
    if (cm && cm.node) {
      const kindLabel = cm.node.kind;
      const hasChildren = cm.node.children.length > 0;
      const description = hasChildren
        ? `"${cm.node.name}" and all its children`
        : `"${cm.node.name}"`;
      const confirmed = await window.confirmDialog?.delete(description, kindLabel);
      if (confirmed) {
        await delta.deleteBoardNode(cm.node.id);
      }
      hideContextMenu();
    }
  };

  const handleAddRootFeature = () => {
    setNewNodeParentId(null);
    setNewNodeKind('feature');
    setShowNewPrompt(true);
    hideContextMenu();
    setTimeout(() => newNodeInputRef?.focus(), 50);
  };

  const handleAddRootCheck = () => {
    setNewNodeParentId(null);
    setNewNodeKind('check');
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

    await delta.createBoardNode({
      parentId: newNodeParentId(),
      name,
      kind: newNodeKind(),
    });

    setShowNewPrompt(false);
    setNewNodeName('');
  };

  const handleStartShepherd = async () => {
    if (!delta.hasDraftNodes() || delta.shepherdStartPending()) {
      if (!delta.hasDraftNodes()) {
        window.toast?.info('No draft nodes to start');
      }
      return;
    }
    await delta.startShepherdRun();
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
      const isTyping = target.tagName === 'INPUT' || target.tagName === 'TEXTAREA';

      if (!isTyping && e.key === '/') {
        e.preventDefault();
        if (!finderOpen() && delta.hasDispatchedNodes()) {
          setFinderOpen(true);
        }
        return;
      }

      if (e.key === 'Escape') {
        if (finderOpen()) setFinderOpen(false);
        else if (editingNode()) setEditingNode(null);
        else if (viewingNode()) { setViewingNode(null); setSelectedNodeId(null); }
        else if (showNewPrompt()) setShowNewPrompt(false);
        else if (contextMenu()) hideContextMenu();
        else if (selectedNodeId()) setSelectedNodeId(null);
      }
    };

    document.addEventListener('keydown', handler);
    onCleanup(() => document.removeEventListener('keydown', handler));
  });

  // Listen for radial menu actions
  createEffect(() => {
    const cleanupStart = onEvent('radial-start-shepherd', () => handleStartShepherd());
    const cleanupDeliver = onEvent('radial-deliver', () => setShowDeliveryDialog(true));
    const cleanupOpenIde = onEvent('radial-open-ide', () => handleOpenInIde());
    const cleanupForkDialog = onEvent('open-fork-dialog', () => setShowForkDialog(true));

    onCleanup(() => {
      cleanupStart();
      cleanupDeliver();
      cleanupOpenIde();
      cleanupForkDialog();
    });
  });

  // Pan and zoom
  const calcFitZoom = (treeWidth: number, treeHeight: number, panelWidth: number, panelHeight: number): number => {
    if (treeWidth <= 0 || treeHeight <= 0) return 0.25;
    const padding = TREE_LEFT_MARGIN * 2;
    const availableWidth = Math.max(panelWidth - padding, 100);
    const availableHeight = Math.max(panelHeight - padding, 100);
    const fitZoom = Math.min(availableWidth / treeWidth, availableHeight / treeHeight);
    return Math.max(0.1, Math.min(1, fitZoom));
  };

  const clampPan = (
    p: { x: number; y: number },
    treeWidth: number,
    treeHeight: number,
    panelWidth: number,
    panelHeight: number,
    z: number
  ): { x: number; y: number } => {
    const scaledTreeWidth = treeWidth * z;
    const scaledTreeHeight = treeHeight * z;
    const minVisible = Math.max(50, Math.min(scaledTreeWidth, scaledTreeHeight) * 0.2);
    const maxX = panelWidth - minVisible - TREE_LEFT_MARGIN;
    const minX = -(scaledTreeWidth - minVisible);
    const maxY = (panelHeight / 2) - minVisible;
    const minY = -(scaledTreeHeight / 2) + minVisible - (panelHeight / 2);
    return {
      x: Math.max(minX, Math.min(maxX, p.x)),
      y: Math.max(minY, Math.min(maxY, p.y)),
    };
  };

  const fitToContent = () => {
    const rect = canvasRef?.getBoundingClientRect();
    if (!rect) return;
    const l = layout();
    if (l.width <= 0 || l.height <= 0 || l.positions.size === 0) return;

    // Fit to actual visible node bounds (more reliable than raw ELK width/height).
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const [, p] of l.positions) {
      minX = Math.min(minX, p.x);
      minY = Math.min(minY, p.y);
      maxX = Math.max(maxX, p.x + p.width);
      maxY = Math.max(maxY, p.y + p.height);
    }
    if (!Number.isFinite(minX) || !Number.isFinite(minY) || !Number.isFinite(maxX) || !Number.isFinite(maxY)) {
      return;
    }

    const boundsW = Math.max(1, maxX - minX);
    const boundsH = Math.max(1, maxY - minY);
    const z = calcFitZoom(boundsW, boundsH, rect.width, rect.height);
    const vp = viewportCenterPoint(rect, TREE_LEFT_MARGIN);
    const contentCenterRel = {
      x: minX + boundsW / 2,
      y: contentYToRelative(minY + boundsH / 2, l.height),
    };
    const newPan = panForContentPoint(vp, contentCenterRel, z);
    const clamped = clampPan(newPan, l.width, l.height, rect.width, rect.height, z);
    setZoom(z);
    setPan(clamped);
  };

  const zoomAroundCenter = (newZoom: number) => {
    const rect = canvasRef?.getBoundingClientRect();
    if (!rect) return;
    const oldZoom = zoom();
    const vp = viewportCenterPoint(rect, TREE_LEFT_MARGIN);
    const newPan = zoomAroundViewportPoint(vp, oldZoom, newZoom, pan());
    const l = layout();
    const clamped = clampPan(newPan, l.width, l.height, rect.width, rect.height, newZoom);
    setZoom(newZoom);
    setPan(clamped);
  };

  const zoomBy = (factor: number) => {
    const rect = canvasRef?.getBoundingClientRect();
    const l = layout();
    const minZoom = rect ? calcFitZoom(l.width, l.height, rect.width, rect.height) : 0.25;
    const z = Math.max(minZoom, Math.min(3, zoom() * factor));
    zoomAroundCenter(z);
  };

  const centerOnNode = (nodeId: string, targetZoom?: number) => {
    const rect = canvasRef?.getBoundingClientRect();
    if (!rect) return;
    const l = layout();
    const pos = l.positions.get(nodeId);
    if (!pos) return;

    const minZoom = calcFitZoom(l.width, l.height, rect.width, rect.height);
    const desiredZoom = targetZoom !== undefined ? Math.max(minZoom, Math.min(3, targetZoom)) : zoom();
    const nodeCenter = {
      x: pos.x + pos.width / 2,
      y: contentYToRelative(pos.y + pos.height / 2, l.height),
    };
    const vp = viewportCenterPoint(rect, TREE_LEFT_MARGIN);
    const rawPan = panForContentPoint(vp, nodeCenter, desiredZoom);
    const clamped = clampPan(rawPan, l.width, l.height, rect.width, rect.height, desiredZoom);
    setZoom(desiredZoom);
    setPan(clamped);
  };

  // Perform a pending jump once the layout includes the node (e.g., after expanding a collapsed feature).
  createEffect(() => {
    const nodeId = pendingJumpNodeId();
    if (!nodeId) return;
    const l = layout();
    if (!l.positions.has(nodeId)) return;
    const target = zoom() < 0.6 ? 0.6 : undefined;
    centerOnNode(nodeId, target);
    setPendingJumpNodeId(null);
  });

  const handleWheel = (e: WheelEvent) => {
    e.preventDefault();

    if (e.ctrlKey) {
      const zoomFactor = e.deltaY < 0 ? 1.1 : 0.9;
      const oldZoom = zoom();
      const section = canvasRef;
      const rect = section?.getBoundingClientRect();
      const l = layout();
      const minZoom = rect ? calcFitZoom(l.width, l.height, rect.width, rect.height) : 0.25;
      const newZoom = Math.max(minZoom, Math.min(3, oldZoom * zoomFactor));
      if (rect) {
        const cursorX = e.clientX - rect.left - TREE_LEFT_MARGIN;
        const cursorY = e.clientY - rect.top - rect.height / 2;
        const currentPan = pan();
        const contentX = (cursorX - currentPan.x) / oldZoom;
        const contentY = (cursorY - currentPan.y) / oldZoom;
        setPan({
          x: cursorX - contentX * newZoom,
          y: cursorY - contentY * newZoom,
        });
      }
      setZoom(newZoom);
    } else {
      const l = layout();
      const rect = canvasRef?.getBoundingClientRect();
      const rawPan = {
        x: pan().x - e.deltaX,
        y: pan().y - e.deltaY,
      };
      const clampedPan = rect
        ? clampPan(rawPan, l.width, l.height, rect.width, rect.height, zoom())
        : rawPan;
      setPan(clampedPan);
    }
  };

  const handleMouseDown = (e: MouseEvent) => {
    if (e.button === 1 || (e.button === 0 && e.altKey)) {
      e.preventDefault();
      setIsPanning(true);
      const p = pan();
      setPanStart({ x: e.clientX - p.x, y: e.clientY - p.y });
    }
  };

  const handleMouseMove = (e: MouseEvent) => {
    if (isPanning()) {
      const rawPan = {
        x: e.clientX - panStart().x,
        y: e.clientY - panStart().y,
      };
      const rect = canvasRef?.getBoundingClientRect();
      const l = layout();
      const clampedPan = rect
        ? clampPan(rawPan, l.width, l.height, rect.width, rect.height, zoom())
        : rawPan;
      setPan(clampedPan);
    }
  };

  const handleMouseUp = () => {
    setIsPanning(false);
  };

  // ==========================================================================
  // Render
  // ==========================================================================

  const NoProjectSelected = () => (
    <div class="flex-1 flex flex-col items-center justify-center bg-pasture-900">
      <div
        class="w-16 h-16 mb-4 rounded-none flex items-center justify-center"
        style={{
          background: amber(0.05),
          border: `1px solid ${amber(0.1)}`,
        }}
      >
        <svg class="w-8 h-8 text-wool-700" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.25" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
        </svg>
      </div>
      <p class="text-sm text-wool-500 mb-4">No project selected</p>
      <button
        onClick={() => project.openProjectSetup()}
        class="px-3 py-1.5 rounded-none text-[12px] font-medium"
        style={{
          background: amber(0.15),
          border: `1px solid ${amber(0.25)}`,
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
      <p class="text-[12px] text-wool-600 mb-1">No specs yet</p>
      <p class="text-[11px] text-wool-700">Right-click to add</p>
    </div>
  );

  return (
    <Show when={project.selectedProject()} fallback={<NoProjectSelected />}>
      <div class="flex-1 flex flex-col overflow-hidden">
        {/* Canvas Toolbar */}
        <CanvasToolbar
          granularity={granularity}
          setGranularity={setGranularity}
          liveFilters={liveFilters}
          setLiveFilters={setLiveFilters}
          edgeToggles={edgeToggles}
          setEdgeToggles={setEdgeToggles}
          scopeName={scopeName}
          clearScope={() => { setScopeRootId(null); setCollapsedFeatureIds([]); setAutoFitDone(false); }}
          hasLiveTree={hasDispatchedNodes()}
        />

        <div class="flex-1 flex flex-col overflow-hidden bg-pasture-900">
        {/* Canvas Area */}
        <div
          ref={canvasRef}
          class="flex-1 overflow-hidden relative"
          style={{
            cursor: isPanning() ? 'grabbing' : 'default',
            'background-image': `radial-gradient(circle, rgba(90, 85, 80, 0.10) 1px, transparent 1px)`,
            'background-size': '26px 26px',
            'background-position': '12px 12px',
          }}
          onClick={(e) => {
            if (e.target === e.currentTarget) {
              setSelectedNodeId(null);
              setViewingNode(null);
            }
          }}
          onWheel={handleWheel}
          onMouseDown={handleMouseDown}
          onMouseMove={handleMouseMove}
          onMouseUp={handleMouseUp}
          onMouseLeave={handleMouseUp}
        >
          {/* Subtle grain overlay */}
          <div
            class="absolute inset-0 pointer-events-none"
            style={{
              'background-image': "url(\"data:image/svg+xml,%3Csvg viewBox='0 0 256 256' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='noise'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.8' numOctaves='4' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23noise)'/%3E%3C/svg%3E\")",
              opacity: 0.02,
              'mix-blend-mode': 'overlay',
            }}
          />

          {/* Zoom controls */}
          <div
            class="absolute bottom-3 right-3 z-20 flex items-center gap-1.5 px-2 py-1 rounded-none text-[10px] font-medium"
            style={{ background: 'rgba(30, 30, 30, 0.85)', border: '1px solid rgba(64, 64, 64, 0.4)' }}
          >
            <button
              type="button"
              class="px-1.5 py-0.5 rounded-none hover:bg-white/5 text-wool-500"
              onClick={fitToContent}
              title="Fit"
            >
              Fit
            </button>
            <button type="button" class="w-6 h-5 rounded-none hover:bg-white/5 text-wool-500" onClick={() => zoomBy(0.9)} title="Zoom out">
              -
            </button>
            <button type="button" class="w-6 h-5 rounded-none hover:bg-white/5 text-wool-500" onClick={() => zoomBy(1.1)} title="Zoom in">
              +
            </button>
            <button type="button" class="px-1.5 py-0.5 rounded-none hover:bg-white/5 text-wool-500" onClick={() => zoomAroundCenter(1)} title="100%">
              100%
            </button>
            <span class="text-wool-600 ml-1">{Math.round(zoom() * 100)}%</span>
          </div>

          <Show
            when={filteredTree().length > 0}
            fallback={<EmptyTreeState />}
          >
            {/* Single tree container */}
            <div
              class="absolute inset-0"
              style={{ padding: '8px' }}
              onContextMenu={(e) => {
                handleContextMenu(e, null);
              }}
            >
              <div
                class="tree-section flex-1 rounded-none relative overflow-hidden h-full"
                onClick={(e) => {
                  if (e.target === e.currentTarget) {
                    setSelectedNodeId(null);
                    setViewingNode(null);
                  }
                }}
              >
                <div
                  class="absolute"
                  style={{
                    left: `${TREE_LEFT_MARGIN}px`,
                    top: '50%',
                    transform: `translate(${pan().x}px, calc(-50% + ${pan().y}px)) scale(${zoom()})`,
                    'transform-origin': '0 50%',
                  }}
                >
                  <div class="relative" style={{ width: `${layout().width}px`, height: `${layout().height}px` }}>
                    <DependencyConnectors
                      edgeRoutes={layout().edgeRoutes}
                      edgeToggles={edgeToggles}
                      focusSet={focusSet}
                      selectedNodeId={selectedNodeId}
                    />
                    <For each={flattenTree(filteredTree())}>
                      {(node) => {
                        const pos = () => layout().positions.get(node.id);
                        return (
                          <Show when={pos()}>
                            <BoardNodeCard
                              node={node}
                              position={pos()!}
                              selected={selectedNodeId() === node.id}
                              dimmed={!!focusSet() && !focusSet()!.has(node.id)}
                              collapsed={node.kind === 'feature' && isCollapsed(node.id)}
                              hiddenCounts={hiddenCountsByFeatureId().get(node.id) || null}
                              onSelect={() => {
                                setSelectedNodeId(node.id);
                              }}
                              onDoubleClick={() => handleDoubleClick(node)}
                              onContextMenu={(e) => {
                                if (node.status === 'draft') {
                                  handleContextMenu(e, node);
                                }
                              }}
                              onEnterScope={() => {
                                if (node.kind === 'feature' && node.status !== 'draft') {
                                  setScopeRootId(node.id);
                                  setCollapsedFeatureIds([]);
                                  setAutoFitDone(false);
                                }
                              }}
                              onToggleCollapse={() => {
                                if (node.kind === 'feature' && node.status !== 'draft') toggleCollapse(node.id);
                              }}
                              nodeMap={nodeMap()}
                            />
                          </Show>
                        );
                      }}
                    </For>
                  </div>
                </div>
              </div>
            </div>
          </Show>
        </div>

        <NodeFinder
          open={finderOpen()}
          query={finderQuery()}
          setQuery={setFinderQuery}
          items={finderItems()}
          onSelect={handleFinderSelect}
          onClose={() => { setFinderOpen(false); setFinderQuery(''); }}
        />

        {/* Context Menu */}
        <Show when={contextMenu()}>
          <div
            class="fixed inset-0 z-[99]"
            onClick={hideContextMenu}
            onContextMenu={(e) => { e.preventDefault(); hideContextMenu(); }}
          />
          <div
            class="fixed z-[100] py-1 min-w-[140px] rounded-none overflow-hidden"
            style={{
              left: `${contextMenu()!.x}px`,
              top: `${contextMenu()!.y}px`,
              background: 'linear-gradient(180deg, #2d2d2d 0%, #262626 100%)',
              border: '1px solid rgba(64, 64, 64, 0.6)',
              'box-shadow': '0 4px 16px rgba(0,0,0,0.4)',
            }}
          >
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
            <Show when={contextMenu()!.isBackground}>
              <button class="w-full px-3 py-1.5 text-left text-[11px] text-wool-200 hover:bg-white/5" onClick={handleAddRootFeature}>
                Add milestone
              </button>
              <button class="w-full px-3 py-1.5 text-left text-[11px] text-sage hover:bg-sage/10" onClick={handleAddRootCheck}>
                Add check
              </button>
            </Show>
          </div>
        </Show>

        {/* New Node Prompt */}
        <Show when={showNewPrompt()}>
          {(() => {
            const isCheck = () => newNodeKind() === 'check';
            const isFeature = () => newNodeKind() === 'feature';
            const typeLabel = () => isCheck() ? 'Check' : isFeature() ? 'Milestone' : 'Issue';

            return (
              <div
                class="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
                onClick={(e) => {
                  if (e.target === e.currentTarget) setShowNewPrompt(false);
                }}
              >
                <div
                  class="w-[360px] rounded-none shadow-xl"
                  style={{
                    background: 'var(--pasture-800)',
                    border: '1px solid var(--pasture-600)',
                  }}
                >
                  <div class="p-4 border-b border-pasture-600">
                    <div class="flex items-center gap-3">
                      <div
                        class="w-9 h-9 rounded-none flex items-center justify-center flex-shrink-0"
                        style={{
                          background: isCheck() ? sage(0.15) : amber(0.12),
                          border: `1px solid ${isCheck() ? sage(0.25) : amber(0.2)}`,
                        }}
                      >
                        <Show when={isCheck()} fallback={
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
                          {isCheck() ? 'Add a check' : isFeature() ? 'Add a milestone' : 'Add an issue'}
                        </p>
                      </div>
                    </div>
                  </div>

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
                          placeholder={isCheck() ? 'e.g., API returns valid JSON' : isFeature() ? 'e.g., Improve checkout conversion' : 'e.g., Build authentication flow'}
                          class="w-full px-3 py-2 rounded-none text-sm bg-pasture-900 border text-wool-100 placeholder-wool-600 focus:outline-none focus:ring-2 focus:ring-amber-500/30"
                          style={{
                            'font-family': 'system-ui, -apple-system, sans-serif',
                            'border-color': isCheck() ? sage(0.4) : 'var(--pasture-600)',
                          }}
                        />
                        <p class="text-[11px] text-wool-500">Press Enter to create, Escape to cancel</p>
                      </div>
                    </form>
                  </div>

                  <div class="px-4 py-3 border-t border-pasture-600 flex justify-end gap-2">
                    <button
                      onClick={() => setShowNewPrompt(false)}
                      class="px-3 py-1.5 rounded-none text-xs font-medium text-wool-400 hover:text-wool-200 hover:bg-white/5"
                    >
                      Cancel
                    </button>
                    <button
                      onClick={handleCreateNode}
                      disabled={!newNodeName().trim()}
                      class="px-3 py-1.5 rounded-none text-xs font-medium disabled:opacity-40"
                      style={{
                        background: isCheck() ? sage(0.2) : 'var(--amber-500)',
                        color: isCheck() ? 'var(--sage)' : 'var(--pasture-900)',
                        border: isCheck() ? `1px solid ${sage(0.3)}` : 'none',
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

        {/* Edit Modal (draft nodes only) */}
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

        {/* Node Detail Modal (non-draft, read-only) */}
        <Show when={viewingNode()}>
          {(node) => {
            const nodeKind = () => node().kind;
            const isCheck = () => nodeKind() === 'check';
            const typeLabel = () => isCheck() ? 'Check' : node().kind === 'feature' ? 'Milestone' : 'Issue';

            const statusLabel = () => {
              switch (node().status) {
                case 'draft': return 'Draft';
                case 'pending': return 'Pending';
                case 'working': return 'Working';
                case 'done': return 'Done';
                case 'awaiting_check': return 'Ready for Validation';
                case 'validated': return 'Validated';
                case 'needs_repair': return 'Needs Repair';
                case 'failed': return 'Failed';
                default: return node().status;
              }
            };

            const statusColor = () => {
              switch (node().status) {
                case 'draft': return 'var(--sky-500)';
                case 'pending': return 'var(--wool-500)';
                case 'working': return 'var(--amber-500)';
                case 'done': return 'var(--sage)';
                case 'awaiting_check': return 'var(--amber-400)';
                case 'validated': return 'var(--sage)';
                case 'needs_repair': return 'var(--golden)';
                case 'failed': return 'var(--terra)';
                default: return 'var(--wool-500)';
              }
            };

            return (
              <div
                class="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
                onClick={(e) => { if (e.target === e.currentTarget) { setViewingNode(null); setSelectedNodeId(null); } }}
              >
                <div
                  class="w-[560px] max-h-[85vh] flex flex-col rounded-none shadow-xl"
                  style={{
                    background: 'linear-gradient(180deg, rgba(36,36,36,0.98) 0%, rgba(26,26,26,0.98) 100%)',
                    border: '1px solid rgba(255,255,255,0.10)',
                    'box-shadow': '0 30px 70px rgba(0,0,0,0.65), 0 0 0 1px rgba(255,255,255,0.05)',
                    'backdrop-filter': 'blur(18px)',
                  }}
                >
                  <div class="p-4 border-b border-white/5 flex items-start justify-between">
                    <div class="flex items-center gap-3">
                      <div
                        class="w-10 h-10 rounded-none flex items-center justify-center flex-shrink-0"
                        style={{
                          background: isCheck() ? sage(0.15) : amber(0.12),
                          border: `1px solid ${isCheck() ? sage(0.25) : amber(0.2)}`,
                        }}
                      >
                        <Show when={isCheck()} fallback={
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
                            style={{ color: isCheck() ? 'var(--sage)' : 'var(--amber-500)', opacity: 0.8 }}
                          >
                            {typeLabel()}
                          </span>
                          <span class="text-[9px] font-medium px-1.5 py-0.5 rounded" style={{ color: statusColor(), background: `color-mix(in srgb, ${statusColor()} 15%, transparent)` }}>
                            {statusLabel()}
                          </span>
                        </div>
                        <h2 class={`-mt-0.5 ${node().kind === 'feature' && node().parentId === null ? 'text-[18px] font-semibold italic' : 'text-sm font-semibold'} text-wool-100`}>
                          {node().name || 'Untitled'}
                        </h2>
                      </div>
                    </div>
                    <button
                      onClick={() => { setViewingNode(null); setSelectedNodeId(null); }}
                      class="p-1 rounded-none text-wool-500 hover:text-wool-300 hover:bg-white/5"
                    >
                      <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
                      </svg>
                    </button>
                  </div>

                  <div class="p-4 space-y-4 overflow-y-auto flex-1">
                    <Show when={node().content}>
                      <div class="space-y-1.5">
                        <label class="text-xs font-medium text-wool-300">
                          {isCheck() ? 'Acceptance Criteria' : 'Description'}
                        </label>
                        <div
                          class="w-full px-4 py-3 rounded-none bg-pasture-900/50 border border-pasture-700 overflow-auto"
                          style={{ 'max-height': '400px' }}
                        >
                          <Markdown content={node().content} compact />
                        </div>
                      </div>
                    </Show>

                    <Show when={isCheck() && node().validates.length > 0}>
                      <div class="space-y-1.5">
                        <label class="text-xs font-medium" style={{ color: 'var(--sage)' }}>Validates Issues</label>
                        <div class="flex flex-wrap gap-1.5">
                          <For each={node().validates}>
                            {(taskId) => (
                              <button
                                type="button"
                                class="px-2 py-0.5 rounded-none text-[11px] font-mono hover:brightness-110 transition"
                                style={{ background: sage(0.14), color: 'var(--sage)', border: `1px solid ${sage(0.28)}` }}
                                onClick={() => {
                                  setViewingNode(null);
                                  setSelectedNodeId(taskId);
                                  setPendingJumpNodeId(taskId);
                                }}
                              >
                                {uncollapsedNodeMap().get(taskId)?.name || taskId}
                              </button>
                            )}
                          </For>
                        </div>
                      </div>
                    </Show>

                    <Show when={node().lastCommitSha}>
                      <div class="space-y-1.5">
                        <label class="text-xs font-medium text-wool-300">Last Commit</label>
                        <code class="px-2 py-1 rounded-none text-xs font-mono bg-pasture-900/50 border border-pasture-700 text-wool-300">
                          {node().lastCommitSha?.slice(0, 7)}
                        </code>
                      </div>
                    </Show>

                    <Show when={node().completedAt}>
                      <div class="space-y-1.5">
                        <label class="text-xs font-medium text-wool-300">Completed</label>
                        <div class="text-xs text-wool-400">
                          {new Date(node().completedAt!).toLocaleString()}
                        </div>
                      </div>
                    </Show>

                    <div class="space-y-2 pt-2 border-t border-pasture-700/50">
                      <label class="text-xs font-medium text-wool-400">Relationships</label>

                      {(() => {
                        const pill = (id: string, variant: 'neutral' | 'blocked' | 'validated' | 'worker' | 'parent') => {
                          const n = uncollapsedNodeMap().get(id);
                          const name = n?.name || id;
                          const kind = n?.kind || 'task';
                          const status = n?.status || null;
                          const bg =
                            variant === 'blocked' ? 'rgba(196,92,74,0.12)'
                            : variant === 'validated' ? 'rgba(125,153,112,0.14)'
                            : variant === 'worker' ? 'rgba(212,165,116,0.14)'
                            : 'rgba(255,255,255,0.06)';
                          const border =
                            variant === 'blocked' ? 'rgba(196,92,74,0.25)'
                            : variant === 'validated' ? 'rgba(125,153,112,0.25)'
                            : variant === 'worker' ? 'rgba(212,165,116,0.25)'
                            : 'rgba(255,255,255,0.10)';
                          const fg =
                            variant === 'blocked' ? 'var(--terra-light)'
                            : variant === 'validated' ? 'var(--sage-light, var(--sage))'
                            : variant === 'worker' ? 'var(--amber-300)'
                            : 'var(--wool-300)';
                          const kindLetter = kind === 'feature' ? 'M' : kind === 'check' ? 'C' : 'I';
                          const statusDot =
                            status === 'working' ? 'var(--amber-500)'
                            : status === 'done' || status === 'validated' ? 'var(--sage)'
                            : status === 'needs_repair' ? 'var(--golden)'
                            : status === 'failed' ? 'var(--terra)'
                            : status === 'awaiting_check' ? 'var(--amber-400)'
                            : status === 'pending' ? 'var(--wool-600)'
                            : null;
                          return (
                            <button
                              type="button"
                              class="inline-flex items-center gap-1.5 px-2 py-1 rounded-none text-[11px] font-medium max-w-full hover:brightness-110 transition"
                              style={{ background: bg, border: `1px solid ${border}`, color: fg }}
                              title={id}
                              onClick={() => {
                                setViewingNode(null);
                                setSelectedNodeId(id);
                                setPendingJumpNodeId(id);
                              }}
                            >
                              <span class="text-[9px] font-semibold opacity-70">{kindLetter}</span>
                              <span class="truncate max-w-[220px]">{name}</span>
                              <Show when={statusDot}>
                                <span class="w-1.5 h-1.5 rounded-none" style={{ background: statusDot || 'transparent' }} />
                              </Show>
                            </button>
                          );
                        };

                        return (
                          <>
                            <Show when={node().parentId}>
                              <div class="flex items-start gap-2 text-[11px]">
                                <span class="text-wool-500 w-16 pt-0.5">Parent:</span>
                                <div class="flex flex-wrap gap-1">
                                  {pill(node().parentId!, 'parent')}
                                </div>
                              </div>
                            </Show>

                            <Show when={node().children.length > 0}>
                              <div class="flex items-start gap-2 text-[11px]">
                                <span class="text-wool-500 w-16 pt-0.5">Children:</span>
                                <div class="flex flex-wrap gap-1">
                                  <For each={node().children}>
                                    {(child) => pill(child.id, 'neutral')}
                                  </For>
                                </div>
                              </div>
                            </Show>

                            <Show when={node().blockedBy.length > 0}>
                              <div class="flex items-start gap-2 text-[11px]">
                                <span class="text-wool-500 w-16 pt-0.5">Blocked:</span>
                                <div class="flex flex-wrap gap-1">
                                  <For each={node().blockedBy}>
                                    {(blockerId) => pill(blockerId, 'blocked')}
                                  </For>
                                </div>
                              </div>
                            </Show>

                            <Show when={!isCheck() && node().validatedBy.length > 0}>
                              <div class="flex items-start gap-2 text-[11px]">
                                <span class="text-wool-500 w-16 pt-0.5">Checks:</span>
                                <div class="flex flex-wrap gap-1">
                                  <For each={node().validatedBy}>
                                    {(checkId) => pill(checkId, 'validated')}
                                  </For>
                                </div>
                              </div>
                            </Show>

                            <Show when={node().resolves}>
                              <div class="flex items-start gap-2 text-[11px]">
                                <span class="text-wool-500 w-16 pt-0.5">Resolves:</span>
                                <div class="flex flex-wrap gap-1">
                                  {pill(node().resolves!, 'validated')}
                                </div>
                              </div>
                            </Show>

                            <Show when={node().claimedBy}>
                              <div class="flex items-start gap-2 text-[11px]">
                                <span class="text-wool-500 w-16 pt-0.5">Worker:</span>
                                <div class="flex flex-wrap gap-1">
                                  <span class="inline-flex items-center gap-1.5 px-2 py-1 rounded-none text-[11px] font-medium"
                                    style={{ background: 'rgba(212,165,116,0.14)', border: '1px solid rgba(212,165,116,0.25)', color: 'var(--amber-300)' }}
                                  >
                                    {node().claimedBy}
                                  </span>
                                </div>
                              </div>
                            </Show>
                          </>
                        );
                      })()}
                    </div>

                    <div class="space-y-1.5 pt-2 border-t border-pasture-700/50">
                      <label class="text-xs font-medium text-wool-500">Node ID</label>
                      <code class="block px-2 py-1 rounded-none text-[10px] font-mono bg-pasture-900/30 border border-pasture-700/50 text-wool-500 truncate">{node().id}</code>
                    </div>
                  </div>

                  <div class="px-4 py-3 border-t border-pasture-600 flex justify-end">
                    <button
                      onClick={() => { setViewingNode(null); setSelectedNodeId(null); }}
                      class="px-3 py-1.5 rounded-none text-xs font-medium text-wool-400 hover:text-wool-200 hover:bg-white/5"
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
              class="w-6 h-6 rounded-none animate-spin"
              style={{ border: `2px solid ${amber(0.2)}`, 'border-top-color': 'var(--amber-500)' }}
            />
          </div>
        </Show>
        </div>

        {/* Delivery Dialog */}
        <Show when={showDeliveryDialog()}>
          <DeliveryDialog onClose={() => setShowDeliveryDialog(false)} />
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
