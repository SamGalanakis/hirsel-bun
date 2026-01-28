/**
 * DualBoard - Unified board with Draft and Live panels
 *
 * Shows draft tree (editable) and live tree (read-only status) side-by-side.
 * Highlights diff: green=new, amber=modified, red=deleted.
 */

import { invoke } from '@tauri-apps/api/core';
import {
  type Component,
  For,
  Show,
  batch,
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
  onMount,
} from 'solid-js';
import { useProject, useApp, useRuns } from '../../stores';
import { useDelta } from '../../stores/delta-context';
import type {
  DraftNodeTree,
  LiveNodeTree,
  TreeDiff,
  NodeType,
  LiveNodeStatus,
  CreateDraftNodeRequest,
  UpdateDraftNodeRequest,
} from '../../lib/types';
import { LIVE_NODE_STATUS_COLORS, LIVE_NODE_STATUS_ICONS } from '../../lib/types';

// =============================================================================
// Helper Components
// =============================================================================

/** Render a draft node (editable) */
const DraftNodeCard: Component<{
  node: DraftNodeTree;
  depth: number;
  diff: TreeDiff | null;
  showDelta: boolean;
  selected: boolean;
  onSelect: (id: string) => void;
  onDoubleClick: (node: DraftNodeTree) => void;
  onContextMenu: (e: MouseEvent, node: DraftNodeTree) => void;
}> = (props) => {
  // Check if this node is new (in diff.newNodes)
  const isNew = () =>
    props.showDelta && props.diff?.newNodes.some((n) => n.id === props.node.id);

  // Check if this node is modified
  const isModified = () =>
    props.showDelta && props.diff?.modifiedNodes.some((m) => m.draftNode.id === props.node.id);

  const borderColor = () => {
    if (isNew()) return 'border-emerald-500/50';
    if (isModified()) return 'border-amber-500/50';
    return 'border-zinc-700/50';
  };

  const bgColor = () => {
    if (isNew()) return 'bg-emerald-500/5';
    if (isModified()) return 'bg-amber-500/5';
    return 'bg-zinc-800/50';
  };

  const isEval = () => props.node.nodeType === 'eval';

  return (
    <div class="flex flex-col gap-1">
      <div
        class={`relative rounded-lg border px-3 py-2 cursor-pointer transition-all ${borderColor()} ${bgColor()} ${
          props.selected ? 'ring-2 ring-amber-500/50' : ''
        }`}
        style={{ 'margin-left': `${props.depth * 16}px` }}
        onClick={() => props.onSelect(props.node.id)}
        onDblClick={() => props.onDoubleClick(props.node)}
        onContextMenu={(e) => props.onContextMenu(e, props.node)}
      >
        {/* Delta indicator */}
        <Show when={props.showDelta && (isNew() || isModified())}>
          <div
            class={`absolute -left-2 top-1/2 -translate-y-1/2 w-1.5 h-1.5 rounded-full ${
              isNew() ? 'bg-emerald-400' : 'bg-amber-400'
            }`}
          />
        </Show>

        {/* Header */}
        <div class="flex items-center gap-2">
          <div
            class={`w-2 h-2 rounded-full ${isEval() ? 'bg-emerald-400' : 'bg-amber-400'}`}
          />
          <span class="text-sm font-medium text-wool-200 truncate">{props.node.name}</span>
          <Show when={isNew()}>
            <span class="text-[10px] px-1.5 py-0.5 rounded bg-emerald-500/20 text-emerald-400">
              new
            </span>
          </Show>
          <Show when={isModified()}>
            <span class="text-[10px] px-1.5 py-0.5 rounded bg-amber-500/20 text-amber-400">
              modified
            </span>
          </Show>
        </div>

        {/* Content preview */}
        <Show when={props.node.content}>
          <p class="text-xs text-wool-500 mt-1 line-clamp-2">{props.node.content}</p>
        </Show>

        {/* Validates (for evals) */}
        <Show when={isEval() && props.node.validates.length > 0}>
          <div class="mt-1 flex items-center gap-1">
            <span class="text-[10px] text-emerald-400/70">validates:</span>
            <span class="text-[10px] text-wool-500">{props.node.validates.join(', ')}</span>
          </div>
        </Show>
      </div>

      {/* Children */}
      <For each={props.node.children}>
        {(child) => (
          <DraftNodeCard
            node={child}
            depth={props.depth + 1}
            diff={props.diff}
            showDelta={props.showDelta}
            selected={props.selected}
            onSelect={props.onSelect}
            onDoubleClick={props.onDoubleClick}
            onContextMenu={props.onContextMenu}
          />
        )}
      </For>
    </div>
  );
};

