/**
 * Tasks tab Alpine component (full task list view)
 */

import type { Task } from '../../types';

declare const Alpine: {
  store: (name: string) => { selectedRun?: string | null } | undefined;
};

/**
 * Tasks tab component
 */
export function tasksTab() {
  return {
    tasks: [] as Task[],
    filter: 'all' as 'all' | 'todo' | 'doing' | 'done',
    selectedTaskId: null as string | null,
    pollInterval: null as ReturnType<typeof setInterval> | null,

    get filteredTasks(): Task[] {
      if (this.filter === 'all') return this.tasks;
      return this.tasks.filter(t => t.status === this.filter);
    },

    async init() {
      window.addEventListener('run-selected', async (e: Event) => {
        const customEvent = e as CustomEvent<string | null>;
        if (customEvent.detail) {
          await this.loadTasks(customEvent.detail);
        } else {
          this.tasks = [];
        }
      });

      const runName = this.getSelectedRun();
      if (runName) {
        await this.loadTasks(runName);
      }
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

    async loadTasks(runName: string) {
      if (!runName) return;

      if (this.pollInterval) {
        clearInterval(this.pollInterval);
      }

      try {
        if (window.tauriInvoke) {
          this.tasks = await window.tauriInvoke<Task[]>('get_tasks', { runName });
        }

        this.pollInterval = setInterval(async () => {
          if (window.tauriInvoke) {
            const currentRun = this.getSelectedRun();
            if (currentRun) {
              this.tasks = await window.tauriInvoke<Task[]>('get_tasks', { runName: currentRun });
            }
          }
        }, 3000);
      } catch (e) {
        console.error('Failed to load tasks:', e);
      }
    },

    selectTask(task: Task) {
      this.selectedTaskId = this.selectedTaskId === task.id ? null : task.id;
    },

    formatTokens(tokens: number | null | undefined): string {
      if (!tokens) return '0';
      if (tokens >= 1000000) return (tokens / 1000000).toFixed(1) + 'M';
      if (tokens >= 1000) return (tokens / 1000).toFixed(1) + 'k';
      return String(tokens);
    },
  };
}
