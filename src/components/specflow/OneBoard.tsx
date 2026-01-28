/**
 * OneBoard - Task/Eval canvas for the selected project
 *
 * Shows tasks and evals on an infinite canvas with pan/zoom.
 * Project selection is handled via the breadcrumbs ProjectSelector.
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
import { TaskCard, EvalCard } from './NodeRenderer';
import { NodeContextMenu } from './NodeContextMenu';
import { DispatchModal } from './DispatchModal';
import { RunsOverlay } from './RunsOverlay';
import {
  computeTreeLayout,
  generateEdgePath,
  getLOADLevel,
  LAYOUT_CONFIG,
  RENDER_CONFIGS,
  type LOADLevel,
  type NodePosition,
} from './use-tree-layout';
import { flattenTree, findNodeById, getDescendantIds } from '../../lib/utils/tree';
import type {
  TaskTree,
  BoardEval,
  Bookmark,
  BoardSyncResult,
  BoardTaskStatus,
  BoardEvalStatus,
  TaskRun,
  RunSummary,
} from '../../lib/types';

interface Transform {
  x: number;
  y: number;
  k: number;
}

// Zoom limits
const MIN_ZOOM = 0.3;
const MAX_ZOOM = 3.0;
const DEFAULT_ZOOM = 1.0;

export const OneBoard: Component = () => {
  const project = useProject();
  const app = useApp();
  const runs = useRuns();

  let viewportRef: HTMLDivElement | undefined;
  let canvasRef: HTMLCanvasElement | undefined;

  // Task runs for this project (maps task ID -> runs dispatched from it)
  const [taskRuns, setTaskRuns] = createSignal<Map<string, TaskRun[]>>(new Map());

  // Transform state
  const [transform, setTransform] = createSignal<Transform>({ x: 0, y: 0, k: DEFAULT_ZOOM });

  // Interaction states
  const [panning, setPanning] = createSignal(false);
  const [panStart, setPanStart] = createSignal({ x: 0, y: 0 });

  // Task/Eval dragging state
  const [taskDragging, setTaskDragging] = createSignal<{
    nodeId: string;
    isEval: boolean;
    startX: number;
    startY: number;
    initialPositions: Map<string, { x: number; y: number }>;
  } | null>(null);
  const [taskDragPositions, setTaskDragPositions] = createSignal<Map<string, NodePosition>>(
    new Map()
  );

  // Task/Eval data
  const [taskTree, setTaskTree] = createSignal<TaskTree[]>([]);
  const [evals, setEvals] = createSignal<BoardEval[]>([]);
  const [bookmarks, setBookmarks] = createSignal<Bookmark[]>([]);
  const [loading, setLoading] = createSignal(false);

  // Task/Eval selection
  const [selectedTaskId, setSelectedTaskId] = createSignal<string | null>(null);
  const [selectedEvalId, setSelectedEvalId] = createSignal<string | null>(null);

  // Dispatch scope (shift+click selection for runs)
  // Contains task IDs that are directly selected (their subtrees + evals are computed)
  const [dispatchRoots, setDispatchRoots] = createSignal<Set<string>>(new Set());

  // Dispatch modal
  const [showDispatchModal, setShowDispatchModal] = createSignal(false);

  // Context menu
  const [contextMenu, setContextMenu] = createSignal<{
    x: number;
    y: number;
    worldX: number;
    worldY: number;
    taskId: string | null;
    evalId: string | null;
  } | null>(null);

  // New item prompt
  const [newItemPrompt, setNewItemPrompt] = createSignal<{
    x: number;
    y: number;
    parentId: string | null;
    type: 'task' | 'eval';
  } | null>(null);
  const [newItemName, setNewItemName] = createSignal('');
  let newItemInputRef: HTMLInputElement | undefined;

  // Edit modal
  const [editingTask, setEditingTask] = createSignal<TaskTree | null>(null);
  const [editingEval, setEditingEval] = createSignal<BoardEval | null>(null);
  const [editForm, setEditForm] = createSignal({
    name: '',
    content: '',
    validates: [] as string[],
  });

  // Gyp editing indicators
  const [gypEditingIslands, setGypEditingIslands] = createSignal<Set<string>>(new Set());

  // Computed: LOAD level for tasks
  const taskLoad = createMemo<LOADLevel>(() => getLOADLevel(transform().k));

  // Computed: task layout
  const taskLayout = createMemo(() => computeTreeLayout(taskTree(), evals()));

  // Computed: task positions (centered in canvas)
  const taskPositions = createMemo(() => {
    const layout = taskLayout();
    const dragPos = taskDragPositions();

    const positions = new Map<string, NodePosition>();
    for (const [id, pos] of layout.positions) {
      const dragOverride = dragPos.get(id);
      if (dragOverride) {
        positions.set(id, dragOverride);
      } else {
        positions.set(id, pos);
      }
    }
    return positions;
  });

  // Get local position for drag calculations
  const getTaskLocalPosition = (id: string): NodePosition | undefined => {
    const dragPos = taskDragPositions().get(id);
    if (dragPos) return dragPos;
    return taskLayout().positions.get(id);
  };

  // Computed: full dispatch scope (all tasks including descendants)
  const dispatchTaskIds = createMemo(() => {
    const roots = dispatchRoots();
    if (roots.size === 0) return new Set<string>();

    const allIds = new Set<string>();
    for (const rootId of roots) {
      const descendants = getDescendantIds(taskTree(), rootId);
      for (const id of descendants) {
        allIds.add(id);
      }
    }
    return allIds;
  });

  // Computed: evals that validate tasks in dispatch scope
  const dispatchEvalIds = createMemo(() => {
    const taskIds = dispatchTaskIds();
    if (taskIds.size === 0) return new Set<string>();

    const evalIds = new Set<string>();
    for (const ev of evals()) {
      // Include eval if any of its validated tasks are in scope
      const validatesInScope = ev.validates.some(tid => taskIds.has(tid));
      if (validatesInScope) {
        evalIds.add(ev.id);
      }
    }
    return evalIds;
  });

  // Toggle a task in dispatch scope (shift+click)
  const toggleDispatchScope = (taskId: string) => {
    setDispatchRoots(prev => {
      const next = new Set(prev);
      if (next.has(taskId)) {
        next.delete(taskId);
      } else {
        // Remove any descendants that are already roots (they'll be covered by this parent)
        const descendants = getDescendantIds(taskTree(), taskId);
        for (const descId of descendants) {
          if (descId !== taskId) next.delete(descId);
        }
        next.add(taskId);
      }
      return next;
    });
  };

  // Clear dispatch scope
  const clearDispatchScope = () => {
    setDispatchRoots(new Set<string>());
  };

  // ==========================================================================
  // Data Loading
  // ==========================================================================

  const loadTaskData = async (projectId: number) => {
    try {
      setLoading(true);
      const [treeData, evalData, bookmarkData] = await Promise.all([
        invoke<TaskTree[]>('get_board_task_tree', { projectId }),
        invoke<BoardEval[]>('get_board_evals', { projectId }),
        invoke<Bookmark[]>('get_bookmarks', { projectId }),
      ]);
      batch(() => {
        setTaskTree(treeData || []);
        setEvals(evalData || []);
        setBookmarks(bookmarkData || []);
        setLoading(false);
      });
    } catch (e) {
      console.error('Failed to load task data:', e);
      setLoading(false);
    }
  };

  // Load data when selected project changes
  createEffect(() => {
    const selectedId = project.selectedProjectId();

    if (selectedId) {
      loadTaskData(selectedId);
      // Also set focused for consistency
      project.setFocusedProjectId(selectedId);
    } else {
      batch(() => {
        setTaskTree([]);
        setEvals([]);
        setBookmarks([]);
        setSelectedTaskId(null);
        setSelectedEvalId(null);
      });
      project.setFocusedProjectId(null);
    }
  });

  // Poll for task changes
  createEffect(() => {
    const selectedId = project.selectedProjectId();
    if (!selectedId) return;

    const interval = setInterval(async () => {
      try {
        const result = await invoke<BoardSyncResult>('poll_board_changes', {
          projectId: selectedId,
        });
        if (result.changes > 0) {
          await loadTaskData(selectedId);
        }
      } catch (e) {
        console.error('Failed to poll task changes:', e);
      }
    }, 2500);

    onCleanup(() => clearInterval(interval));
  });

  // Listen for gyp-editing-islands events
  createEffect(() => {
    const handler = (e: Event) => {
      const customEvent = e as CustomEvent<Set<string>>;
      setGypEditingIslands(customEvent.detail);
    };
    window.addEventListener('gyp-editing-islands', handler);
    onCleanup(() => window.removeEventListener('gyp-editing-islands', handler));
  });

  // Listen for board-refresh events
  createEffect(() => {
    const handler = async (e: Event) => {
      const customEvent = e as CustomEvent<number>;
      const pid = customEvent.detail;
      if (pid && pid === project.selectedProjectId()) {
        try {
          await invoke('import_board_from_agent', { projectId: pid });
        } catch (e) {
          console.error('Failed to import board changes:', e);
        }
        loadTaskData(pid);
      }
    };
    window.addEventListener('board-refresh', handler);
    onCleanup(() => window.removeEventListener('board-refresh', handler));
  });

  // Load task runs for project
  const loadTaskRuns = async (projectId: number) => {
    try {
      const allRuns = await invoke<TaskRun[]>('get_all_task_runs', { projectId });
      // Group by taskId
      const runsByTask = new Map<string, TaskRun[]>();
      for (const run of allRuns) {
        const existing = runsByTask.get(run.taskId) || [];
        existing.push(run);
        runsByTask.set(run.taskId, existing);
      }
      setTaskRuns(runsByTask);
    } catch (e) {
      console.error('Failed to load task runs:', e);
    }
  };

  // Load task runs when project changes
  createEffect(() => {
    const selectedId = project.selectedProjectId();
    if (selectedId) {
      loadTaskRuns(selectedId);
    } else {
      setTaskRuns(new Map());
    }
  });

  // Subscribe to runs polling
  createEffect(() => {
    const unsubscribe = runs.subscribe();
    onCleanup(unsubscribe);
  });

  // Helper: Get active run for a task (most recent active run)
  const getActiveRunForTask = (taskId: string): { name: string; status: string } | null => {
    const taskRunList = taskRuns().get(taskId);
    if (!taskRunList || taskRunList.length === 0) return null;

    // Get the most recent run name
    const latestTaskRun = taskRunList[taskRunList.length - 1];

    // Find the run in the runs list and check if it's active
    const run = runs.runs().find((r) => r.name === latestTaskRun.runName);
    if (!run) return null;

    // Only return if it's an "active" status
    const activeStatuses = ['working', 'eval', 'paused', 'idle', 'waiting'];
    if (!activeStatuses.includes(run.status)) return null;

    return { name: run.name, status: run.status };
  };

  // ==========================================================================
  // Canvas Rendering
  // ==========================================================================

  const drawTaskEdges = (
    ctx: CanvasRenderingContext2D,
    positions: Map<string, NodePosition>,
    tree: TaskTree[],
    load: LOADLevel
  ) => {
    const nodeHeight = RENDER_CONFIGS[load].nodeHeight;

    const drawNodeEdges = (node: TaskTree) => {
      const parentPos = positions.get(node.id);
      if (!parentPos) return;

      for (const child of node.children) {
        const childPos = positions.get(child.id);
        if (!childPos) continue;

        const path = generateEdgePath(parentPos, childPos, nodeHeight);

        // Edge glow
        ctx.strokeStyle = 'rgba(212, 165, 116, 0.15)';
        ctx.lineWidth = 6;
        ctx.beginPath();
        const path2d = new Path2D(path);
        ctx.stroke(path2d);

        // Edge core
        ctx.strokeStyle = 'rgba(212, 165, 116, 0.5)';
        ctx.lineWidth = 2;
        ctx.stroke(path2d);

        drawNodeEdges(child);
      }
    };

    tree.forEach(drawNodeEdges);
  };

  const drawEvalConnections = (
    ctx: CanvasRenderingContext2D,
    positions: Map<string, NodePosition>,
    evalList: BoardEval[],
    load: LOADLevel
  ) => {
    const nodeHeight = RENDER_CONFIGS[load].nodeHeight;

    for (const ev of evalList) {
      const evalPos = positions.get(ev.id);
      if (!evalPos) continue;

      for (const taskId of ev.validates) {
        const taskPos = positions.get(taskId);
        if (!taskPos) continue;

        const path = generateEdgePath(taskPos, evalPos, nodeHeight);

        // Connection glow
        ctx.strokeStyle = 'rgba(16, 185, 129, 0.1)';
        ctx.lineWidth = 4;
        ctx.setLineDash([]);
        ctx.beginPath();
        const path2d = new Path2D(path);
        ctx.stroke(path2d);

        // Connection line (dashed)
        ctx.strokeStyle = 'rgba(16, 185, 129, 0.4)';
        ctx.lineWidth = 1.5;
        ctx.setLineDash([6, 4]);
        ctx.stroke(path2d);
      }
    }

    ctx.setLineDash([]);
  };

  const render = () => {
    if (!canvasRef || !viewportRef) return;

    const ctx = canvasRef.getContext('2d');
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const { x, y, k } = transform();
    const vw = viewportRef.clientWidth;
    const vh = viewportRef.clientHeight;

    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, vw * dpr, vh * dpr);
    ctx.setTransform(dpr * k, 0, 0, dpr * k, dpr * x, dpr * y);

    // Draw grid
    const majorSpacing = 100;
    const minorSpacing = 20;
    const showMinor = k >= 0.5;

    const worldLeft = Math.floor(-x / k / majorSpacing) * majorSpacing - majorSpacing;
    const worldTop = Math.floor(-y / k / majorSpacing) * majorSpacing - majorSpacing;
    const worldRight = Math.ceil((-x + vw) / k / majorSpacing) * majorSpacing + majorSpacing;
    const worldBottom = Math.ceil((-y + vh) / k / majorSpacing) * majorSpacing + majorSpacing;

    // Minor grid dots
    if (showMinor) {
      ctx.fillStyle = 'rgba(255, 255, 255, 0.03)';
      for (let wx = worldLeft; wx <= worldRight; wx += minorSpacing) {
        for (let wy = worldTop; wy <= worldBottom; wy += minorSpacing) {
          ctx.beginPath();
          ctx.arc(wx, wy, 1 / k, 0, Math.PI * 2);
          ctx.fill();
        }
      }
    }

    // Major grid dots
    ctx.fillStyle = 'rgba(255, 255, 255, 0.08)';
    for (let wx = worldLeft; wx <= worldRight; wx += majorSpacing) {
      for (let wy = worldTop; wy <= worldBottom; wy += majorSpacing) {
        ctx.beginPath();
        ctx.arc(wx, wy, 2 / k, 0, Math.PI * 2);
        ctx.fill();
      }
    }

    // Draw task edges
    const positions = taskPositions();
    const load = taskLoad();
    drawTaskEdges(ctx, positions, taskTree(), load);
    drawEvalConnections(ctx, positions, evals(), load);
  };

  const handleResize = () => {
    if (!canvasRef || !viewportRef) return;
    const dpr = window.devicePixelRatio || 1;
    canvasRef.width = viewportRef.clientWidth * dpr;
    canvasRef.height = viewportRef.clientHeight * dpr;
    canvasRef.style.width = `${viewportRef.clientWidth}px`;
    canvasRef.style.height = `${viewportRef.clientHeight}px`;
    render();
  };

  onMount(() => {
    if (canvasRef) {
      handleResize();
      const resizeObserver = new ResizeObserver(handleResize);
      if (viewportRef) resizeObserver.observe(viewportRef);
      onCleanup(() => resizeObserver.disconnect());
    }
    // Center transform
    if (viewportRef) {
      const vw = viewportRef.clientWidth;
      const vh = viewportRef.clientHeight;
      setTransform({
        x: vw / 2,
        y: vh / 2,
        k: DEFAULT_ZOOM,
      });
    }
  });

  // Re-render when transform or data changes
  createEffect(() => {
    transform();
    taskTree();
    evals();
    taskDragPositions();
    render();
  });

  // ==========================================================================
  // Pan & Zoom
  // ==========================================================================

  const handleWheel = (e: WheelEvent) => {
    e.preventDefault();
    if (!viewportRef) return;

    const rect = viewportRef.getBoundingClientRect();
    const t = transform();
    const mouseX = e.clientX - rect.left;
    const mouseY = e.clientY - rect.top;

    const delta = e.deltaY > 0 ? 0.9 : 1.1;
    const newK = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, t.k * delta));

    const newX = mouseX - (mouseX - t.x) * (newK / t.k);
    const newY = mouseY - (mouseY - t.y) * (newK / t.k);

    setTransform({ x: newX, y: newY, k: newK });
  };

  const handleMouseDown = (e: MouseEvent) => {
    if (e.button === 1 || (e.button === 0 && e.altKey)) {
      e.preventDefault();
      setPanning(true);
      setPanStart({ x: e.clientX, y: e.clientY });
    }
  };

  const handleMouseMove = (e: MouseEvent) => {
    // Handle task/eval dragging
    const taskDrag = taskDragging();
    if (taskDrag) {
      const t = transform();
      const dx = (e.clientX - taskDrag.startX) / t.k;
      const dy = (e.clientY - taskDrag.startY) / t.k;

      const newPositions = new Map<string, NodePosition>();
      for (const [id, initial] of taskDrag.initialPositions) {
        newPositions.set(id, { x: initial.x + dx, y: initial.y + dy });
      }
      setTaskDragPositions(newPositions);
      return;
    }

    if (panning()) {
      const dx = e.clientX - panStart().x;
      const dy = e.clientY - panStart().y;
      setTransform((t) => ({ ...t, x: t.x + dx, y: t.y + dy }));
      setPanStart({ x: e.clientX, y: e.clientY });
    }
  };

  const handleMouseUp = async () => {
    // Handle task/eval drag end
    const taskDrag = taskDragging();
    if (taskDrag) {
      const projectId = project.selectedProjectId();
      if (projectId) {
        const positions = taskDragPositions();
        const updates = Array.from(positions.entries()).map(([id, pos]) => {
          const isEval = evals().some((e) => e.id === id);
          if (isEval) {
            return invoke('update_board_eval', { projectId, evalId: id, x: pos.x, y: pos.y });
          }
          return invoke('update_board_task', { projectId, taskId: id, x: pos.x, y: pos.y });
        });

        try {
          await Promise.all(updates);
          await loadTaskData(projectId);
        } catch (e) {
          console.error('Failed to save positions:', e);
          window.toast?.error('Failed to save positions');
        }
      }

      setTaskDragging(null);
      setTaskDragPositions(new Map());
      return;
    }

    setPanning(false);
  };

  // ==========================================================================
  // Task/Eval Interactions
  // ==========================================================================

  const collectDescendantIds = (nodeId: string): string[] => {
    const taskIds: string[] = [nodeId];

    const collectChildren = (node: TaskTree) => {
      for (const child of node.children) {
        taskIds.push(child.id);
        collectChildren(child);
      }
    };
    const node = findNodeById(taskTree(), nodeId);
    if (node) collectChildren(node);

    const evalIds: string[] = [];
    for (const ev of evals()) {
      if (ev.validates.some((taskId) => taskIds.includes(taskId))) {
        evalIds.push(ev.id);
      }
    }

    return [...taskIds, ...evalIds];
  };

  const handleTaskDragStart = (e: MouseEvent, taskId: string) => {
    e.preventDefault();
    const idsToMove = collectDescendantIds(taskId);
    const initialPositions = new Map<string, { x: number; y: number }>();
    for (const id of idsToMove) {
      const pos = getTaskLocalPosition(id);
      if (pos) initialPositions.set(id, { x: pos.x, y: pos.y });
    }

    setTaskDragging({
      nodeId: taskId,
      isEval: false,
      startX: e.clientX,
      startY: e.clientY,
      initialPositions,
    });
    setSelectedTaskId(taskId);
    setSelectedEvalId(null);
  };

  const handleEvalDragStart = (e: MouseEvent, evalId: string) => {
    e.preventDefault();
    const pos = getTaskLocalPosition(evalId);
    const initialPositions = new Map<string, { x: number; y: number }>();
    if (pos) initialPositions.set(evalId, { x: pos.x, y: pos.y });

    setTaskDragging({
      nodeId: evalId,
      isEval: true,
      startX: e.clientX,
      startY: e.clientY,
      initialPositions,
    });
    setSelectedEvalId(evalId);
    setSelectedTaskId(null);
  };

  // ==========================================================================
  // Context Menu
  // ==========================================================================

  const handleContextMenu = (
    e: MouseEvent,
    taskId: string | null = null,
    evalId: string | null = null
  ) => {
    e.preventDefault();
    e.stopPropagation();
    if (!viewportRef) return;

    const rect = viewportRef.getBoundingClientRect();
    const t = transform();
    const worldX = (e.clientX - rect.left - t.x) / t.k;
    const worldY = (e.clientY - rect.top - t.y) / t.k;

    setContextMenu({ x: e.clientX, y: e.clientY, worldX, worldY, taskId, evalId });
  };

  const hideContextMenu = () => setContextMenu(null);

  // ==========================================================================
  // Task CRUD
  // ==========================================================================

  const createTask = async (parentId: string | null, name: string) => {
    const projectId = project.selectedProjectId();
    if (!projectId || !name.trim()) return;

    try {
      const task = await invoke<TaskTree>('create_board_task', {
        projectId,
        parentId,
        name: name.trim(),
      });
      await loadTaskData(projectId);
      setSelectedTaskId(task.id);
      setSelectedEvalId(null);
    } catch (e) {
      console.error('Failed to create task:', e);
      window.toast?.error('Failed to create task');
    }
  };

  const updateTask = async (
    taskId: string,
    updates: { name?: string; status?: BoardTaskStatus; content?: string; x?: number; y?: number }
  ) => {
    const projectId = project.selectedProjectId();
    if (!projectId) return;

    try {
      await invoke('update_board_task', { projectId, taskId, ...updates });
      await loadTaskData(projectId);
    } catch (e) {
      console.error('Failed to update task:', e);
      window.toast?.error('Failed to update task');
    }
  };

  const deleteTask = async (taskId: string) => {
    const projectId = project.selectedProjectId();
    if (!projectId) return;

    const confirmed = await window.confirmDialog?.delete('this task and all children', 'task');
    if (!confirmed) return;

    try {
      await invoke('delete_board_task', { projectId, taskId });
      await loadTaskData(projectId);
      if (selectedTaskId() === taskId) setSelectedTaskId(null);
      window.toast?.success('Task deleted');
    } catch (e) {
      console.error('Failed to delete task:', e);
      window.toast?.error(`Failed to delete task: ${e}`);
    }
  };

  // ==========================================================================
  // Eval CRUD
  // ==========================================================================

  const createEval = async (name: string) => {
    const projectId = project.selectedProjectId();
    if (!projectId || !name.trim()) return;

    try {
      const ev = await invoke<BoardEval>('create_board_eval', {
        projectId,
        name: name.trim(),
      });
      await loadTaskData(projectId);
      setSelectedEvalId(ev.id);
      setSelectedTaskId(null);
    } catch (e) {
      console.error('Failed to create eval:', e);
      window.toast?.error('Failed to create eval');
    }
  };

  const updateEval = async (
    evalId: string,
    updates: { name?: string; status?: BoardEvalStatus; content?: string; validates?: string[]; x?: number; y?: number }
  ) => {
    const projectId = project.selectedProjectId();
    if (!projectId) return;

    try {
      await invoke('update_board_eval', { projectId, evalId, ...updates });
      await loadTaskData(projectId);
    } catch (e) {
      console.error('Failed to update eval:', e);
      window.toast?.error('Failed to update eval');
    }
  };

  const deleteEval = async (evalId: string) => {
    const projectId = project.selectedProjectId();
    if (!projectId) return;

    const confirmed = await window.confirmDialog?.delete('this eval', 'eval');
    if (!confirmed) return;

    try {
      await invoke('delete_board_eval', { projectId, evalId });
      await loadTaskData(projectId);
      if (selectedEvalId() === evalId) setSelectedEvalId(null);
      window.toast?.success('Eval deleted');
    } catch (e) {
      console.error('Failed to delete eval:', e);
      window.toast?.error(`Failed to delete eval: ${e}`);
    }
  };

  // ==========================================================================
  // Edit Modal
  // ==========================================================================

  const openTaskEdit = (task: TaskTree) => {
    setEditForm({ name: task.name, content: task.content, validates: [] });
    setEditingTask(task);
    setEditingEval(null);
  };

  const openEvalEdit = (ev: BoardEval) => {
    setEditForm({ name: ev.name, content: ev.content, validates: [...ev.validates] });
    setEditingTask(null);
    setEditingEval(ev);
  };

  const saveEdit = async () => {
    const task = editingTask();
    const ev = editingEval();

    if (task) {
      await updateTask(task.id, { name: editForm().name, content: editForm().content });
      setEditingTask(null);
    } else if (ev) {
      await updateEval(ev.id, {
        name: editForm().name,
        content: editForm().content,
        validates: editForm().validates,
      });
      setEditingEval(null);
    }
  };

  // New item prompt
  const showNewItemPrompt = (
    x: number,
    y: number,
    parentId: string | null,
    type: 'task' | 'eval'
  ) => {
    setNewItemPrompt({ x, y, parentId, type });
    setNewItemName('');
    setTimeout(() => newItemInputRef?.focus(), 50);
  };

  const submitNewItem = () => {
    const prompt = newItemPrompt();
    const name = newItemName().trim();
    if (!prompt || !name) {
      setNewItemPrompt(null);
      return;
    }
    if (prompt.type === 'task') {
      createTask(prompt.parentId, name);
    } else {
      createEval(name);
    }
    setNewItemPrompt(null);
  };

  // ==========================================================================
  // Keyboard Shortcuts
  // ==========================================================================

  createEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') return;

      if (e.key === 'Escape') {
        if (editingTask() || editingEval()) {
          setEditingTask(null);
          setEditingEval(null);
        } else if (newItemPrompt()) {
          setNewItemPrompt(null);
        } else if (contextMenu()) {
          setContextMenu(null);
        } else if (dispatchRoots().size > 0) {
          clearDispatchScope();
        }
      }
    };

    document.addEventListener('keydown', handler);
    onCleanup(() => document.removeEventListener('keydown', handler));
  });

  // Fit all tasks in view
  const fitAll = () => {
    const positions = taskPositions();
    if (positions.size === 0 || !viewportRef) return;

    let minX = Infinity, maxX = -Infinity;
    let minY = Infinity, maxY = -Infinity;

    for (const pos of positions.values()) {
      minX = Math.min(minX, pos.x - 150);
      maxX = Math.max(maxX, pos.x + 150);
      minY = Math.min(minY, pos.y - 100);
      maxY = Math.max(maxY, pos.y + 100);
    }

    const padding = 60;
    const vw = viewportRef.clientWidth;
    const vh = viewportRef.clientHeight;
    const scaleX = (vw - padding * 2) / (maxX - minX);
    const scaleY = (vh - padding * 2) / (maxY - minY);
    const scale = Math.max(MIN_ZOOM, Math.min(scaleX, scaleY, MAX_ZOOM));

    const centerX = (minX + maxX) / 2;
    const centerY = (minY + maxY) / 2;

    setTransform({
      x: vw / 2 - centerX * scale,
      y: vh / 2 - centerY * scale,
      k: scale,
    });
  };

  // ==========================================================================
  // Render
  // ==========================================================================

  return (
    <div
      class="flex-1 flex flex-col overflow-hidden relative"
      style={{
        background: 'linear-gradient(145deg, #0f0f0f 0%, #1a1815 50%, #0f0f0f 100%)',
      }}
    >
      {/* Canvas layer for grid and edges */}
      <canvas
        ref={canvasRef}
        class="absolute inset-0 w-full h-full pointer-events-none"
        style={{ 'z-index': 0 }}
      />

      {/* Viewport */}
      <div
        ref={viewportRef}
        class="absolute inset-0 overflow-hidden"
        style={{
          'z-index': 1,
          cursor: taskDragging() ? 'grabbing' : panning() ? 'grabbing' : 'grab',
        }}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
        onMouseLeave={handleMouseUp}
        onWheel={handleWheel}
        onContextMenu={(e) => handleContextMenu(e, null, null)}
      >
        {/* Transformed container */}
        <div
          class="absolute origin-top-left will-change-transform"
          style={{
            transform: `translate(${transform().x}px, ${transform().y}px) scale(${transform().k})`,
          }}
        >
          {/* Tasks */}
          <For each={flattenTree(taskTree())}>
            {(task) => {
              const pos = () => taskPositions().get(task.id);
              return (
                <Show when={pos()}>
                  <TaskCard
                    task={task}
                    position={pos()!}
                    load={taskLoad()}
                    selected={selectedTaskId() === task.id}
                    editing={gypEditingIslands().has(task.name.toLowerCase())}
                    zoom={transform().k}
                    inDispatchScope={dispatchTaskIds().has(task.id)}
                    isDispatchRoot={dispatchRoots().has(task.id)}
                    activeRun={getActiveRunForTask(task.id)}
                    onClick={(e) => {
                      if (e.shiftKey) {
                        toggleDispatchScope(task.id);
                      } else {
                        setSelectedTaskId(task.id);
                        setSelectedEvalId(null);
                      }
                    }}
                    onDoubleClick={() => openTaskEdit(task)}
                    onContextMenu={(e) => handleContextMenu(e, task.id, null)}
                    onAskGyp={(e) => {
                      e.stopPropagation();
                      window.dispatchEvent(
                        new CustomEvent('gyp-focus-node', {
                          detail: { id: task.id, name: task.name },
                        })
                      );
                      app.setAiChatOpen(true);
                    }}
                    onDragStart={handleTaskDragStart}
                  />
                </Show>
              );
            }}
          </For>

          {/* Evals */}
          <For each={evals()}>
            {(ev) => {
              const pos = () => taskPositions().get(ev.id);
              return (
                <Show when={pos()}>
                  <EvalCard
                    eval={ev}
                    position={pos()!}
                    load={taskLoad()}
                    selected={selectedEvalId() === ev.id}
                    zoom={transform().k}
                    inDispatchScope={dispatchEvalIds().has(ev.id)}
                    onClick={(e) => {
                      // Evals can't be dispatch roots, just show selection
                      setSelectedEvalId(ev.id);
                      setSelectedTaskId(null);
                    }}
                    onDoubleClick={() => openEvalEdit(ev)}
                    onContextMenu={(e) => handleContextMenu(e, null, ev.id)}
                    onDragStart={handleEvalDragStart}
                  />
                </Show>
              );
            }}
          </For>
        </div>
      </div>

      {/* HUD Toolbar */}
      <div
        class="absolute top-4 right-4 flex items-center gap-2"
        style={{ 'z-index': 50, 'pointer-events': 'auto' }}
      >
        <button
          onClick={fitAll}
          class="p-2 rounded-lg text-zinc-500 hover:text-zinc-200 hover:bg-zinc-800 transition-all"
          style={{ background: 'rgba(39,39,42,0.8)', border: '1px solid rgba(63,63,70,0.5)' }}
          title="Fit all"
        >
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M4 8V4m0 0h4M4 4l5 5m11-1V4m0 0h-4m4 0l-5 5M4 16v4m0 0h4m-4 0l5-5m11 5l-5-5m5 5v-4m0 4h-4"
            />
          </svg>
        </button>
      </div>

      {/* Runs Overlay */}
      <RunsOverlay
        projectId={project.selectedProjectId()}
        onViewRun={(runName) => {
          runs.setSelectedRun(runName);
        }}
      />

      {/* Dispatch Bar */}
      <Show when={dispatchRoots().size > 0}>
        <div
          class="dispatch-bar absolute bottom-6 left-1/2 -translate-x-1/2 flex items-center gap-4 px-5 py-3 rounded-lg bg-pasture-800 border border-pasture-600"
          style={{
            'z-index': 50,
            'pointer-events': 'auto',
            'box-shadow': '0 8px 24px rgba(0,0,0,0.3)',
          }}
        >
          {/* Stats */}
          <div class="flex items-center gap-4 text-sm">
            <div class="flex items-center gap-2">
              <span class="text-wool-500">Roots</span>
              <span class="text-amber-500 font-semibold tabular-nums">{dispatchRoots().size}</span>
            </div>
            <span class="text-wool-700">·</span>
            <div class="flex items-center gap-2">
              <span class="text-wool-500">Tasks</span>
              <span class="text-wool-100 tabular-nums">{dispatchTaskIds().size}</span>
            </div>
            <Show when={dispatchEvalIds().size > 0}>
              <span class="text-wool-700">·</span>
              <div class="flex items-center gap-2">
                <span class="text-wool-500">Evals</span>
                <span class="text-sage tabular-nums">{dispatchEvalIds().size}</span>
              </div>
            </Show>
          </div>

          {/* Actions */}
          <div class="flex items-center gap-2 pl-4 border-l border-pasture-600">
            <button
              onClick={clearDispatchScope}
              class="p-2 rounded-md text-wool-500 hover:text-wool-100 hover:bg-pasture-700 transition-colors"
              title="Clear selection (Esc)"
            >
              <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
              </svg>
            </button>
            <button
              onClick={() => setShowDispatchModal(true)}
              class="btn-warning px-4 py-2 rounded-md text-sm font-medium transition-colors"
              title="Start run with selected scope"
            >
              Dispatch
            </button>
          </div>
        </div>
      </Show>

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
        <NodeContextMenu
          x={contextMenu()!.x}
          y={contextMenu()!.y}
          node={
            contextMenu()!.taskId
              ? findNodeById(taskTree(), contextMenu()!.taskId!) ?? null
              : null
          }
          onClose={hideContextMenu}
          onAddChild={() => {
            const cm = contextMenu()!;
            showNewItemPrompt(cm.x, cm.y, cm.taskId, 'task');
            hideContextMenu();
          }}
          onAddSibling={() => {
            const cm = contextMenu()!;
            const allTasks = flattenTree(taskTree());
            const parentTask = allTasks.find((t) => t.children.some((c) => c.id === cm.taskId));
            showNewItemPrompt(cm.x, cm.y, parentTask?.id ?? null, 'task');
            hideContextMenu();
          }}
          onEdit={() => {
            const cm = contextMenu()!;
            if (cm.taskId) {
              const task = findNodeById(taskTree(), cm.taskId);
              if (task) openTaskEdit(task);
            } else if (cm.evalId) {
              const ev = evals().find((e) => e.id === cm.evalId);
              if (ev) openEvalEdit(ev);
            }
            hideContextMenu();
          }}
          onAskGyp={() => {
            const cm = contextMenu()!;
            const task = findNodeById(taskTree(), cm.taskId!);
            if (task) {
              window.dispatchEvent(
                new CustomEvent('gyp-focus-node', { detail: { id: task.id, name: task.name } })
              );
              app.setAiChatOpen(true);
            }
            hideContextMenu();
          }}
          onSetTaskStatus={(status) => {
            const cm = contextMenu()!;
            if (cm.taskId) updateTask(cm.taskId, { status });
            hideContextMenu();
          }}
          onDelete={() => {
            const cm = contextMenu()!;
            if (cm.taskId) deleteTask(cm.taskId);
            else if (cm.evalId) deleteEval(cm.evalId);
            hideContextMenu();
          }}
          onAddRootNode={() => {
            const cm = contextMenu()!;
            showNewItemPrompt(cm.x, cm.y, null, 'task');
            hideContextMenu();
          }}
          onAddEval={() => {
            const cm = contextMenu()!;
            showNewItemPrompt(cm.x, cm.y, null, 'eval');
            hideContextMenu();
          }}
          onFitAll={() => {
            fitAll();
            hideContextMenu();
          }}
        />
      </Show>

      {/* New Item Prompt */}
      <Show when={newItemPrompt()}>
        <div
          class="fixed w-72 z-[100] card"
          classList={{
            'border-emerald-500/40': newItemPrompt()!.type === 'eval',
          }}
          style={{
            left: `${newItemPrompt()!.x}px`,
            top: `${newItemPrompt()!.y}px`,
            transform: 'translate(-50%, -50%)',
          }}
        >
          <header class="flex items-center gap-3">
            <div
              class="w-8 h-8 rounded-lg flex items-center justify-center"
              classList={{
                'bg-emerald-500/15 border border-emerald-500/25': newItemPrompt()!.type === 'eval',
                'bg-amber-500/15 border border-amber-500/25': newItemPrompt()!.type !== 'eval',
              }}
            >
              <svg
                class={`w-4 h-4 ${newItemPrompt()!.type === 'eval' ? 'text-emerald-400' : 'text-amber-400'}`}
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
              </svg>
            </div>
            <div>
              <h3 class="text-sm font-medium text-wool-200">
                {newItemPrompt()!.type === 'eval'
                  ? 'New Eval'
                  : newItemPrompt()!.parentId
                    ? 'New Child Task'
                    : 'New Root Task'}
              </h3>
              <p class="text-[11px] text-wool-500">
                {newItemPrompt()!.type === 'eval' ? 'Verification for tasks' : 'A work item'}
              </p>
            </div>
          </header>
          <section>
            <form class="form">
              <input
                ref={newItemInputRef}
                type="text"
                value={newItemName()}
                onInput={(e) => setNewItemName(e.currentTarget.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && !e.shiftKey) {
                    e.preventDefault();
                    submitNewItem();
                  }
                  if (e.key === 'Escape') setNewItemPrompt(null);
                }}
                placeholder={`${newItemPrompt()!.type === 'eval' ? 'Eval' : 'Task'} name...`}
              />
            </form>
          </section>
          <footer class="flex items-center justify-between">
            <span class="text-[11px] text-wool-600">Enter to create</span>
            <div class="flex gap-2">
              <button
                onClick={() => setNewItemPrompt(null)}
                class="btn-ghost btn-sm"
              >
                Cancel
              </button>
              <button
                onClick={submitNewItem}
                disabled={!newItemName().trim()}
                class={`btn-sm ${newItemPrompt()!.type === 'eval' ? 'btn-success' : 'btn'}`}
              >
                Create
              </button>
            </div>
          </footer>
        </div>
      </Show>

      {/* Edit Modal */}
      <Show when={editingTask() || editingEval()}>
        <dialog
          open
          class="dialog fixed inset-0 z-[100] m-0 h-full w-full max-w-none max-h-none bg-transparent flex items-center justify-center"
          style={{ 'backdrop-filter': 'blur(4px)' }}
          onClick={() => {
            setEditingTask(null);
            setEditingEval(null);
          }}
        >
          <div class="card w-full max-w-md" onClick={(e) => e.stopPropagation()}>
            <header>
              <h3 class="text-sm font-semibold text-wool-200">
                Edit {editingTask() ? 'Task' : 'Eval'}
              </h3>
            </header>
            <section>
              <form class="form grid gap-4">
                <div class="grid gap-2">
                  <label for="edit-name" class="text-xs font-medium text-wool-400">Name</label>
                  <input
                    id="edit-name"
                    type="text"
                    value={editForm().name}
                    onInput={(e) => setEditForm((f) => ({ ...f, name: e.currentTarget.value }))}
                  />
                </div>
                <div class="grid gap-2">
                  <label for="edit-content" class="text-xs font-medium text-wool-400">Content</label>
                  <textarea
                    id="edit-content"
                    value={editForm().content}
                    onInput={(e) => setEditForm((f) => ({ ...f, content: e.currentTarget.value }))}
                    rows={4}
                    placeholder={editingTask() ? 'Task details...' : 'What to verify...'}
                  />
                </div>
                <Show when={editingEval()}>
                  <div class="grid gap-2">
                    <label for="edit-validates" class="text-xs font-medium text-emerald-400/70">
                      Validates (task IDs)
                    </label>
                    <input
                      id="edit-validates"
                      type="text"
                      value={editForm().validates.join(', ')}
                      onInput={(e) =>
                        setEditForm((f) => ({
                          ...f,
                          validates: e.currentTarget.value
                            .split(',')
                            .map((s) => s.trim())
                            .filter(Boolean),
                        }))
                      }
                      placeholder="task-1, task-2"
                    />
                  </div>
                </Show>
              </form>
            </section>
            <footer>
              <button
                onClick={() => {
                  setEditingTask(null);
                  setEditingEval(null);
                }}
                class="btn-ghost"
              >
                Cancel
              </button>
              <button onClick={saveEdit} class="btn">
                Save
              </button>
            </footer>
          </div>
        </dialog>
      </Show>

      {/* Empty state - no project selected */}
      <Show when={!project.selectedProject() && !project.loading()}>
        <div
          class="absolute inset-0 flex items-center justify-center pointer-events-none"
          style={{ 'z-index': 5 }}
        >
          <div class="text-center">
            <div
              class="w-20 h-20 mx-auto mb-4 rounded-2xl flex items-center justify-center"
              style={{
                background: 'linear-gradient(145deg, rgba(212,165,116,0.1) 0%, rgba(212,165,116,0.05) 100%)',
                border: '1px solid rgba(212,165,116,0.2)',
              }}
            >
              <svg class="w-10 h-10 text-amber-500/50" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z"
                />
              </svg>
            </div>
            <h2 class="text-lg font-medium text-wool-300 mb-1">No Project Selected</h2>
            <p class="text-sm text-wool-600 mb-4">Select a project from the dropdown above</p>
            <button
              onClick={() => project.openProjectSetup()}
              class="pointer-events-auto px-4 py-2 rounded-lg text-sm font-medium transition-all"
              style={{
                background: 'linear-gradient(180deg, rgba(212,165,116,0.3) 0%, rgba(212,165,116,0.2) 100%)',
                border: '1px solid rgba(212,165,116,0.5)',
                color: 'rgb(232,193,154)',
              }}
            >
              New Project
            </button>
          </div>
        </div>
      </Show>

      {/* Empty task state - project selected but no tasks */}
      <Show when={project.selectedProject() && !loading() && taskTree().length === 0 && evals().length === 0}>
        <div
          class="absolute inset-0 flex items-center justify-center pointer-events-none"
          style={{ 'z-index': 5 }}
        >
          <div class="text-center">
            <div
              class="w-20 h-20 mx-auto mb-4 rounded-2xl flex items-center justify-center"
              style={{
                background: 'linear-gradient(145deg, rgba(212,165,116,0.1) 0%, rgba(212,165,116,0.05) 100%)',
                border: '1px solid rgba(212,165,116,0.2)',
              }}
            >
              <svg class="w-10 h-10 text-amber-500/50" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2"
                />
              </svg>
            </div>
            <h2 class="text-lg font-medium text-wool-300 mb-1">No Tasks Yet</h2>
            <p class="text-sm text-wool-600">Right-click to add a task or eval</p>
          </div>
        </div>
      </Show>

      {/* Dispatch Modal */}
      <Show when={showDispatchModal()}>
        <DispatchModal
          rootTaskIds={[...dispatchRoots()]}
          taskCount={dispatchTaskIds().size}
          evalCount={dispatchEvalIds().size}
          onClose={() => setShowDispatchModal(false)}
          onDispatch={(runName) => {
            setShowDispatchModal(false);
            clearDispatchScope();
            window.toast?.success(`Dispatched run: ${runName}`);
          }}
        />
      </Show>

      {/* Animations */}
      <style>{`
        .pulse-glow {
          animation: pulse-glow 2s ease-in-out infinite;
        }
        @keyframes pulse-glow {
          0%, 100% { box-shadow: 0 0 8px rgba(212, 165, 116, 0.5); }
          50% { box-shadow: 0 0 16px rgba(212, 165, 116, 0.8); }
        }
        .pulse-glow-sage {
          animation: pulse-glow-sage 2s ease-in-out infinite;
        }
        @keyframes pulse-glow-sage {
          0%, 100% { box-shadow: 0 0 8px rgba(125, 153, 112, 0.5); }
          50% { box-shadow: 0 0 16px rgba(125, 153, 112, 0.8); }
        }
        .gyp-editing-shimmer {
          position: relative;
          overflow: hidden;
        }
        .gyp-editing-shimmer::after {
          content: '';
          position: absolute;
          inset: 0;
          background: linear-gradient(90deg, transparent, rgba(212,165,116,0.15), transparent);
          animation: gyp-shimmer 1.5s infinite;
          pointer-events: none;
          border-radius: inherit;
        }
        @keyframes gyp-shimmer {
          0% { transform: translateX(-100%); }
          100% { transform: translateX(100%); }
        }
      `}</style>
    </div>
  );
};

export default OneBoard;
