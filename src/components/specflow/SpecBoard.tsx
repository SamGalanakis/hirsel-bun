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
// Layout Constants
// =============================================================================

const NODE_WIDTH = 140;
const NODE_HEIGHT = 32;
const H_GAP = 24;
const V_GAP = 20;
const TREE_PADDING = 32;
const DIVIDER_WIDTH = 40;

interface NodePosition {
  x: number;
  y: number;
}

// =============================================================================
// Tree Layout Algorithm
// =============================================================================

function layoutTree<T extends { id: string; children: T[] }>(
  roots: T[],
  startX: number = 0
): { positions: Map<string, NodePosition>; width: number; height: number } {
  const positions = new Map<string, NodePosition>();

  if (roots.length === 0) {
    return { positions, width: NODE_WIDTH, height: NODE_HEIGHT };
  }

  let maxHeight = 0;

  function layoutNode(node: T, depth: number, xOffset: number): number {
    if (node.children.length === 0) {
      const y = depth * (NODE_HEIGHT + V_GAP);
      positions.set(node.id, { x: xOffset + NODE_WIDTH / 2, y });
      maxHeight = Math.max(maxHeight, y + NODE_HEIGHT);
      return NODE_WIDTH;
    }

    let childX = xOffset;
    let totalWidth = 0;
    for (const child of node.children) {
      const childWidth = layoutNode(child, depth + 1, childX);
      childX += childWidth + H_GAP;
      totalWidth += childWidth + H_GAP;
    }
    totalWidth -= H_GAP;

    const nodeX = xOffset + totalWidth / 2;
    const y = depth * (NODE_HEIGHT + V_GAP);
    positions.set(node.id, { x: nodeX, y });
    maxHeight = Math.max(maxHeight, y + NODE_HEIGHT);

    return Math.max(NODE_WIDTH, totalWidth);
  }

  let currentX = startX;
  for (const root of roots) {
    const width = layoutNode(root, 0, currentX);
    currentX += width + H_GAP * 2;
  }

  return {
    positions,
    width: Math.max(NODE_WIDTH, currentX - startX - H_GAP * 2),
    height: maxHeight,
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

  const getBorderColor = () => {
    if (isNew()) return 'rgba(125, 153, 112, 0.6)';
    if (isModified()) return 'rgba(212, 165, 116, 0.6)';
    if (props.selected) return 'rgba(212, 165, 116, 0.5)';
    if (isProject()) return 'rgba(212, 165, 116, 0.3)';
    return 'rgba(64, 64, 64, 0.4)';
  };

  const getBgColor = () => {
    if (isNew()) return 'rgba(125, 153, 112, 0.08)';
    if (isModified()) return 'rgba(212, 165, 116, 0.08)';
    if (isProject()) return 'rgba(45, 42, 38, 0.95)';
    return 'rgba(36, 36, 36, 0.9)';
  };

  return (
    <div
      class={`absolute cursor-pointer group ${props.selected ? 'z-10' : ''}`}
      style={{
        left: `${props.position.x - NODE_WIDTH / 2}px`,
        top: `${props.position.y}px`,
        width: `${NODE_WIDTH}px`,
        height: `${NODE_HEIGHT}px`,
      }}
      onClick={() => props.onSelect()}
      onDblClick={() => props.onDoubleClick()}
      onContextMenu={(e) => props.onContextMenu(e)}
    >
      <div
        class="h-full rounded flex items-center gap-1.5 px-2"
        style={{
          background: getBgColor(),
          border: `1px solid ${getBorderColor()}`,
          'box-shadow': props.selected ? '0 2px 8px rgba(0,0,0,0.25)' : undefined,
        }}
      >
        {/* Type indicator */}
        <div
          class="w-1.5 h-1.5 rounded-full flex-shrink-0"
          style={{
            background: isProject() ? 'var(--amber-500)' : isEval() ? 'var(--sage)' : 'var(--amber-500)',
            opacity: isProject() ? 1 : 0.7,
          }}
        />

        {/* Name */}
        <span
          class="text-[11px] font-medium truncate flex-1"
          style={{ color: isProject() ? 'var(--wool-100)' : 'var(--wool-200)' }}
        >
          {props.node.name}
        </span>

        {/* Delta badge */}
        <Show when={isNew()}>
          <span class="text-[8px] px-1 py-px rounded bg-sage/20 text-sage font-medium">+</span>
        </Show>
        <Show when={isModified()}>
          <span class="text-[8px] px-1 py-px rounded bg-amber-500/20 text-amber-400 font-medium">~</span>
        </Show>
      </div>

      {/* Left edge indicator for delta */}
      <Show when={props.showDelta && (isNew() || isModified())}>
        <div
          class="absolute left-0 top-1 bottom-1 w-0.5 rounded-full"
          style={{ background: isNew() ? 'var(--sage)' : 'var(--amber-500)' }}
        />
      </Show>
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

  const style = () => statusStyles[props.node.status] || statusStyles.pending;
  const isWorking = () => props.node.status === 'working';

  return (
    <div
      class="absolute"
      style={{
        left: `${props.position.x - NODE_WIDTH / 2}px`,
        top: `${props.position.y}px`,
        width: `${NODE_WIDTH}px`,
        height: `${NODE_HEIGHT}px`,
      }}
    >
      <div
        class={`h-full rounded flex items-center gap-1.5 px-2 ${isDeleted() ? 'opacity-40' : ''}`}
        style={{
          background: isDeleted() ? 'rgba(196, 92, 74, 0.08)' : style().bg,
          border: `1px solid ${isDeleted() ? 'rgba(196, 92, 74, 0.4)' : style().border}`,
        }}
      >
        {/* Status dot */}
        <div class="relative flex-shrink-0">
          <div
            class={`w-1.5 h-1.5 rounded-full ${isWorking() ? 'animate-pulse' : ''}`}
            style={{ background: style().dot }}
          />
        </div>

        {/* Name */}
        <span class="text-[11px] font-medium text-wool-300 truncate flex-1">
          {props.node.name}
        </span>

        {/* Status indicator */}
        <Show when={props.node.status === 'done'}>
          <svg class="w-3 h-3 text-sage" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2.5" d="M5 13l4 4L19 7" />
          </svg>
        </Show>
        <Show when={props.node.status === 'failed'}>
          <svg class="w-3 h-3 text-terra" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2.5" d="M6 18L18 6M6 6l12 12" />
          </svg>
        </Show>
        <Show when={isDeleted()}>
          <span class="text-[8px] px-1 py-px rounded bg-terra/20 text-terra font-medium">-</span>
        </Show>
      </div>
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
          const x1 = edge.from.x;
          const y1 = edge.from.y + NODE_HEIGHT;
          const x2 = edge.to.x;
          const y2 = edge.to.y;
          const midY = (y1 + y2) / 2;

          return (
            <path
              d={`M ${x1} ${y1} C ${x1} ${midY}, ${x2} ${midY}, ${x2} ${y2}`}
              fill="none"
              stroke={props.color}
              stroke-width="1"
              stroke-dasharray={props.dashed ? '3 2' : undefined}
              opacity="0.35"
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

  // Computed layouts
  const draftLayout = createMemo(() => layoutTree(delta.draftTree(), 0));
  const liveLayout = createMemo(() => layoutTree(delta.liveTree(), 0));

  // Check if we have a live tree (post-dispatch)
  const hasLiveTree = () => delta.liveTree().length > 0;

  // Total canvas dimensions
  const canvasDimensions = createMemo(() => {
    const draft = draftLayout();
    const live = liveLayout();

    if (!hasLiveTree()) {
      return {
        width: draft.width + TREE_PADDING * 2,
        height: Math.max(draft.height, NODE_HEIGHT) + TREE_PADDING * 2,
        draftOffset: TREE_PADDING,
        liveOffset: 0,
        dividerOffset: 0,
        showDivider: false,
      };
    }

    const liveWidth = Math.max(live.width, NODE_WIDTH);
    const totalWidth = liveWidth + DIVIDER_WIDTH + draft.width + TREE_PADDING * 2;
    const maxHeight = Math.max(draft.height, live.height, NODE_HEIGHT) + TREE_PADDING * 2;

    return {
      width: totalWidth,
      height: maxHeight,
      liveOffset: TREE_PADDING,
      dividerOffset: TREE_PADDING + liveWidth + DIVIDER_WIDTH / 2,
      draftOffset: TREE_PADDING + liveWidth + DIVIDER_WIDTH,
      showDivider: true,
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

        {/* Canvas Area */}
        <div class="flex-1 overflow-auto flex items-start justify-center p-4">
          <Show
            when={delta.draftTree().length > 0 || delta.liveTree().length > 0}
            fallback={<EmptyTreeState />}
          >
            {/* Canvas container */}
            <div
              class="relative"
              style={{
                width: `${canvasDimensions().width}px`,
                height: `${canvasDimensions().height}px`,
              }}
              onContextMenu={(e) => {
                if ((e.target as HTMLElement).classList.contains('relative')) {
                  handleContextMenu(e, null);
                }
              }}
            >
              {/* Live Tree Region (LEFT) */}
              <Show when={hasLiveTree()}>
                <div
                  class="absolute"
                  style={{
                    left: `${canvasDimensions().liveOffset}px`,
                    top: `${TREE_PADDING / 2}px`,
                    width: `${Math.max(liveLayout().width, NODE_WIDTH)}px`,
                    height: `${Math.max(liveLayout().height, canvasDimensions().height - TREE_PADDING)}px`,
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

              {/* Center Divider */}
              <Show when={canvasDimensions().showDivider}>
                <div
                  class="absolute"
                  style={{
                    left: `${canvasDimensions().dividerOffset}px`,
                    top: '0',
                    width: '1px',
                    height: `${canvasDimensions().height}px`,
                    'border-left': '1px dashed rgba(90, 85, 80, 0.3)',
                  }}
                />
              </Show>

              {/* Draft Tree Region */}
              <div
                class="absolute"
                style={{
                  left: `${canvasDimensions().draftOffset}px`,
                  top: `${TREE_PADDING / 2}px`,
                  width: `${draftLayout().width}px`,
                  height: `${draftLayout().height}px`,
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
