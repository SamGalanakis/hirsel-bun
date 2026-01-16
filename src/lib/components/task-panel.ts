/**
 * Task panel Alpine component (for overview tab)
 */

import type { Task, TaskDisplay } from '../types';

/**
 * Task panel component
 */
export function taskPanel() {
  return {
    tasks: [] as Task[],
    flatTasks: [] as TaskDisplay[],
    loading: false,
    error: null as string | null,
    pollInterval: null as ReturnType<typeof setInterval> | null,
    selectedTaskId: null as string | null,
    contextMenuVisible: false,
    contextMenuX: 0,
    contextMenuY: 0,
    contextMenuTask: null as TaskDisplay | null,
    selectedTask: null as TaskDisplay | null,
    showTaskDetail: false,

    async init() {
      window.addEventListener('run-selected', async (e: Event) => {
        const customEvent = e as CustomEvent<string | null>;
        if (customEvent.detail) {
          await this.loadTasks(customEvent.detail);
        } else {
          this.clearTasks();
        }
      });

      const app = this.getAppState();
      if (app && app.selectedRun) {
        await this.loadTasks(app.selectedRun);
      }
    },

    destroy() {
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }
    },

    getAppState(): { selectedRun?: string | null; tasksTotal?: number; tasksDone?: number } | null {
      // @ts-expect-error Alpine.js $el magic property
      let el = this.$el as HTMLElement;
      while (el && el.parentElement) {
        el = el.parentElement;
        // @ts-expect-error Alpine.js internal property
        if (el._x_dataStack) {
          // @ts-expect-error Alpine.js internal property
          return el._x_dataStack[0];
        }
      }
      return null;
    },

    getSelectedRun(): string | null {
      const app = this.getAppState();
      return app ? app.selectedRun || null : null;
    },

    async loadTasks(runName: string) {
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }

      this.loading = true;
      this.error = null;

      try {
        if (window.tauriInvoke) {
          this.tasks = await window.tauriInvoke<Task[]>('get_tasks', { runName });
        } else {
          this.tasks = [];
        }
        this.buildFlatList();
        this.loading = false;
        this.updateTaskCounts();

        this.pollInterval = setInterval(async () => {
          const currentRun = this.getSelectedRun();
          if (!currentRun) return;
          try {
            if (window.tauriInvoke) {
              this.tasks = await window.tauriInvoke<Task[]>('get_tasks', { runName: currentRun });
              this.buildFlatList();
              this.updateTaskCounts();
            }
          } catch (err) {
            console.error('Task poll error:', err);
          }
        }, 2000);
      } catch (err) {
        const error = err as Error;
        this.error = error.message || String(error);
        this.loading = false;
      }
    },

    clearTasks() {
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }
      this.tasks = [];
      this.flatTasks = [];
      this.selectedTaskId = null;
      this.loading = false;
      this.error = null;
    },

    buildFlatList() {
      const taskMap = new Map<string, TaskDisplay>();
      const rootTasks: TaskDisplay[] = [];

      this.tasks.forEach(t => {
        taskMap.set(t.id, { ...t, children: [], depth: 0, isBlocked: false });
      });

      this.tasks.forEach(t => {
        const task = taskMap.get(t.id)!;
        if (t.parentId && taskMap.has(t.parentId)) {
          taskMap.get(t.parentId)!.children.push(task);
        } else {
          rootTasks.push(task);
        }
      });

      const flat: TaskDisplay[] = [];
      const flatten = (tasks: TaskDisplay[], depth: number) => {
        tasks.forEach(t => {
          t.depth = depth;
          t.isBlocked =
            !!t.blockedBy &&
            t.blockedBy.length > 0 &&
            t.blockedBy.some(bid => {
              const blocker = taskMap.get(bid);
              return blocker && blocker.status !== 'done';
            });
          flat.push(t);
          if (t.children && t.children.length > 0) {
            flatten(t.children, depth + 1);
          }
        });
      };
      flatten(rootTasks, 0);
      this.flatTasks = flat;
    },

    updateTaskCounts() {
      const app = this.getAppState();
      if (app) {
        app.tasksTotal = this.tasks.length;
        app.tasksDone = this.tasks.filter(t => t.status === 'done').length;
      }
    },

    getStatusIcon(status: string): string {
      return status === 'done' ? '✓' : status === 'doing' ? '●' : '○';
    },

    getStatusClass(status: string): string {
      if (status === 'done') return 'text-sage';
      if (status === 'doing') return 'text-amber-500';
      return 'text-wool-500';
    },

    getTaskNameClass(task: TaskDisplay): string {
      const classes = ['text-sm', 'truncate'];
      if (task.status === 'done') {
        classes.push('text-wool-500', 'line-through');
      } else if (task.isBlocked) {
        classes.push('text-wool-500', 'italic');
      } else {
        classes.push('text-wool-100');
      }
      return classes.join(' ');
    },

    selectTask(taskId: string) {
      this.selectedTaskId = taskId;
      window.dispatchEvent(new CustomEvent('task-selected', { detail: taskId }));
      const task = this.flatTasks.find(t => t.id === taskId);
      if (task) {
        this.openTaskDetail(task);
      }
    },

    openTaskDetail(task: TaskDisplay) {
      this.selectedTask = task;
      this.showTaskDetail = true;
    },

    closeTaskDetail() {
      this.showTaskDetail = false;
      this.selectedTask = null;
    },

    showContextMenu(event: MouseEvent, task: TaskDisplay) {
      event.preventDefault();
      event.stopPropagation();
      this.contextMenuX = event.clientX;
      this.contextMenuY = event.clientY;
      this.contextMenuTask = task;
      this.contextMenuVisible = true;
    },

    hideContextMenu() {
      this.contextMenuVisible = false;
      this.contextMenuTask = null;
    },

    canMarkDone(task: TaskDisplay | null): boolean {
      return !!task && task.status !== 'done';
    },

    canReopen(task: TaskDisplay | null): boolean {
      return !!task && task.status === 'done';
    },

    canUnclaim(task: TaskDisplay | null): boolean {
      return !!task && task.status === 'doing' && !!task.claimedBy;
    },

    formatTaskTime(timestamp: string | null | undefined): string {
      if (!timestamp) return '';
      const date = new Date(timestamp);
      return date.toLocaleString('en-GB', {
        month: 'short',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
        hour12: false,
      });
    },

    formatTaskTokens(n: number | null | undefined): string {
      if (n == null || n === 0) return '0';
      if (n < 1000) return String(n);
      if (n < 1000000) return (n / 1000).toFixed(1).replace(/\.0$/, '') + 'k';
      return (n / 1000000).toFixed(1).replace(/\.0$/, '') + 'M';
    },

    async markDone(taskId: string | undefined) {
      if (!taskId) return;
      const runName = this.getSelectedRun();
      if (!runName) return;

      try {
        if (window.tauriInvoke) {
          await window.tauriInvoke('complete_task', { runName, taskId });
          await this.loadTasks(runName);
        }
      } catch (err) {
        const error = err as Error;
        console.error('Failed to complete task:', error);
        window.toast?.error('Failed to complete task');
      }
    },

    async reopenTask(taskId: string | undefined) {
      if (!taskId) return;
      const runName = this.getSelectedRun();
      if (!runName) return;

      try {
        if (window.tauriInvoke) {
          await window.tauriInvoke('reopen_task', { runName, taskId });
          await this.loadTasks(runName);
        }
      } catch (err) {
        const error = err as Error;
        console.error('Failed to reopen task:', error);
        window.toast?.error('Failed to reopen task');
      }
    },

    async contextMarkDone() {
      if (!this.contextMenuTask || !this.canMarkDone(this.contextMenuTask)) {
        this.hideContextMenu();
        return;
      }
      await this.markDone(this.contextMenuTask.id);
      this.hideContextMenu();
    },

    async contextReopen() {
      if (!this.contextMenuTask || !this.canReopen(this.contextMenuTask)) {
        this.hideContextMenu();
        return;
      }
      await this.reopenTask(this.contextMenuTask.id);
      this.hideContextMenu();
    },

    async contextUnclaim() {
      if (!this.contextMenuTask || !this.canUnclaim(this.contextMenuTask)) {
        this.hideContextMenu();
        return;
      }
      const runName = this.getSelectedRun();
      if (!runName) return;

      try {
        if (window.tauriInvoke) {
          await window.tauriInvoke('unclaim_task', { runName, taskId: this.contextMenuTask.id });
          await this.loadTasks(runName);
        }
      } catch (err) {
        const error = err as Error;
        console.error('Failed to unclaim task:', error);
        window.toast?.error('Failed to unclaim task');
      }
      this.hideContextMenu();
    },

    async contextDelete() {
      if (!this.contextMenuTask) {
        this.hideContextMenu();
        return;
      }
      const runName = this.getSelectedRun();
      if (!runName) return;

      const taskToDelete = this.contextMenuTask.id;
      this.hideContextMenu();

      const confirmed = await window.confirmDialog?.delete(taskToDelete, 'task')
        ?? confirm(`Delete task "${taskToDelete}"?`);
      if (!confirmed) return;

      try {
        if (window.tauriInvoke) {
          await window.tauriInvoke('delete_task', { runName, taskId: taskToDelete });
          await this.loadTasks(runName);
        }
      } catch (err) {
        const error = err as Error;
        console.error('Failed to delete task:', error);
        window.toast?.error('Failed to delete task');
      }
    },
  };
}
