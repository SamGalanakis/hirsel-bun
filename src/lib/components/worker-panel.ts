/**
 * Worker panel Alpine component
 */

import type { WorkerDisplay, Task } from '../types';

interface EnrichedWorker extends WorkerDisplay {
  isLeader: boolean;
  currentTask: string | null;
}

/**
 * Worker panel component
 */
export function workerPanel() {
  return {
    workers: [] as EnrichedWorker[],
    loading: false,
    error: null as string | null,
    selectedRun: null as string | null,
    _pollInterval: null as ReturnType<typeof setInterval> | null,
    selectedWorker: null as EnrichedWorker | null,
    showWorkerDetail: false,

    async init() {
      window.addEventListener('run-selected', (e: Event) => {
        const customEvent = e as CustomEvent<string | null>;
        this.onRunSelected(customEvent.detail);
      });

      const app = this.getAppState();
      if (app && app.selectedRun) {
        await this.onRunSelected(app.selectedRun);
      }
    },

    destroy() {
      if (this._pollInterval) {
        clearInterval(this._pollInterval);
        this._pollInterval = null;
      }
    },

    getAppState(): { selectedRun?: string | null } | null {
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

    async onRunSelected(runName: string | null) {
      if (this._pollInterval) {
        clearInterval(this._pollInterval);
        this._pollInterval = null;
      }

      this.selectedRun = runName;

      if (!runName) {
        this.workers = [];
        this.loading = false;
        this.error = null;
        return;
      }

      await this.fetchWorkers();

      this._pollInterval = setInterval(() => {
        this.fetchWorkers();
      }, 2000);
    },

    async fetchWorkers() {
      if (!this.selectedRun) return;

      if (this.workers.length === 0) {
        this.loading = true;
      }

      try {
        if (!window.tauriInvoke) {
          this.workers = [];
          this.loading = false;
          return;
        }

        const workers = await window.tauriInvoke<WorkerDisplay[]>('get_workers', {
          runName: this.selectedRun,
        });

        let tasks: Task[] = [];
        try {
          tasks = await window.tauriInvoke<Task[]>('get_tasks', {
            runName: this.selectedRun,
          });
        } catch {
          // Ignore task fetch errors
        }

        const taskByWorker = new Map<string, string>();
        for (const task of tasks) {
          if (task.claimedBy && task.status === 'doing') {
            taskByWorker.set(task.claimedBy, task.id);
          }
        }

        const enrichedWorkers: EnrichedWorker[] = workers.map((worker, index) => {
          const isLeader = worker.name.endsWith('-0') || index === 0;
          const currentTask = taskByWorker.get(worker.name) || null;

          return {
            ...worker,
            isLeader,
            currentTask,
          };
        });

        const activeStatuses = ['working', 'waiting', 'awaiting'];
        this.workers = enrichedWorkers.sort((a, b) => {
          const aActive = activeStatuses.includes(a.status);
          const bActive = activeStatuses.includes(b.status);
          if (aActive && !bActive) return -1;
          if (!aActive && bActive) return 1;
          return a.name.localeCompare(b.name);
        });

        this.loading = false;
        this.error = null;
      } catch (err) {
        const error = err as Error;
        this.error = error.message || String(error);
        this.loading = false;
      }
    },

    formatTokens(n: number | null | undefined): string {
      if (n == null || n === 0) return '0';
      if (n < 1000) return String(n);
      if (n < 1000000) return (n / 1000).toFixed(1).replace(/\.0$/, '') + 'k';
      return (n / 1000000).toFixed(1).replace(/\.0$/, '') + 'M';
    },

    formatElapsedTime(sessionStartedAt: string | null | undefined): string {
      if (!sessionStartedAt) return '';
      const start = new Date(sessionStartedAt);
      const now = new Date();
      const seconds = Math.floor((now.getTime() - start.getTime()) / 1000);
      if (seconds < 60) return seconds + 's';
      const minutes = Math.floor(seconds / 60);
      if (minutes < 60) return minutes + 'm';
      const hours = Math.floor(minutes / 60);
      const mins = minutes % 60;
      if (hours < 24) return mins > 0 ? hours + 'h ' + mins + 'm' : hours + 'h';
      const days = Math.floor(hours / 24);
      const hrs = hours % 24;
      return hrs > 0 ? days + 'd ' + hrs + 'h' : days + 'd';
    },

    getContextClass(utilization: number | null | undefined): string {
      if (utilization == null) return 'text-wool-600';
      if (utilization >= 90) return 'text-terra';
      if (utilization >= 75) return 'text-amber-500';
      return 'text-wool-500';
    },

    async attachWorker(name: string) {
      if (!this.selectedRun) return;

      try {
        if (!window.tauriInvoke) {
          console.log('Attach to worker:', name);
          return;
        }

        await window.tauriInvoke('attach_worker', {
          runName: this.selectedRun,
          workerName: name,
        });
      } catch (err) {
        console.error('Failed to attach to worker:', err);
      }
    },

    openWorkerDetail(worker: EnrichedWorker) {
      this.selectedWorker = worker;
      this.showWorkerDetail = true;
    },

    closeWorkerDetail() {
      this.showWorkerDetail = false;
      this.selectedWorker = null;
    },

    async attachAndClose(name: string) {
      await this.attachWorker(name);
      this.closeWorkerDetail();
    },
  };
}
