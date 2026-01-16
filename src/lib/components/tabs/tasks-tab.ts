/**
 * Tasks tab Alpine component - Enhanced task list view with nesting and details
 */

import type { Task, TaskDisplay, WorkerDisplay } from '../../types';
import { formatTokens, formatRelativeTime, formatFullDateTime, formatDuration } from '../../utils/formatters';
import { dataCache, DATA_EVENTS } from '../../data-cache';
import { generateSheepSvg } from '../../sheep-avatar';

declare const Alpine: {
  store: (name: string) => { selectedRun?: string | null } | undefined;
};

declare const window: Window & {
  tauriInvoke?: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  toast?: {
    success: (message: string, title?: string) => void;
    error: (message: string, title?: string) => void;
    info: (message: string, title?: string) => void;
  };
  confirmDialog?: {
    show: (options: { title: string; message: string; confirmText?: string; cancelText?: string; danger?: boolean }) => Promise<boolean>;
    delete: (itemName: string, itemType?: string) => Promise<boolean>;
  };
};

export interface TasksTabData {
  tasks: Task[];
  taskTree: TaskDisplay[];
  flatTasks: TaskDisplay[];
  workers: WorkerDisplay[];
  filter: 'all' | 'todo' | 'doing' | 'done' | 'blocked';
  selectedTaskId: string | null;
  selectedTask: TaskDisplay | null;
  showTaskModal: boolean;
  collapsedTasks: Set<string>;
  searchQuery: string;
  currentRunName: string | null;
  _eventCleanups: (() => void)[];
  _cacheUnsubscribe: (() => void) | null;
  _parentIds: Set<string>; // Set of task IDs that have children (for O(1) hasChildren lookup)
}

/**
 * Tasks tab component
 */
