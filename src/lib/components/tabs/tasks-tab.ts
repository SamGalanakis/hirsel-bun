/**
 * Tasks tab Alpine component - Enhanced task list view with nesting and details
 */

import type { Task, TaskDisplay } from '../../types';
import { formatTokens, formatRelativeTime, formatFullDateTime } from '../../utils/formatters';

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
  filter: 'all' | 'todo' | 'doing' | 'done' | 'blocked';
  selectedTaskId: string | null;
  selectedTask: TaskDisplay | null;
  showTaskPopover: boolean;
  popoverPosition: { x: number; y: number };
  collapsedTasks: Set<string>;
  pollInterval: ReturnType<typeof setInterval> | null;
  searchQuery: string;
  currentRunName: string | null;
}

/**
 * Tasks tab component
 */
export function tasksTab(): TasksTabData & Record<string, unknown> {
  return {
    tasks: [],
    taskTree: [],
    flatTasks: [],
    filter: 'all',
    selectedTaskId: null,
    selectedTask: null,
    showTaskPopover: false,
    popoverPosition: { x: 0, y: 0 },
    collapsedTasks: new Set<string>(),
    pollInterval: null,
    searchQuery: '',
    currentRunName: null,

    // Computed: filtered tasks based on filter and search
    get filteredTasks(): TaskDisplay[] {
      let result = this.flatTasks;

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
          t.description.toLowerCase().includes(query) ||
          (t.claimedBy && t.claimedBy.toLowerCase().includes(query))
        );
      }

      return result;
    },

    // Computed: stats
    get stats() {
      const total = this.tasks.length;
      const done = this.tasks.filter(t => t.status === 'done').length;
      const inProgress = this.tasks.filter(t => t.status === 'doing').length;
      const blocked = this.flatTasks.filter(t => t.isBlocked).length;
      const todo = this.tasks.filter(t => t.status === 'todo').length;
      return { total, done, inProgress, blocked, todo, percentDone: total > 0 ? Math.round((done / total) * 100) : 0 };
    },

    async init() {
      // Listen for run selection changes
      window.addEventListener('run-selected', async (e: Event) => {
        const customEvent = e as CustomEvent<string | null>;
        if (customEvent.detail) {
          await this.loadTasks(customEvent.detail);
        } else {
          this.clearTasks();
        }
      });
    },

    destroy() {
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
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
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }
      this.tasks = [];
      this.taskTree = [];
      this.flatTasks = [];
      this.selectedTaskId = null;
      this.selectedTask = null;
      this.showTaskPopover = false;
      this.currentRunName = null;
    },

    async loadTasks(runName: string) {
      if (!runName) return;

      // If already polling for this run, just do a refresh
      if (this.currentRunName === runName && this.pollInterval) {
        try {
          if (window.tauriInvoke) {
            this.tasks = await window.tauriInvoke<Task[]>('get_tasks', { runName });
            this.buildTaskTree();
          }
        } catch (e) {
          console.error('Failed to refresh tasks:', e);
        }
        return;
      }

      // Stop existing polling
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }

      // Update current run
      this.currentRunName = runName;

      try {
        if (window.tauriInvoke) {
          this.tasks = await window.tauriInvoke<Task[]>('get_tasks', { runName });
          this.buildTaskTree();
        }

        // Start polling
        this.pollInterval = setInterval(async () => {
          if (window.tauriInvoke && this.currentRunName) {
            try {
              const prevSelected = this.selectedTaskId;
              this.tasks = await window.tauriInvoke<Task[]>('get_tasks', { runName: this.currentRunName });
              this.buildTaskTree();
              // Update selected task if still exists
              if (prevSelected && this.selectedTask) {
                const updated = this.flatTasks.find(t => t.id === prevSelected);
                if (updated) {
                  this.selectedTask = updated;
                }
              }
            } catch (e) {
              console.error('Failed to poll tasks:', e);
            }
          }
        }, 2000); // Poll every 2 seconds for more responsive updates
      } catch (e) {
        console.error('Failed to load tasks:', e);
      }
    },

    buildTaskTree() {
      const taskMap = new Map<string, TaskDisplay>();

      // First pass: create TaskDisplay objects
      this.tasks.forEach(t => {
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

    hasChildren(task: TaskDisplay): boolean {
      // Check original task data for children
      return this.tasks.some(t => t.parentId === task.id);
    },

    selectTask(task: TaskDisplay, event: MouseEvent) {
      event.stopPropagation();

      if (this.selectedTaskId === task.id && this.showTaskPopover) {
        // Clicking same task again closes popover
        this.closeTaskPopover();
        return;
      }

      this.selectedTaskId = task.id;
      this.selectedTask = task;

      // Position popover near click, but ensure it stays in viewport
      const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
      this.popoverPosition = {
        x: Math.min(rect.right + 8, window.innerWidth - 350),
        y: Math.max(rect.top, 60),
      };
      this.showTaskPopover = true;
    },

    closeTaskPopover() {
      this.showTaskPopover = false;
    },

    // Get the blocker tasks for a task
    getBlockers(task: TaskDisplay): TaskDisplay[] {
      if (!task.blockedBy || task.blockedBy.length === 0) return [];
      return task.blockedBy
        .map(id => this.flatTasks.find(t => t.id === id))
        .filter((t): t is TaskDisplay => t !== undefined && t.status !== 'done');
    },

    // Get the parent task
    getParentTask(task: TaskDisplay): TaskDisplay | null {
      if (!task.parentId) return null;
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

    getStatusLabel(status: string): string {
      switch (status) {
        case 'todo': return 'To Do';
        case 'doing': return 'In Progress';
        case 'done': return 'Done';
        default: return status;
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
          this.closeTaskPopover();
          await this.loadTasks(runName);
        }
      } catch (err) {
        console.error('Failed to delete task:', err);
        window.toast?.error('Failed to delete task');
      }
    },
  };
}