/** Render a live node (read-only) */
const LiveNodeCard: Component<{
  node: LiveNodeTree;
  depth: number;
  diff: TreeDiff | null;
  showDelta: boolean;
}> = (props) => {
  // Check if this node is deleted from draft
  const isDeleted = () =>
    props.showDelta && props.diff?.deletedNodes.some((n) => n.id === props.node.id);

  const statusColor = () => {
    const colors: Record<LiveNodeStatus, string> = {
      pending: 'text-wool-500',
      working: 'text-amber-400',
      done: 'text-sage',
      failed: 'text-terra',
    };
    return colors[props.node.status] || 'text-wool-500';
  };

  const statusBg = () => {
    const bgs: Record<LiveNodeStatus, string> = {
      pending: 'bg-wool-500/10',
      working: 'bg-amber-500/10',
      done: 'bg-sage/10',
      failed: 'bg-terra/10',
    };
    return bgs[props.node.status] || 'bg-wool-500/10';
  };

  const isEval = () => props.node.nodeType === 'eval';

  return (
    <div class="flex flex-col gap-1">
      <div
        class={`relative rounded-lg border px-3 py-2 transition-all ${
          isDeleted()
            ? 'border-red-500/30 bg-red-500/5 opacity-60'
            : 'border-zinc-700/30 bg-zinc-900/50'
        }`}
        style={{ 'margin-left': `${props.depth * 16}px` }}
      >
        {/* Deleted indicator */}
        <Show when={props.showDelta && isDeleted()}>
          <div class="absolute -left-2 top-1/2 -translate-y-1/2 w-1.5 h-1.5 rounded-full bg-red-400" />
        </Show>

        {/* Header */}
        <div class="flex items-center gap-2">
          <div class={`px-1.5 py-0.5 rounded text-[10px] font-medium ${statusBg()} ${statusColor()}`}>
            {props.node.status}
          </div>
          <span class="text-sm font-medium text-wool-300 truncate">{props.node.name}</span>
          <Show when={isDeleted()}>
            <span class="text-[10px] px-1.5 py-0.5 rounded bg-red-500/20 text-red-400">
              deleted
            </span>
          </Show>
        </div>

        {/* Content preview */}
        <Show when={props.node.content}>
          <p class="text-xs text-wool-600 mt-1 line-clamp-2">{props.node.content}</p>
        </Show>

        {/* Commit SHA */}
        <Show when={props.node.lastCommitSha}>
          <div class="mt-1 flex items-center gap-1">
            <span class="text-[10px] text-wool-600">commit:</span>
            <span class="text-[10px] text-wool-500 font-mono">
              {props.node.lastCommitSha?.slice(0, 7)}
            </span>
          </div>
        </Show>
      </div>

      {/* Children */}
      <For each={props.node.children}>
        {(child) => (
          <LiveNodeCard
            node={child}
            depth={props.depth + 1}
            diff={props.diff}
            showDelta={props.showDelta}
          />
        )}
      </For>
    </div>
  );
};

// =============================================================================
// Main Component
// =============================================================================