export function tasksTab(): TasksTabData & Record<string, unknown> {
  return {
    tasks: [],
    taskTree: [],
    flatTasks: [],
    workers: [] as WorkerDisplay[],
    filter: 'all',
    selectedTaskId: null,
    selectedTask: null,
    showTaskModal: false,
    collapsedTasks: new Set<string>(),
    searchQuery: '',
    currentRunName: null,
    _eventCleanups: [],
    _cacheUnsubscribe: null,
    _parentIds: new Set<string>(),

    // Computed: filtered tasks based on filter and search
    get filteredTasks(): TaskDisplay[] {
      let result = this.flatTasks.filter(t => t != null);

      // Apply status filter
      if (this.filter === 'blocked') {
        result = result.filter(t => t.isBlocked);
      } else if (this.filter !== 'all') {
        result = result.filter(t => t.status === this.filter);
      }

      // Apply search filter
      if (this.searchQuery.trim()) {
        const query = this.searchQuery.toLowerCase();
        result = result.filter(t =>
          t.description?.toLowerCase().includes(query) ||
          (t.claimedBy && t.claimedBy.toLowerCase().includes(query))
        );
      }

      return result;
    },

    // Computed: stats
    get stats() {
      const validTasks = this.tasks.filter(t => t != null);
      const total = validTasks.length;
      const done = validTasks.filter(t => t.status === 'done').length;
      const inProgress = validTasks.filter(t => t.status === 'doing').length;
      const blocked = this.flatTasks.filter(t => t && t.isBlocked).length;
      const todo = validTasks.filter(t => t.status === 'todo').length;
      return { total, done, inProgress, blocked, todo, percentDone: total > 0 ? Math.round((done / total) * 100) : 0 };
    },

    async init() {
      // Subscribe to shared cache
      this._cacheUnsubscribe = dataCache.subscribe();

      // Listen for tasks updates from cache
      const tasksUpdatedHandler = (e: Event) => {
        const customEvent = e as CustomEvent<Task[]>;
        const prevSelected = this.selectedTaskId;
        // Filter out any null/undefined tasks
        this.tasks = (customEvent.detail || []).filter((t): t is Task => t != null);
        this.buildTaskTree();
        // Update selected task if still exists
        if (prevSelected && this.selectedTask) {
          const updated = this.flatTasks.find(t => t.id === prevSelected);
          if (updated) {
            this.selectedTask = updated;
          }
        }
      };
      window.addEventListener(DATA_EVENTS.TASKS_UPDATED, tasksUpdatedHandler);
      this._eventCleanups.push(() => window.removeEventListener(DATA_EVENTS.TASKS_UPDATED, tasksUpdatedHandler));

      // Listen for workers updates from cache
      const workersUpdatedHandler = (e: Event) => {
        const customEvent = e as CustomEvent<WorkerDisplay[]>;
        this.workers = (customEvent.detail || []).filter((w): w is WorkerDisplay => w != null);
      };
      window.addEventListener(DATA_EVENTS.WORKERS_UPDATED, workersUpdatedHandler);
      this._eventCleanups.push(() => window.removeEventListener(DATA_EVENTS.WORKERS_UPDATED, workersUpdatedHandler));

      // Listen for run selection changes
      const runSelectedHandler = (e: Event) => {
        const customEvent = e as CustomEvent<string | null>;
        if (customEvent.detail) {
          this.currentRunName = customEvent.detail;
          // Get initial data from cache
          const cachedTasks = dataCache.getTasks().filter((t): t is Task => t != null);
          if (cachedTasks.length > 0) {
            this.tasks = cachedTasks;
            this.buildTaskTree();
          }
          this.workers = dataCache.getWorkers().filter((w): w is WorkerDisplay => w != null);
        } else {
          this.clearTasks();
        }
      };
      window.addEventListener('run-selected', runSelectedHandler);
      this._eventCleanups.push(() => window.removeEventListener('run-selected', runSelectedHandler));

      // Get initial data from cache if a run is already selected
      const selectedRun = dataCache.getSelectedRun();
      if (selectedRun) {
        this.currentRunName = selectedRun;
        const cachedTasks = dataCache.getTasks().filter((t): t is Task => t != null);
        if (cachedTasks.length > 0) {
          this.tasks = cachedTasks;
          this.buildTaskTree();
        }
        this.workers = dataCache.getWorkers().filter((w): w is WorkerDisplay => w != null);
      }
    },

    destroy() {
      this._eventCleanups.forEach(fn => fn());
      this._eventCleanups = [];
      if (this._cacheUnsubscribe) {
        this._cacheUnsubscribe();
        this._cacheUnsubscribe = null;
      }
    },

    getSelectedRun(): string | null {
      if (typeof Alpine !== 'undefined' && Alpine.store && Alpine.store('app')) {
        return Alpine.store('app')?.selectedRun || null;
      }
      // @ts-expect-error Alpine.js $root magic property
      return this.$root?.selectedRun || null;
    },

    clearTasks() {
      this.tasks = [];
      this.taskTree = [];
      this.flatTasks = [];
      this.workers = [];
      this.selectedTaskId = null;
      this.selectedTask = null;
      this.showTaskModal = false;
      this.currentRunName = null;
    },

    buildTaskTree() {
      const taskMap = new Map<string, TaskDisplay>();

      // Build parent IDs set for O(1) hasChildren lookup
      this._parentIds = new Set<string>();
      this.tasks.forEach(t => {
        if (t.parentId) {
          this._parentIds.add(t.parentId);
        }
      });

      // First pass: create TaskDisplay objects
      this.tasks.forEach(t => {
        if (!t) return; // Skip null entries
        taskMap.set(t.id, {
          ...t,
          children: [],
          depth: 0,
          isBlocked: false,
        });
      });

      // Second pass: determine blocked status and build parent-child relationships
      const rootTasks: TaskDisplay[] = [];

      this.tasks.forEach(t => {
        if (!t) return; // Skip null entries
        const task = taskMap.get(t.id);
        if (!task) return; // Shouldn't happen, but be defensive

        // Check if blocked
        task.isBlocked = !!t.blockedBy && t.blockedBy.length > 0 && t.blockedBy.some(bid => {
          const blocker = taskMap.get(bid);
          return blocker && blocker.status !== 'done';
        });

        // Build hierarchy
        if (t.parentId) {
          const parent = taskMap.get(t.parentId);
          if (parent) {
            parent.children.push(task);
          } else {
            rootTasks.push(task);
          }
        } else {
          rootTasks.push(task);
        }
      });

      // Sort root tasks: in progress first, then todo, then done
      const statusOrder = { doing: 0, todo: 1, done: 2 };
      rootTasks.sort((a, b) => {
        const orderA = statusOrder[a.status] ?? 1;
        const orderB = statusOrder[b.status] ?? 1;
        return orderA - orderB;
      });

      this.taskTree = rootTasks;

      // Flatten for easy filtering/display
      const flat: TaskDisplay[] = [];
      const flatten = (tasks: TaskDisplay[], depth: number) => {
        tasks.forEach(task => {
          task.depth = depth;
          flat.push(task);
          if (task.children.length > 0 && !this.collapsedTasks.has(task.id)) {
            // Sort children the same way
            task.children.sort((a, b) => {
              const orderA = statusOrder[a.status] ?? 1;
              const orderB = statusOrder[b.status] ?? 1;
              return orderA - orderB;
            });
            flatten(task.children, depth + 1);
          }
        });
      };
      flatten(rootTasks, 0);
      this.flatTasks = flat;
    },

    toggleCollapse(taskId: string) {
      if (this.collapsedTasks.has(taskId)) {
        this.collapsedTasks.delete(taskId);
      } else {
        this.collapsedTasks.add(taskId);
      }
      this.buildTaskTree();
    },

    isCollapsed(taskId: string): boolean {
      return this.collapsedTasks.has(taskId);
    },

    hasChildren(task: TaskDisplay | null): boolean {
      // O(1) lookup using pre-built parent IDs Set
      if (!task) return false;
      return this._parentIds.has(task.id);
    },

    selectTask(task: TaskDisplay, event: MouseEvent) {
      event.stopPropagation();

      if (this.selectedTaskId === task.id && this.showTaskModal) {
        // Clicking same task again closes modal
        this.closeTaskModal();
        return;
      }

      this.selectedTaskId = task.id;
      this.selectedTask = task;
      this.showTaskModal = true;
    },

    closeTaskModal() {
      this.showTaskModal = false;
    },

    // Get worker avatar SVG by worker name
    getWorkerAvatar(workerName: string, size = 32): string {
      const worker = this.workers.find(w => w.name === workerName);
      if (worker?.sheepConfig) {
        return generateSheepSvg(worker.sheepConfig, size, worker.status);
      }
      // Fallback - return empty string (will show default icon in HTML)
      return '';
    },

    // Get the blocker tasks for a task
    getBlockers(task: TaskDisplay | null): TaskDisplay[] {
      if (!task || !task.blockedBy || task.blockedBy.length === 0) return [];
      return task.blockedBy
        .map(id => this.flatTasks.find(t => t.id === id))
        .filter((t): t is TaskDisplay => t !== undefined && t.status !== 'done');
    },

    // Get the parent task
    getParentTask(task: TaskDisplay | null): TaskDisplay | null {
      if (!task || !task.parentId) return null;
      return this.flatTasks.find(t => t.id === task.parentId) || null;
    },

    // Navigate to a task
    navigateToTask(taskId: string) {
      const task = this.flatTasks.find(t => t.id === taskId);
      if (task) {
        this.selectedTaskId = taskId;
        this.selectedTask = task;
        // Scroll task into view
        setTimeout(() => {
          const el = document.querySelector(`[data-task-id="${taskId}"]`);
          el?.scrollIntoView({ behavior: 'smooth', block: 'center' });
        }, 50);
      }
    },

    // Use shared formatters
    formatTokens,
    formatTime: formatRelativeTime,
    formatFullTime: formatFullDateTime,
    formatDuration,

    getStatusLabel(status: string): string {
      switch (status) {
        case 'todo': return 'To Do';
        case 'doing': return 'In Progress';
        case 'done': return 'Done';
        default: return status;
      }
    },

    // Load tasks from backend and update cache
    async loadTasks(runName: string) {
      try {
        if (window.tauriInvoke) {
          const tasks = await window.tauriInvoke<Task[]>('get_tasks', { runName });
          this.tasks = (tasks || []).filter((t): t is Task => t != null);
          this.buildTaskTree();
        }
      } catch (err) {
        console.error('Failed to load tasks:', err);
      }
    },

    // Task actions
    async markTaskDone(task: TaskDisplay) {
      const runName = this.getSelectedRun();
      if (!runName || task.status === 'done') return;

      try {
        if (window.tauriInvoke) {
          await window.tauriInvoke('complete_task', { runName, taskId: task.id });
          await this.loadTasks(runName);
        }
      } catch (err) {
        console.error('Failed to complete task:', err);
        window.toast?.error('Failed to complete task');
      }
    },

    async reopenTask(task: TaskDisplay) {
      const runName = this.getSelectedRun();
      if (!runName || task.status !== 'done') return;

      try {
        if (window.tauriInvoke) {
          await window.tauriInvoke('reopen_task', { runName, taskId: task.id });
          await this.loadTasks(runName);
        }
      } catch (err) {
        console.error('Failed to reopen task:', err);
        window.toast?.error('Failed to reopen task');
      }
    },

    async unclaimTask(task: TaskDisplay) {
      const runName = this.getSelectedRun();
      if (!runName || !task.claimedBy) return;

      try {
        if (window.tauriInvoke) {
          await window.tauriInvoke('unclaim_task', { runName, taskId: task.id });
          await this.loadTasks(runName);
        }
      } catch (err) {
        console.error('Failed to unclaim task:', err);
        window.toast?.error('Failed to unclaim task');
      }
    },

    async deleteTask(task: TaskDisplay) {
      const runName = this.getSelectedRun();
      if (!runName) return;

      const confirmed = await window.confirmDialog?.delete(task.description.slice(0, 50), 'task')
        ?? confirm(`Delete task "${task.description.slice(0, 50)}..."?`);
      if (!confirmed) return;

      try {
        if (window.tauriInvoke) {
          await window.tauriInvoke('delete_task', { runName, taskId: task.id });
          this.closeTaskModal();
          await this.loadTasks(runName);
        }
      } catch (err) {
        console.error('Failed to delete task:', err);
        window.toast?.error('Failed to delete task');
      }
    },
  };
}
