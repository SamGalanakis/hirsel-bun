/**
 * SpecFlow Board - Canvas for project planning with Tasks and Evals
 *
 * Tasks: Nested tree of work items (post-it style)
 * Evals: Flat list of verifications that validate tasks
 *
 * Validation propagates up the tree:
 * - A task is "validated" if it has a passing eval OR all children are validated
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
import type {
  TaskTree,
  BoardEval,
  Bookmark,
  BoardSyncResult,
  BoardTaskStatus,
  BoardEvalStatus,
} from '../../lib/types';
import { flattenTree, findNodeById } from '../../lib/utils/tree';
import { useProject } from '../../stores';
import { useBoardChat } from '../../hooks';
import { GypChatDrawer } from './GypChatDrawer';
import { TaskCard, EvalCard } from './NodeRenderer';
import { NodeContextMenu } from './NodeContextMenu';
import {
  computeTreeLayout,
  generateEdgePath,
  getLOADLevel,
  getLayoutConfigForZoom,
  type LOADLevel,
  type NodePosition,
} from './use-tree-layout';

interface Transform {
  x: number;
  y: number;
  k: number;
}

interface ContextMenuState {
  x: number;
  y: number;
  worldX: number;
  worldY: number;
  taskId: string | null;
  evalId: string | null;
}

// Canvas grid renderer
class CanvasRenderer {
  private ctx: CanvasRenderingContext2D;
  private dpr: number;

  constructor(canvas: HTMLCanvasElement) {
    const ctx = canvas.getContext('2d');
    if (!ctx) throw new Error('Failed to get 2d context');
    this.ctx = ctx;
    this.dpr = window.devicePixelRatio || 1;
  }

  render(
    transform: Transform,
    positions: Map<string, NodePosition>,
    tree: TaskTree[],
    nodeHeight: number,
    viewportWidth: number,
    viewportHeight: number
  ) {
    const { ctx, dpr } = this;
    const { x, y, k } = transform;

    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, viewportWidth * dpr, viewportHeight * dpr);
    ctx.setTransform(dpr * k, 0, 0, dpr * k, dpr * x, dpr * y);

    this.drawGrid(transform, viewportWidth, viewportHeight);
    this.drawEdges(positions, tree, nodeHeight);
  }

  private drawGrid(transform: Transform, vw: number, vh: number) {
    const { ctx } = this;
    const { x, y, k } = transform;

    const majorSpacing = 100;
    const minorSpacing = 20;
    const showMinor = k >= 0.4;

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
  }

  private drawEdges(positions: Map<string, NodePosition>, tree: TaskTree[], nodeHeight: number) {
    const { ctx } = this;

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
  }
}

export const SpecflowBoard: Component = () => {
  const project = useProject();

  // Refs
  let viewportRef: HTMLDivElement | undefined;
  let canvasRef: HTMLCanvasElement | undefined;
  let containerRef: HTMLDivElement | undefined;
  let canvasRenderer: CanvasRenderer | null = null;

  // Transform state
  const [transform, setTransform] = createSignal<Transform>({ x: 100, y: 100, k: 1 });

  // Data - separate tasks and evals
  const [taskTree, setTaskTree] = createSignal<TaskTree[]>([]);
  const [evals, setEvals] = createSignal<BoardEval[]>([]);
  const [bookmarks, setBookmarks] = createSignal<Bookmark[]>([]);
  const [loading, setLoading] = createSignal(true);

  // Layout
  const load = createMemo<LOADLevel>(() => getLOADLevel(transform().k));
  const layoutConfig = createMemo(() => getLayoutConfigForZoom(transform().k));
  const layout = createMemo(() => computeTreeLayout(taskTree(), layoutConfig()));

  // Selection - can select either a task or an eval
  const [selectedTaskId, setSelectedTaskId] = createSignal<string | null>(null);
  const [selectedEvalId, setSelectedEvalId] = createSignal<string | null>(null);

  const selectedTask = createMemo(() => {
    const id = selectedTaskId();
    return id ? findNodeById(taskTree(), id) : undefined;
  });

  const selectedEval = createMemo(() => {
    const id = selectedEvalId();
    return id ? evals().find((e) => e.id === id) : undefined;
  });

  // Interaction - panning
  const [panning, setPanning] = createSignal(false);
  const [panStart, setPanStart] = createSignal({ x: 0, y: 0 });

  // Interaction - node dragging
  const [dragging, setDragging] = createSignal<{
    nodeId: string;
    isEval: boolean;
    startX: number;
    startY: number;
    initialPositions: Map<string, { x: number; y: number }>;
  } | null>(null);
  const [dragPositions, setDragPositions] = createSignal<Map<string, NodePosition>>(new Map());

  // Context menu
  const [contextMenu, setContextMenu] = createSignal<ContextMenuState | null>(null);

  // Edit modal
  const [editingTask, setEditingTask] = createSignal<TaskTree | null>(null);
  const [editingEval, setEditingEval] = createSignal<BoardEval | null>(null);
  const [editForm, setEditForm] = createSignal({
    name: '',
    content: '',
    validates: [] as string[],
  });

  // Command palette
  const [showCommandPalette, setShowCommandPalette] = createSignal(false);
  const [commandQuery, setCommandQuery] = createSignal('');
  let commandInputRef: HTMLInputElement | undefined;

  // New item prompt
  const [newItemPrompt, setNewItemPrompt] = createSignal<{
    x: number;
    y: number;
    parentId: string | null;
    type: 'task' | 'eval';
  } | null>(null);
  const [newItemName, setNewItemName] = createSignal('');
  let newItemInputRef: HTMLInputElement | undefined;

  // Gyp chat focus
  const [gypFocusTaskId, setGypFocusTaskId] = createSignal<string | null>(null);
  const [gypFocusTaskName, setGypFocusTaskName] = createSignal<string | null>(null);

  // Board chat hook
  const boardChat = useBoardChat(() => project.selectedProjectId(), {
    historyDepth: 10,
    onEditComplete: async () => {
      const pid = project.selectedProjectId();
      if (pid) {
        try {
          await invoke('import_board_from_agent', { project_id: pid });
        } catch (e) {
          console.error('Failed to import board changes:', e);
        }
        loadData(pid);
      }
    },
  });

  // ==========================================================================
  // Data Loading
  // ==========================================================================

  const loadData = async (projectId: number) => {
    setLoading(true);
    try {
      const [treeData, evalData, bookmarkData] = await Promise.all([
        invoke<TaskTree[]>('get_board_task_tree', { project_id: projectId }),
        invoke<BoardEval[]>('get_board_evals', { project_id: projectId }),
        invoke<Bookmark[]>('get_bookmarks', { project_id: projectId }),
      ]);
      batch(() => {
        setTaskTree(treeData || []);
        setEvals(evalData || []);
        setBookmarks(bookmarkData || []);
      });
    } catch (e) {
      console.error('Failed to load board data:', e);
    } finally {
      setLoading(false);
    }
  };

  createEffect(() => {
    const projectId = project.selectedProjectId();
    if (projectId) {
      loadData(projectId);
    } else {
      setTaskTree([]);
      setEvals([]);
      setBookmarks([]);
    }
  });

  // Poll for changes
  createEffect(() => {
    const pid = project.selectedProjectId();
    if (!pid) return;

    const interval = setInterval(async () => {
      try {
        const result = await invoke<BoardSyncResult>('poll_board_changes', { project_id: pid });
        if (result.changes > 0) {
          await loadData(pid);
        }
      } catch (e) {
        console.error('Failed to poll board changes:', e);
      }
    }, 2500);

    onCleanup(() => clearInterval(interval));
  });

  // ==========================================================================
  // Canvas Rendering
  // ==========================================================================

  const getEffectivePositions = (): Map<string, NodePosition> => {
    const positions = new Map<string, NodePosition>();
    const layoutPositions = layout().positions;
    const dragPos = dragPositions();

    // Start with layout positions for tasks
    for (const [id, pos] of layoutPositions) {
      positions.set(id, pos);
    }

    // Add eval positions (use stored x,y or auto-position)
    const evalList = evals();
    const treeNodes = flattenTree(taskTree());
    for (let i = 0; i < evalList.length; i++) {
      const ev = evalList[i];
      if (ev.x != null && ev.y != null) {
        positions.set(ev.id, { x: ev.x, y: ev.y });
      } else {
        // Auto-position: to the right of the board
        const bounds = layout().bounds;
        positions.set(ev.id, {
          x: bounds.maxX + 200,
          y: bounds.minY + i * 120,
        });
      }
    }

    // Override with drag positions
    for (const [id, pos] of dragPos) {
      positions.set(id, pos);
    }

    return positions;
  };

  const render = () => {
    if (!canvasRenderer || !viewportRef) return;
    canvasRenderer.render(
      transform(),
      getEffectivePositions(),
      taskTree(),
      layoutConfig().nodeHeight,
      viewportRef.clientWidth,
      viewportRef.clientHeight
    );
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
      canvasRenderer = new CanvasRenderer(canvasRef);
      handleResize();

      const resizeObserver = new ResizeObserver(handleResize);
      if (viewportRef) resizeObserver.observe(viewportRef);
      onCleanup(() => resizeObserver.disconnect());
    }
  });

  createEffect(() => {
    transform();
    layout();
    evals();
    dragPositions();
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
    const newK = Math.max(0.15, Math.min(3, t.k * delta));

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
    const drag = dragging();
    if (drag) {
      const t = transform();
      const dx = (e.clientX - drag.startX) / t.k;
      const dy = (e.clientY - drag.startY) / t.k;

      const newPositions = new Map<string, NodePosition>();
      for (const [id, initial] of drag.initialPositions) {
        newPositions.set(id, {
          x: initial.x + dx,
          y: initial.y + dy,
        });
      }
      setDragPositions(newPositions);
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
    const drag = dragging();
    if (drag) {
      const projectId = project.selectedProjectId();
      if (projectId) {
        const positions = dragPositions();
        const updates = Array.from(positions.entries()).map(([id, pos]) => {
          if (drag.isEval) {
            return invoke('update_board_eval', {
              project_id: projectId,
              eval_id: id,
              x: pos.x,
              y: pos.y,
            });
          }
          return invoke('update_board_task', {
            project_id: projectId,
            task_id: id,
            x: pos.x,
            y: pos.y,
          });
        });

        try {
          await Promise.all(updates);
          await loadData(projectId);
        } catch (e) {
          console.error('Failed to save positions:', e);
          window.toast?.error('Failed to save positions');
        }
      }

      setDragging(null);
      setDragPositions(new Map());
      return;
    }

    setPanning(false);
  };

  // ==========================================================================
  // Dragging
  // ==========================================================================

  const collectDescendantIds = (nodeId: string): string[] => {
    const ids: string[] = [nodeId];
    const collectChildren = (node: TaskTree) => {
      for (const child of node.children) {
        ids.push(child.id);
        collectChildren(child);
      }
    };
    const node = findNodeById(taskTree(), nodeId);
    if (node) collectChildren(node);
    return ids;
  };

  const getItemPosition = (id: string, isEval: boolean): NodePosition | undefined => {
    const dragPos = dragPositions().get(id);
    if (dragPos) return dragPos;

    if (isEval) {
      const ev = evals().find((e) => e.id === id);
      if (ev?.x != null && ev?.y != null) {
        return { x: ev.x, y: ev.y };
      }
    }

    return layout().positions.get(id);
  };

  const handleTaskDragStart = (e: MouseEvent, taskId: string) => {
    e.preventDefault();
    const idsToMove = collectDescendantIds(taskId);
    const initialPositions = new Map<string, { x: number; y: number }>();
    for (const id of idsToMove) {
      const pos = getItemPosition(id, false);
      if (pos) initialPositions.set(id, { x: pos.x, y: pos.y });
    }

    setDragging({
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
    const pos = getItemPosition(evalId, true);
    const initialPositions = new Map<string, { x: number; y: number }>();
    if (pos) initialPositions.set(evalId, { x: pos.x, y: pos.y });

    setDragging({
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
  // Zoom Controls
  // ==========================================================================

  const zoomIn = () => setTransform((t) => ({ ...t, k: Math.min(t.k * 1.3, 3) }));
  const zoomOut = () => setTransform((t) => ({ ...t, k: Math.max(t.k / 1.3, 0.15) }));

  const fitAll = () => {
    const bounds = layout().bounds;
    if (bounds.width === 0 || !viewportRef) return;

    const padding = 80;
    const vw = viewportRef.clientWidth;
    const vh = viewportRef.clientHeight;
    const scaleX = (vw - padding * 2) / bounds.width;
    const scaleY = (vh - padding * 2) / bounds.height;
    const scale = Math.min(scaleX, scaleY, 1);

    const centerX = bounds.minX + bounds.width / 2;
    const centerY = bounds.minY + bounds.height / 2;

    setTransform({
      x: vw / 2 - centerX * scale,
      y: vh / 2 - centerY * scale,
      k: scale,
    });
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

    setContextMenu({
      x: e.clientX,
      y: e.clientY,
      worldX,
      worldY,
      taskId,
      evalId,
    });
  };

  const hideContextMenu = () => setContextMenu(null);

  // ==========================================================================
  // Task Operations
  // ==========================================================================

  const createTask = async (parentId: string | null, name: string) => {
    const projectId = project.selectedProjectId();
    if (!projectId || !name.trim()) return;

    try {
      const task = await invoke<any>('create_board_task', {
        project_id: projectId,
        parent_id: parentId,
        name: name.trim(),
      });
      await loadData(projectId);
      setSelectedTaskId(task.id);
      setSelectedEvalId(null);
    } catch (e) {
      console.error('Failed to create task:', e);
      window.toast?.error('Failed to create task');
    }
  };

  const updateTask = async (
    taskId: string,
    updates: { name?: string; status?: BoardTaskStatus; content?: string }
  ) => {
    const projectId = project.selectedProjectId();
    if (!projectId) return;

    try {
      await invoke('update_board_task', {
        project_id: projectId,
        task_id: taskId,
        ...updates,
      });
      await loadData(projectId);
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
      await invoke('delete_board_task', { project_id: projectId, task_id: taskId });
      await loadData(projectId);
      if (selectedTaskId() === taskId) setSelectedTaskId(null);
      window.toast?.success('Task deleted');
    } catch (e) {
      console.error('Failed to delete task:', e);
      window.toast?.error(`Failed to delete task: ${e}`);
    }
  };

  // ==========================================================================
  // Eval Operations
  // ==========================================================================

  const createEval = async (name: string) => {
    const projectId = project.selectedProjectId();
    if (!projectId || !name.trim()) return;

    try {
      const ev = await invoke<BoardEval>('create_board_eval', {
        project_id: projectId,
        name: name.trim(),
      });
      await loadData(projectId);
      setSelectedEvalId(ev.id);
      setSelectedTaskId(null);
    } catch (e) {
      console.error('Failed to create eval:', e);
      window.toast?.error('Failed to create eval');
    }
  };

  const updateEval = async (
    evalId: string,
    updates: { name?: string; status?: BoardEvalStatus; content?: string; validates?: string[] }
  ) => {
    const projectId = project.selectedProjectId();
    if (!projectId) return;

    try {
      await invoke('update_board_eval', {
        project_id: projectId,
        eval_id: evalId,
        ...updates,
      });
      await loadData(projectId);
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
      await invoke('delete_board_eval', { project_id: projectId, eval_id: evalId });
      await loadData(projectId);
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
      await updateTask(task.id, {
        name: editForm().name,
        content: editForm().content,
      });
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

  // ==========================================================================
  // New Item Prompt
  // ==========================================================================

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
  // Command Palette
  // ==========================================================================

  const filteredResults = createMemo(() => {
    const q = commandQuery().toLowerCase();
    const allTasks = flattenTree(taskTree());
    return {
      tasks: allTasks.filter((n) => n.name.toLowerCase().includes(q)),
      evals: evals().filter((e) => e.name.toLowerCase().includes(q)),
      bookmarks: bookmarks().filter((b) => b.name.toLowerCase().includes(q)),
    };
  });

  const jumpToTask = (task: TaskTree) => {
    const pos = layout().positions.get(task.id);
    if (!pos || !viewportRef) return;

    setTransform({
      x: viewportRef.clientWidth / 2 - pos.x,
      y: viewportRef.clientHeight / 2 - pos.y,
      k: 1,
    });
    setSelectedTaskId(task.id);
    setSelectedEvalId(null);
    setShowCommandPalette(false);
  };

  const jumpToBookmark = (bookmark: Bookmark) => {
    if (!viewportRef) return;
    setTransform({
      x: viewportRef.clientWidth / 2 - bookmark.x * bookmark.zoom,
      y: viewportRef.clientHeight / 2 - bookmark.y * bookmark.zoom,
      k: bookmark.zoom,
    });
    setShowCommandPalette(false);
  };

  // ==========================================================================
  // Keyboard Shortcuts
  // ==========================================================================

  createEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') return;

      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault();
        setShowCommandPalette(true);
        setCommandQuery('');
        setTimeout(() => commandInputRef?.focus(), 50);
        return;
      }

      if (e.key === 'Escape') {
        if (editingTask() || editingEval()) {
          setEditingTask(null);
          setEditingEval(null);
        } else if (showCommandPalette()) {
          setShowCommandPalette(false);
        } else if (newItemPrompt()) {
          setNewItemPrompt(null);
        } else if (contextMenu()) {
          setContextMenu(null);
        } else if (gypFocusTaskId()) {
          setGypFocusTaskId(null);
          setGypFocusTaskName(null);
        }
      }
    };

    document.addEventListener('keydown', handler);
    onCleanup(() => document.removeEventListener('keydown', handler));
  });

  createEffect(() => {
    const handler = () => setContextMenu(null);
    document.addEventListener('click', handler);
    onCleanup(() => document.removeEventListener('click', handler));
  });

  // ==========================================================================
  // Render
  // ==========================================================================

  const allPositions = createMemo(() => getEffectivePositions());

  return (
    <div
      class="flex-1 flex flex-col overflow-hidden relative"
      style={{
        background: 'linear-gradient(145deg, #0f0f0f 0%, #1a1815 50%, #0f0f0f 100%)',
      }}
    >
      {/* Canvas layer */}
      <canvas
        ref={canvasRef}
        class="absolute inset-0 w-full h-full pointer-events-none"
        style={{ 'z-index': 0 }}
      />

      {/* Viewport container */}
      <div
        ref={viewportRef}
        class="absolute inset-0 overflow-hidden"
        style={{
          'z-index': 1,
          cursor: dragging() ? 'grabbing' : panning() ? 'grabbing' : 'grab',
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
          ref={containerRef}
          class="absolute origin-top-left will-change-transform"
          style={{
            transform: `translate(${transform().x}px, ${transform().y}px) scale(${transform().k})`,
          }}
        >
          {/* Render Tasks */}
          <For each={flattenTree(taskTree())}>
            {(task) => {
              const pos = () => allPositions().get(task.id);
              return (
                <Show when={pos()}>
                  <TaskCard
                    task={task}
                    position={pos()!}
                    load={load()}
                    selected={selectedTaskId() === task.id}
                    editing={boardChat.editingIslands().has(task.name.toLowerCase())}
                    onClick={() => {
                      setSelectedTaskId(task.id);
                      setSelectedEvalId(null);
                    }}
                    onDoubleClick={() => openTaskEdit(task)}
                    onContextMenu={(e) => handleContextMenu(e, task.id, null)}
                    onAskGyp={(e) => {
                      e.stopPropagation();
                      setGypFocusTaskId(task.id);
                      setGypFocusTaskName(task.name);
                    }}
                    onDragStart={handleTaskDragStart}
                  />
                </Show>
              );
            }}
          </For>

          {/* Render Evals */}
          <For each={evals()}>
            {(ev) => {
              const pos = () => allPositions().get(ev.id);
              return (
                <Show when={pos()}>
                  <EvalCard
                    eval={ev}
                    position={pos()!}
                    load={load()}
                    selected={selectedEvalId() === ev.id}
                    onClick={() => {
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
          onClick={() => {
            setShowCommandPalette(true);
            setTimeout(() => commandInputRef?.focus(), 50);
          }}
          class="p-2 rounded-lg text-zinc-500 hover:text-zinc-200 hover:bg-zinc-800 transition-all"
          style={{ background: 'rgba(39,39,42,0.8)', border: '1px solid rgba(63,63,70,0.5)' }}
          title="Search (Cmd+K)"
        >
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"
            />
          </svg>
        </button>

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

      {/* Context Menu */}
      <Show when={contextMenu()}>
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
            const task = findNodeById(taskTree(), cm.taskId!);
            // Find parent ID from tree
            const allTasks = flattenTree(taskTree());
            const parentTask = allTasks.find((t) =>
              t.children.some((c) => c.id === cm.taskId)
            );
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
              setGypFocusTaskId(task.id);
              setGypFocusTaskName(task.name);
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
          class="fixed w-72 rounded-xl overflow-hidden"
          style={{
            'z-index': 100,
            left: `${newItemPrompt()!.x}px`,
            top: `${newItemPrompt()!.y}px`,
            transform: 'translate(-50%, -50%)',
            background: 'rgba(24,24,27,0.98)',
            border: `1px solid ${newItemPrompt()!.type === 'eval' ? 'rgba(16,185,129,0.4)' : 'rgba(63,63,70,0.8)'}`,
            'box-shadow': '0 20px 60px rgba(0,0,0,0.6)',
          }}
        >
          <div
            class="px-4 py-3 flex items-center gap-3"
            style={{
              background:
                newItemPrompt()!.type === 'eval'
                  ? 'linear-gradient(180deg, rgba(16,185,129,0.12) 0%, transparent 100%)'
                  : 'linear-gradient(180deg, rgba(212,165,116,0.08) 0%, transparent 100%)',
              'border-bottom': '1px solid rgba(63,63,70,0.5)',
            }}
          >
            <div
              class="w-8 h-8 rounded-lg flex items-center justify-center"
              style={{
                background:
                  newItemPrompt()!.type === 'eval'
                    ? 'rgba(16,185,129,0.15)'
                    : 'rgba(212,165,116,0.15)',
                border: `1px solid ${newItemPrompt()!.type === 'eval' ? 'rgba(16,185,129,0.25)' : 'rgba(212,165,116,0.25)'}`,
              }}
            >
              <svg
                class={`w-4 h-4 ${newItemPrompt()!.type === 'eval' ? 'text-emerald-400' : 'text-amber-400'}`}
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  d="M12 4v16m8-8H4"
                />
              </svg>
            </div>
            <div>
              <div class="text-sm font-medium text-zinc-200">
                {newItemPrompt()!.type === 'eval'
                  ? 'New Eval'
                  : newItemPrompt()!.parentId
                    ? 'New Child Task'
                    : 'New Root Task'}
              </div>
              <div class="text-[11px] text-zinc-500">
                {newItemPrompt()!.type === 'eval'
                  ? 'Verification for tasks'
                  : 'A work item'}
              </div>
            </div>
          </div>
          <div class="p-4">
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
              class="w-full text-sm text-zinc-200 placeholder-zinc-600 focus:outline-none px-3 py-2.5 rounded-lg transition-all focus:ring-2 focus:ring-amber-500/30"
              style={{ background: 'rgba(0,0,0,0.4)', border: '1px solid rgba(63,63,70,0.6)' }}
            />
            <div class="flex items-center justify-between mt-4">
              <span class="text-[11px] text-zinc-600">Enter to create</span>
              <div class="flex gap-2">
                <button
                  onClick={() => setNewItemPrompt(null)}
                  class="px-3 py-1.5 text-xs rounded-md text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800 transition-colors"
                >
                  Cancel
                </button>
                <button
                  onClick={submitNewItem}
                  disabled={!newItemName().trim()}
                  class="flex items-center gap-1.5 px-4 py-1.5 text-xs font-medium rounded-md transition-all disabled:opacity-40"
                  style={{
                    background:
                      newItemPrompt()!.type === 'eval'
                        ? 'linear-gradient(180deg, rgba(16,185,129,0.3) 0%, rgba(16,185,129,0.2) 100%)'
                        : 'linear-gradient(180deg, rgba(212,165,116,0.3) 0%, rgba(212,165,116,0.2) 100%)',
                    border: `1px solid ${newItemPrompt()!.type === 'eval' ? 'rgba(16,185,129,0.5)' : 'rgba(212,165,116,0.5)'}`,
                    color: newItemPrompt()!.type === 'eval' ? 'rgb(134,239,172)' : 'rgb(232,193,154)',
                  }}
                >
                  Create
                </button>
              </div>
            </div>
          </div>
        </div>
      </Show>

      {/* Edit Modal */}
      <Show when={editingTask() || editingEval()}>
        <div
          class="fixed inset-0 flex items-center justify-center"
          style={{ 'z-index': 100, background: 'rgba(0,0,0,0.6)', 'backdrop-filter': 'blur(4px)' }}
          onClick={() => {
            setEditingTask(null);
            setEditingEval(null);
          }}
        >
          <div
            class="w-full max-w-md rounded-xl overflow-hidden"
            style={{
              background: 'rgba(26,26,26,0.98)',
              border: '1px solid rgba(255,255,255,0.1)',
              'box-shadow': '0 24px 64px rgba(0,0,0,0.5)',
            }}
            onClick={(e) => e.stopPropagation()}
          >
            <div
              class="px-4 py-3"
              style={{ 'border-bottom': '1px solid rgba(255,255,255,0.06)' }}
            >
              <h3 class="text-sm font-semibold text-wool-200">
                Edit {editingTask() ? 'Task' : 'Eval'}
              </h3>
            </div>
            <div class="p-4 space-y-4">
              <div>
                <label class="block text-xs font-medium text-wool-400 mb-1">Name</label>
                <input
                  type="text"
                  value={editForm().name}
                  onInput={(e) => setEditForm((f) => ({ ...f, name: e.currentTarget.value }))}
                  class="w-full px-3 py-2 text-sm rounded-lg bg-black/30 border border-wool-800 text-wool-200 focus:outline-none focus:ring-2 focus:ring-amber-500/30"
                />
              </div>
              <div>
                <label class="block text-xs font-medium text-wool-400 mb-1">Content</label>
                <textarea
                  value={editForm().content}
                  onInput={(e) => setEditForm((f) => ({ ...f, content: e.currentTarget.value }))}
                  rows={4}
                  class="w-full px-3 py-2 text-sm rounded-lg bg-black/30 border border-wool-800 text-wool-200 focus:outline-none focus:ring-2 focus:ring-amber-500/30 resize-none"
                  placeholder={editingTask() ? 'Task details...' : 'What to verify...'}
                />
              </div>
              <Show when={editingEval()}>
                <div>
                  <label class="block text-xs font-medium text-emerald-400/70 mb-1">
                    Validates (task IDs)
                  </label>
                  <input
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
                    class="w-full px-3 py-2 text-sm rounded-lg bg-black/30 border border-emerald-800/50 text-emerald-200 focus:outline-none focus:ring-2 focus:ring-emerald-500/30"
                    placeholder="task-1, task-2"
                  />
                </div>
              </Show>
            </div>
            <div
              class="px-4 py-3 flex justify-end gap-2"
              style={{ 'border-top': '1px solid rgba(255,255,255,0.06)' }}
            >
              <button
                onClick={() => {
                  setEditingTask(null);
                  setEditingEval(null);
                }}
                class="px-4 py-2 text-xs rounded-lg text-wool-400 hover:text-wool-200 hover:bg-white/5 transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={saveEdit}
                class="px-4 py-2 text-xs font-medium rounded-lg transition-all"
                style={{
                  background:
                    'linear-gradient(180deg, rgba(212,165,116,0.3) 0%, rgba(212,165,116,0.2) 100%)',
                  border: '1px solid rgba(212,165,116,0.5)',
                  color: 'rgb(232,193,154)',
                }}
              >
                Save
              </button>
            </div>
          </div>
        </div>
      </Show>

      {/* Command Palette */}
      <Show when={showCommandPalette()}>
        <div
          class="fixed inset-0 flex items-start justify-center pt-[15vh]"
          style={{ 'z-index': 50, background: 'rgba(0,0,0,0.6)', 'backdrop-filter': 'blur(4px)' }}
          onClick={() => setShowCommandPalette(false)}
        >
          <div
            class="w-full max-w-md rounded-xl overflow-hidden"
            style={{
              background: 'rgba(26,26,26,0.98)',
              border: '1px solid rgba(255,255,255,0.1)',
              'box-shadow': '0 24px 64px rgba(0,0,0,0.5)',
            }}
            onClick={(e) => e.stopPropagation()}
          >
            <div
              class="flex items-center gap-3 px-4 py-3"
              style={{ 'border-bottom': '1px solid rgba(255,255,255,0.06)' }}
            >
              <svg class="w-4 h-4 text-wool-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"
                />
              </svg>
              <input
                ref={commandInputRef}
                type="text"
                value={commandQuery()}
                onInput={(e) => setCommandQuery(e.currentTarget.value)}
                placeholder="Search tasks, evals, bookmarks..."
                class="flex-1 bg-transparent text-sm text-wool-200 placeholder-wool-600 focus:outline-none"
              />
            </div>
            <div class="max-h-72 overflow-y-auto">
              <Show when={filteredResults().tasks.length > 0}>
                <div class="py-1">
                  <div class="px-3 py-1.5 text-[10px] font-semibold text-wool-600 uppercase tracking-wider">
                    Tasks
                  </div>
                  <For each={filteredResults().tasks}>
                    {(task) => (
                      <button
                        onClick={() => jumpToTask(task)}
                        class="w-full px-3 py-2 text-left text-sm text-wool-300 hover:bg-white/5 flex items-center gap-2"
                      >
                        <span class="w-2 h-2 rounded-full bg-amber-500" />
                        <span>{task.name}</span>
                        <Show when={task.children.length > 0}>
                          <span class="text-xs text-wool-600">{task.children.length} children</span>
                        </Show>
                      </button>
                    )}
                  </For>
                </div>
              </Show>
              <Show when={filteredResults().evals.length > 0}>
                <div class="py-1" style={{ 'border-top': '1px solid rgba(255,255,255,0.06)' }}>
                  <div class="px-3 py-1.5 text-[10px] font-semibold text-wool-600 uppercase tracking-wider">
                    Evals
                  </div>
                  <For each={filteredResults().evals}>
                    {(ev) => (
                      <button
                        onClick={() => {
                          setSelectedEvalId(ev.id);
                          setSelectedTaskId(null);
                          setShowCommandPalette(false);
                        }}
                        class="w-full px-3 py-2 text-left text-sm text-emerald-300 hover:bg-white/5 flex items-center gap-2"
                      >
                        <svg
                          class="w-3.5 h-3.5 text-emerald-500"
                          fill="none"
                          stroke="currentColor"
                          viewBox="0 0 24 24"
                        >
                          <path
                            stroke-linecap="round"
                            stroke-linejoin="round"
                            stroke-width="2"
                            d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z"
                          />
                        </svg>
                        <span>{ev.name}</span>
                      </button>
                    )}
                  </For>
                </div>
              </Show>
              <Show when={filteredResults().bookmarks.length > 0}>
                <div class="py-1" style={{ 'border-top': '1px solid rgba(255,255,255,0.06)' }}>
                  <div class="px-3 py-1.5 text-[10px] font-semibold text-wool-600 uppercase tracking-wider">
                    Bookmarks
                  </div>
                  <For each={filteredResults().bookmarks}>
                    {(bm) => (
                      <button
                        onClick={() => jumpToBookmark(bm)}
                        class="w-full px-3 py-2 text-left text-sm text-wool-300 hover:bg-white/5 flex items-center gap-2"
                      >
                        <svg
                          class="w-3.5 h-3.5 text-wool-600"
                          fill="none"
                          stroke="currentColor"
                          viewBox="0 0 24 24"
                        >
                          <path
                            stroke-linecap="round"
                            stroke-linejoin="round"
                            stroke-width="2"
                            d="M5 5a2 2 0 012-2h10a2 2 0 012 2v16l-7-3.5L5 21V5z"
                          />
                        </svg>
                        <span>{bm.name}</span>
                      </button>
                    )}
                  </For>
                </div>
              </Show>
              <Show
                when={
                  commandQuery() &&
                  filteredResults().tasks.length === 0 &&
                  filteredResults().evals.length === 0 &&
                  filteredResults().bookmarks.length === 0
                }
              >
                <div class="py-8 text-center text-wool-600 text-sm">No results found</div>
              </Show>
            </div>
          </div>
        </div>
      </Show>

      {/* Gyp Chat Drawer */}
      <GypChatDrawer
        projectId={project.selectedProjectId()!}
        focusNodeId={gypFocusTaskId()}
        focusNodeName={gypFocusTaskName()}
        messages={boardChat.messages}
        currentMessage={boardChat.currentMessage}
        connected={boardChat.connected}
        connecting={boardChat.connecting}
        gypEditing={boardChat.gypEditing}
        onSend={boardChat.sendMessage}
        onConnect={boardChat.connect}
        onFocusNode={(nodeId, nodeName) => {
          setGypFocusTaskId(nodeId);
          setGypFocusTaskName(nodeName);
        }}
      />

      {/* Shimmer CSS */}
      <style>{`
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

      {/* Empty state */}
      <Show when={!loading() && taskTree().length === 0 && evals().length === 0}>
        <div
          class="absolute inset-0 flex items-center justify-center pointer-events-none"
          style={{ 'z-index': 5 }}
        >
          <div class="text-center">
            <div
              class="w-20 h-20 mx-auto mb-4 rounded-2xl flex items-center justify-center"
              style={{
                background:
                  'linear-gradient(145deg, rgba(212,165,116,0.1) 0%, rgba(212,165,116,0.05) 100%)',
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
            <p class="text-sm text-wool-600">
              Right-click to add a task or eval, or press{' '}
              <kbd class="px-1.5 py-0.5 rounded bg-white/5 text-wool-400 text-xs">Cmd+K</kbd>
            </p>
          </div>
        </div>
      </Show>

      {/* Loading */}
      <Show when={loading()}>
        <div
          class="absolute inset-0 flex items-center justify-center"
          style={{ 'z-index': 100, background: 'rgba(15,15,15,0.9)' }}
        >
          <div class="text-center">
            <div class="spinner w-8 h-8 mb-3 mx-auto" />
            <p class="text-sm text-wool-500">Loading board...</p>
          </div>
        </div>
      </Show>
    </div>
  );
};