export const DualBoard: Component = () => {
  const project = useProject();
  const app = useApp();
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
    node: DraftNodeTree;
  } | null>(null);

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

  const handleContextMenu = (e: MouseEvent, node: DraftNodeTree) => {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({ x: e.clientX, y: e.clientY, node });
  };

  const hideContextMenu = () => setContextMenu(null);

  const handleAddChild = () => {
    const cm = contextMenu();
    if (cm) {
      setNewNodeParentId(cm.node.id);
      setNewNodeType('task');
      setShowNewPrompt(true);
      hideContextMenu();
      setTimeout(() => newNodeInputRef?.focus(), 50);
    }
  };

  const handleAddSibling = () => {
    const cm = contextMenu();
    if (cm) {
      // Find parent by searching tree
      const findParent = (nodes: DraftNodeTree[], targetId: string): string | null => {
        for (const node of nodes) {
          if (node.children.some((c) => c.id === targetId)) {
            return node.id;
          }
          const found = findParent(node.children, targetId);
          if (found !== null) return found;
        }
        return null;
      };
      setNewNodeParentId(findParent(delta.draftTree(), cm.node.id));
      setNewNodeType('task');
      setShowNewPrompt(true);
      hideContextMenu();
      setTimeout(() => newNodeInputRef?.focus(), 50);
    }
  };

  const handleDelete = async () => {
    const cm = contextMenu();
    if (cm) {
      const confirmed = await window.confirmDialog?.delete('this node and all children', 'node');
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
    setTimeout(() => newNodeInputRef?.focus(), 50);
  };

  const handleAddRootEval = () => {
    setNewNodeParentId(null);
    setNewNodeType('eval');
    setShowNewPrompt(true);
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

    const result = await delta.dispatch();
    if (result) {
      // Could navigate to run view here
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

  // ==========================================================================
  // Render
  // ==========================================================================

  return (
    <div
      class="flex-1 flex flex-col overflow-hidden"
      style={{ background: 'linear-gradient(145deg, #0f0f0f 0%, #1a1815 50%, #0f0f0f 100%)' }}
    >
      {/* Header */}
      <div class="flex items-center justify-between px-4 py-3 border-b border-zinc-800">
        <div class="flex items-center gap-4">
          <h2 class="text-sm font-semibold text-wool-200">
            {project.selectedProject()?.name || 'No Project'}
          </h2>
          <Show when={delta.projectRun()}>
            <div class="flex items-center gap-2">
              <span class="text-xs text-wool-500">Run:</span>
              <span class="text-xs text-amber-400 font-mono">{delta.projectRun()?.runName}</span>
              <span
                class={`text-[10px] px-1.5 py-0.5 rounded ${
                  delta.projectRun()?.status === 'working'
                    ? 'bg-amber-500/20 text-amber-400'
                    : delta.projectRun()?.status === 'paused'
                      ? 'bg-wool-500/20 text-wool-400'
                      : 'bg-red-500/20 text-red-400'
                }`}
              >
                {delta.projectRun()?.status}
              </span>
            </div>
          </Show>
        </div>

        <div class="flex items-center gap-2">
          {/* Delta toggle */}
          <button
            onClick={() => delta.toggleDeltaIndicators()}
            class={`p-2 rounded-lg transition-colors ${
              delta.showDeltaIndicators()
                ? 'bg-amber-500/20 text-amber-400'
                : 'bg-zinc-800 text-wool-500 hover:text-wool-300'
            }`}
            title="Toggle delta indicators"
          >
            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2"
              />
            </svg>
          </button>

          {/* Diff summary */}
          <Show when={delta.hasDiff()}>
            <div class="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-zinc-800/50 border border-zinc-700/50">
              <Show when={(delta.diff()?.newNodes.length || 0) > 0}>
                <span class="text-xs text-emerald-400">
                  +{delta.diff()?.newNodes.length}
                </span>
              </Show>
              <Show when={(delta.diff()?.modifiedNodes.length || 0) > 0}>
                <span class="text-xs text-amber-400">
                  ~{delta.diff()?.modifiedNodes.length}
                </span>
              </Show>
              <Show when={(delta.diff()?.deletedNodes.length || 0) > 0}>
                <span class="text-xs text-red-400">
                  -{delta.diff()?.deletedNodes.length}
                </span>
              </Show>
            </div>
          </Show>

          {/* Dispatch button */}
          <button
            onClick={handleDispatch}
            disabled={!delta.hasDiff() || delta.dispatchPending()}
            class="btn-warning px-4 py-1.5 rounded-lg text-sm font-medium disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {delta.dispatchPending() ? 'Dispatching...' : 'Dispatch'}
          </button>
        </div>
      </div>

      {/* Panels */}
      <div class="flex-1 flex overflow-hidden">
        {/* Draft Panel */}
        <div class="flex-1 flex flex-col border-r border-zinc-800">
          <div class="flex items-center justify-between px-4 py-2 border-b border-zinc-800/50">
            <span class="text-xs font-medium text-wool-400">Draft</span>
            <div class="flex items-center gap-1">
              <button
                onClick={handleAddRootTask}
                class="p-1 rounded text-wool-500 hover:text-wool-200 hover:bg-zinc-700/50 transition-colors"
                title="Add root task"
              >
                <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
                </svg>
              </button>
              <button
                onClick={handleAddRootEval}
                class="p-1 rounded text-emerald-500 hover:text-emerald-300 hover:bg-emerald-700/20 transition-colors"
                title="Add eval"
              >
                <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    stroke-width="2"
                    d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z"
                  />
                </svg>
              </button>
            </div>
          </div>
          <div class="flex-1 overflow-auto p-4">
            <Show
              when={delta.draftTree().length > 0}
              fallback={
                <div class="flex flex-col items-center justify-center h-full text-center">
                  <div class="w-12 h-12 mb-3 rounded-xl bg-amber-500/10 border border-amber-500/20 flex items-center justify-center">
                    <svg class="w-6 h-6 text-amber-500/50" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
                    </svg>
                  </div>
                  <p class="text-sm text-wool-500 mb-2">No draft nodes</p>
                  <p class="text-xs text-wool-600">Click + to add a task</p>
                </div>
              }
            >
              <div class="flex flex-col gap-2">
                <For each={delta.draftTree()}>
                  {(node) => (
                    <DraftNodeCard
                      node={node}
                      depth={0}
                      diff={delta.diff()}
                      showDelta={delta.showDeltaIndicators()}
                      selected={selectedNodeId() === node.id}
                      onSelect={setSelectedNodeId}
                      onDoubleClick={handleDoubleClick}
                      onContextMenu={handleContextMenu}
                    />
                  )}
                </For>
              </div>
            </Show>
          </div>
        </div>

        {/* Live Panel */}
        <div class="flex-1 flex flex-col">
          <div class="flex items-center justify-between px-4 py-2 border-b border-zinc-800/50">
            <span class="text-xs font-medium text-wool-400">Live</span>
            <Show when={delta.projectRun()}>
              <span class="text-[10px] text-wool-600">
                Last dispatch: {delta.projectRun()?.lastDispatchAt?.slice(0, 19) || 'never'}
              </span>
            </Show>
          </div>
          <div class="flex-1 overflow-auto p-4">
            <Show
              when={delta.liveTree().length > 0}
              fallback={
                <div class="flex flex-col items-center justify-center h-full text-center">
                  <div class="w-12 h-12 mb-3 rounded-xl bg-zinc-800 border border-zinc-700/50 flex items-center justify-center">
                    <svg class="w-6 h-6 text-wool-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 10V3L4 14h7v7l9-11h-7z" />
                    </svg>
                  </div>
                  <p class="text-sm text-wool-500 mb-2">No dispatched nodes</p>
                  <p class="text-xs text-wool-600">Dispatch changes to see them here</p>
                </div>
              }
            >
              <div class="flex flex-col gap-2">
                <For each={delta.liveTree()}>
                  {(node) => (
                    <LiveNodeCard
                      node={node}
                      depth={0}
                      diff={delta.diff()}
                      showDelta={delta.showDeltaIndicators()}
                    />
                  )}
                </For>
              </div>
            </Show>
          </div>
        </div>
      </div>

      {/* Context Menu */}
      <Show when={contextMenu()}>
        <div
          class="fixed inset-0"
          style={{ 'z-index': 99 }}
          onClick={hideContextMenu}
          onContextMenu={(e) => {
            e.preventDefault();
            hideContextMenu();
          }}
        />
        <div
          class="fixed z-[100] bg-zinc-900 border border-zinc-700 rounded-lg shadow-xl py-1 min-w-[160px]"
          style={{ left: `${contextMenu()!.x}px`, top: `${contextMenu()!.y}px` }}
        >
          <button
            class="w-full px-3 py-1.5 text-left text-sm text-wool-300 hover:bg-zinc-800 transition-colors"
            onClick={handleAddChild}
          >
            Add child
          </button>
          <button
            class="w-full px-3 py-1.5 text-left text-sm text-wool-300 hover:bg-zinc-800 transition-colors"
            onClick={handleAddSibling}
          >
            Add sibling
          </button>
          <button
            class="w-full px-3 py-1.5 text-left text-sm text-wool-300 hover:bg-zinc-800 transition-colors"
            onClick={() => {
              handleDoubleClick(contextMenu()!.node);
              hideContextMenu();
            }}
          >
            Edit
          </button>
          <div class="h-px bg-zinc-700 my-1" />
          <button
            class="w-full px-3 py-1.5 text-left text-sm text-red-400 hover:bg-red-500/10 transition-colors"
            onClick={handleDelete}
          >
            Delete
          </button>
        </div>
      </Show>

      {/* New Node Prompt */}
      <Show when={showNewPrompt()}>
        <dialog
          open
          class="fixed inset-0 z-[100] m-0 h-full w-full max-w-none max-h-none bg-black/50 flex items-center justify-center"
          onClick={() => setShowNewPrompt(false)}
        >
          <div class="card w-80" onClick={(e) => e.stopPropagation()}>
            <header>
              <h3 class="text-sm font-semibold text-wool-200">
                New {newNodeType() === 'eval' ? 'Eval' : 'Task'}
              </h3>
            </header>
            <section>
              <form class="form" onSubmit={(e) => { e.preventDefault(); handleCreateNode(); }}>
                <input
                  ref={newNodeInputRef}
                  type="text"
                  value={newNodeName()}
                  onInput={(e) => setNewNodeName(e.currentTarget.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Escape') setShowNewPrompt(false);
                  }}
                  placeholder={`${newNodeType() === 'eval' ? 'Eval' : 'Task'} name...`}
                />
              </form>
            </section>
            <footer>
              <button onClick={() => setShowNewPrompt(false)} class="btn-ghost">
                Cancel
              </button>
              <button
                onClick={handleCreateNode}
                disabled={!newNodeName().trim()}
                class={newNodeType() === 'eval' ? 'btn-success' : 'btn'}
              >
                Create
              </button>
            </footer>
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
          <div class="card w-full max-w-md" onClick={(e) => e.stopPropagation()}>
            <header>
              <h3 class="text-sm font-semibold text-wool-200">
                Edit {editingNode()!.nodeType === 'eval' ? 'Eval' : 'Task'}
              </h3>
            </header>
            <section>
              <form class="form grid gap-4">
                <div class="grid gap-2">
                  <label for="edit-name" class="text-xs font-medium text-wool-400">
                    Name
                  </label>
                  <input
                    id="edit-name"
                    type="text"
                    value={editForm().name}
                    onInput={(e) => setEditForm((f) => ({ ...f, name: e.currentTarget.value }))}
                  />
                </div>
                <div class="grid gap-2">
                  <label for="edit-content" class="text-xs font-medium text-wool-400">
                    Content
                  </label>
                  <textarea
                    id="edit-content"
                    value={editForm().content}
                    onInput={(e) => setEditForm((f) => ({ ...f, content: e.currentTarget.value }))}
                    rows={4}
                    placeholder="Description..."
                  />
                </div>
                <Show when={editingNode()!.nodeType === 'eval'}>
                  <div class="grid gap-2">
                    <label for="edit-validates" class="text-xs font-medium text-emerald-400/70">
                      Validates (task IDs)
                    </label>
                    <input
                      id="edit-validates"
                      type="text"
                      value={editForm().validates}
                      onInput={(e) => setEditForm((f) => ({ ...f, validates: e.currentTarget.value }))}
                      placeholder="task-1, task-2"
                    />
                  </div>
                </Show>
              </form>
            </section>
            <footer>
              <button onClick={() => setEditingNode(null)} class="btn-ghost">
                Cancel
              </button>
              <button onClick={handleSaveEdit} class="btn">
                Save
              </button>
            </footer>
          </div>
        </dialog>
      </Show>

      {/* Loading overlay */}
      <Show when={delta.loading()}>
        <div class="absolute inset-0 bg-black/30 flex items-center justify-center">
          <div class="animate-spin w-8 h-8 border-2 border-amber-500 border-t-transparent rounded-full" />
        </div>
      </Show>
    </div>
  );
};

export default DualBoard;
