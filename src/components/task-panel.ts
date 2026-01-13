/**
 * Task Panel Alpine.js Component
 *
 * Displays the task list with:
 * - Hierarchical task structure with indentation
 * - Status icons (todo, doing, done, blocked)
 * - Claimed-by worker info
 * - Right-click context menu for task actions
 * - Real-time updates via polling
 */

import {
  getTasks,
  addTask,
  deleteTask,
  completeTask,
  reopenTask,
  unclaimTask,
} from '../lib/api';
import type { Task, TaskStatus, TaskDisplay } from '../lib/types';

/**
 * Status icon mapping
 */
const STATUS_ICONS: Record<TaskStatus | 'blocked', string> = {
  todo: '\u25cb',    // ○
  doing: '\u25cf',   // ●
  done: '\u2713',    // ✓
  blocked: '\u29d7', // ⧗
};

/**
 * Status color classes
 */
const STATUS_COLORS: Record<TaskStatus | 'blocked', string> = {
  todo: 'text-wool-500',
  doing: 'text-amber-500',
  done: 'text-sage',
  blocked: 'text-wool-600',
};

/**
 * Build task hierarchy from flat task list
 */
function buildTaskHierarchy(tasks: Task[]): TaskDisplay[] {
  const taskMap = new Map<string, TaskDisplay>();
  const roots: TaskDisplay[] = [];

  // First pass: create TaskDisplay objects
  for (const task of tasks) {
    const isBlocked = task.blockedBy && task.blockedBy.length > 0 &&
      task.blockedBy.some(id => {
        const blocker = tasks.find(t => t.id === id);
        return blocker && blocker.status !== 'done';
      });

    taskMap.set(task.id, {
      ...task,
      isBlocked: !!isBlocked,
      children: [],
      depth: 0,
    });
  }

  // Second pass: build hierarchy
  for (const task of tasks) {
    const display = taskMap.get(task.id)!;
    if (task.parentId && taskMap.has(task.parentId)) {
      const parent = taskMap.get(task.parentId)!;
      parent.children.push(display);
    } else {
      roots.push(display);
    }
  }

  // Third pass: calculate depths and flatten for display
  function setDepths(tasks: TaskDisplay[], depth: number): void {
    for (const task of tasks) {
      task.depth = depth;
      setDepths(task.children, depth + 1);
    }
  }
  setDepths(roots, 0);

  // Flatten hierarchy for display (parents followed by children)
  function flatten(tasks: TaskDisplay[]): TaskDisplay[] {
    const result: TaskDisplay[] = [];
    for (const task of tasks) {
      result.push(task);
      result.push(...flatten(task.children));
    }
    return result;
  }

  return flatten(roots);
}

/**
 * Task panel component data
 */
export interface TaskPanelData {
  runName: string | null;
  tasks: TaskDisplay[];
  loading: boolean;
  error: string | null;
  selectedTaskId: string | null;
  contextMenuVisible: boolean;
  contextMenuX: number;
  contextMenuY: number;
  contextMenuTaskId: string | null;
  pollInterval: ReturnType<typeof setInterval> | null;
  showAddDialog: boolean;
  newTaskId: string;
  newTaskDescription: string;
  newTaskParentId: string;
}

/**
 * Alpine.js component factory for task panel
 */
export function taskPanel(): TaskPanelData & {
  init(): void;
  destroy(): void;
  loadTasks(runName: string): Promise<void>;
  fetchTasks(): Promise<void>;
  clearTasks(): void;
  selectTask(taskId: string): void;
  getStatusIcon(task: TaskDisplay): string;
  getStatusColor(task: TaskDisplay): string;
  getTaskStyle(task: TaskDisplay): string;
  showContextMenu(event: MouseEvent, taskId: string): void;
  hideContextMenu(): void;
  contextComplete(): Promise<void>;
  contextReopen(): Promise<void>;
  contextUnclaim(): Promise<void>;
  contextDelete(): Promise<void>;
  openAddDialog(parentId?: string): void;
  closeAddDialog(): void;
  submitAddTask(): Promise<void>;
  getTaskById(taskId: string): TaskDisplay | undefined;
  isTaskActionable(task: TaskDisplay): boolean;
} {
  return {
    runName: null,
    tasks: [],
    loading: false,
    error: null,
    selectedTaskId: null,
    contextMenuVisible: false,
    contextMenuX: 0,
    contextMenuY: 0,
    contextMenuTaskId: null,
    pollInterval: null,
    showAddDialog: false,
    newTaskId: '',
    newTaskDescription: '',
    newTaskParentId: '',

    /**
     * Initialize the component
     */
    init(): void {
      // Listen for run selection events
      window.addEventListener('run-selected', ((e: CustomEvent<string | null>) => {
        if (e.detail) {
          this.loadTasks(e.detail);
        } else {
          this.clearTasks();
        }
      }) as EventListener);

      // Hide context menu on click away
      document.addEventListener('click', () => {
        this.hideContextMenu();
      });
    },

    /**
     * Clean up when component is destroyed
     */
    destroy(): void {
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }
    },

    /**
     * Load tasks for a run
     */
    async loadTasks(runName: string): Promise<void> {
      // Stop existing polling
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }

      this.runName = runName;
      this.loading = true;
      this.error = null;

      await this.fetchTasks();

      // Start polling for updates every 2 seconds
      this.pollInterval = setInterval(() => {
        this.fetchTasks();
      }, 2000);
    },

    /**
     * Fetch tasks from backend
     */
    async fetchTasks(): Promise<void> {
      if (!this.runName) return;

      try {
        const rawTasks = await getTasks(this.runName);
        this.tasks = buildTaskHierarchy(rawTasks);
        this.loading = false;
        this.error = null;
      } catch (err) {
        this.error = err instanceof Error ? err.message : String(err);
        this.loading = false;
      }
    },

    /**
     * Clear tasks when no run is selected
     */
    clearTasks(): void {
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }
      this.runName = null;
      this.tasks = [];
      this.loading = false;
      this.error = null;
      this.selectedTaskId = null;
      this.hideContextMenu();
    },

    /**
     * Select a task
     */
    selectTask(taskId: string): void {
      this.selectedTaskId = taskId;
      window.dispatchEvent(new CustomEvent('task-selected', { detail: taskId }));
    },

    /**
     * Get status icon for a task
     */
    getStatusIcon(task: TaskDisplay): string {
      if (task.isBlocked && task.status === 'todo') {
        return STATUS_ICONS.blocked;
      }
      return STATUS_ICONS[task.status];
    },

    /**
     * Get status color class for a task
     */
    getStatusColor(task: TaskDisplay): string {
      if (task.isBlocked && task.status === 'todo') {
        return STATUS_COLORS.blocked;
      }
      return STATUS_COLORS[task.status];
    },

    /**
     * Get inline style for task indentation
     */
    getTaskStyle(task: TaskDisplay): string {
      return `padding-left: ${task.depth * 16 + 12}px`;
    },

    /**
     * Show context menu for a task
     */
    showContextMenu(event: MouseEvent, taskId: string): void {
      event.preventDefault();
      event.stopPropagation();
      this.contextMenuX = event.clientX;
      this.contextMenuY = event.clientY;
      this.contextMenuTaskId = taskId;
      this.contextMenuVisible = true;
    },

    /**
     * Hide context menu
     */
    hideContextMenu(): void {
      this.contextMenuVisible = false;
      this.contextMenuTaskId = null;
    },

    /**
     * Mark task as complete via context menu
     */
    async contextComplete(): Promise<void> {
      if (!this.runName || !this.contextMenuTaskId) return;
      try {
        await completeTask(this.runName, this.contextMenuTaskId);
        await this.fetchTasks();
      } catch (err) {
        console.error('Failed to complete task:', err);
        alert(`Failed to complete task: ${err instanceof Error ? err.message : err}`);
      }
      this.hideContextMenu();
    },

    /**
     * Reopen task via context menu
     */
    async contextReopen(): Promise<void> {
      if (!this.runName || !this.contextMenuTaskId) return;
      try {
        await reopenTask(this.runName, this.contextMenuTaskId);
        await this.fetchTasks();
      } catch (err) {
        console.error('Failed to reopen task:', err);
        alert(`Failed to reopen task: ${err instanceof Error ? err.message : err}`);
      }
      this.hideContextMenu();
    },

    /**
     * Unclaim task via context menu
     */
    async contextUnclaim(): Promise<void> {
      if (!this.runName || !this.contextMenuTaskId) return;
      try {
        await unclaimTask(this.runName, this.contextMenuTaskId);
        await this.fetchTasks();
      } catch (err) {
        console.error('Failed to unclaim task:', err);
        alert(`Failed to unclaim task: ${err instanceof Error ? err.message : err}`);
      }
      this.hideContextMenu();
    },

    /**
     * Delete task via context menu
     */
    async contextDelete(): Promise<void> {
      if (!this.runName || !this.contextMenuTaskId) return;
      if (!confirm(`Delete task "${this.contextMenuTaskId}"?`)) {
        this.hideContextMenu();
        return;
      }
      try {
        await deleteTask(this.runName, this.contextMenuTaskId);
        await this.fetchTasks();
      } catch (err) {
        console.error('Failed to delete task:', err);
        alert(`Failed to delete task: ${err instanceof Error ? err.message : err}`);
      }
      this.hideContextMenu();
    },

    /**
     * Open add task dialog
     */
    openAddDialog(parentId?: string): void {
      this.newTaskId = '';
      this.newTaskDescription = '';
      this.newTaskParentId = parentId || '';
      this.showAddDialog = true;
      this.hideContextMenu();
    },

    /**
     * Close add task dialog
     */
    closeAddDialog(): void {
      this.showAddDialog = false;
      this.newTaskId = '';
      this.newTaskDescription = '';
      this.newTaskParentId = '';
    },

    /**
     * Submit new task
     */
    async submitAddTask(): Promise<void> {
      if (!this.runName || !this.newTaskId.trim() || !this.newTaskDescription.trim()) {
        return;
      }

      try {
        await addTask(
          this.runName,
          this.newTaskId.trim(),
          this.newTaskDescription.trim(),
          this.newTaskParentId ? { parentId: this.newTaskParentId } : undefined
        );
        await this.fetchTasks();
        this.closeAddDialog();
      } catch (err) {
        console.error('Failed to add task:', err);
        alert(`Failed to add task: ${err instanceof Error ? err.message : err}`);
      }
    },

    /**
     * Get task by ID
     */
    getTaskById(taskId: string): TaskDisplay | undefined {
      return this.tasks.find(t => t.id === taskId);
    },

    /**
     * Check if task can have actions performed on it
     */
    isTaskActionable(task: TaskDisplay): boolean {
      return task.status !== 'done' || task.claimedBy !== null;
    },
  };
}

/**
 * Register the component with Alpine.js
 */
export function registerTaskPanelComponent(): void {
  if (typeof window !== 'undefined') {
    (window as unknown as Record<string, unknown>).taskPanel = taskPanel;
  }
}

// Auto-register if Alpine is already loaded
if (typeof window !== 'undefined' && typeof Alpine !== 'undefined') {
  registerTaskPanelComponent();
}

// Declare Alpine global for TypeScript
declare const Alpine: {
  store: (name: string) => Record<string, unknown> | undefined;
};
