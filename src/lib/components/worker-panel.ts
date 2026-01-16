/**
 * Worker panel Alpine component
 */

import type { WorkerDisplay, Task } from '../types';
import { formatTokens, formatElapsedTime } from '../utils/formatters';
import { dataCache, DATA_EVENTS } from '../data-cache';

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
    _eventCleanups: [] as (() => void)[],
    _cacheUnsubscribe: null as (() => void) | null,
    selectedWorker: null as EnrichedWorker | null,
    showWorkerDetail: false,

    async init() {
      // Subscribe to shared cache
      this._cacheUnsubscribe = dataCache.subscribe();

      // Listen for workers updates from cache
      const workersUpdatedHandler = (e: Event) => {
        const customEvent = e as CustomEvent<WorkerDisplay[]>;
        this.enrichAndSetWorkers(customEvent.detail);
      };
      window.addEventListener(DATA_EVENTS.WORKERS_UPDATED, workersUpdatedHandler);
      this._eventCleanups.push(() => window.removeEventListener(DATA_EVENTS.WORKERS_UPDATED, workersUpdatedHandler));

      // Listen for tasks updates (to show current task per worker)
      const tasksUpdatedHandler = (e: Event) => {
        const customEvent = e as CustomEvent<Task[]>;
        this.updateCurrentTasks(customEvent.detail);
      };
      window.addEventListener(DATA_EVENTS.TASKS_UPDATED, tasksUpdatedHandler);
      this._eventCleanups.push(() => window.removeEventListener(DATA_EVENTS.TASKS_UPDATED, tasksUpdatedHandler));

      // Listen for run selection
      const runSelectedHandler = (e: Event) => {
        const customEvent = e as CustomEvent<string | null>;
        this.selectedRun = customEvent.detail;
        if (!customEvent.detail) {
          this.workers = [];
          this.loading = false;
          this.error = null;
        }
      };
      window.addEventListener('run-selected', runSelectedHandler);
      this._eventCleanups.push(() => window.removeEventListener('run-selected', runSelectedHandler));

      // Get initial data from cache
      const cachedWorkers = dataCache.getWorkers();
      if (cachedWorkers.length > 0) {
        this.enrichAndSetWorkers(cachedWorkers);
      }

      const app = this.getAppState();
      if (app && app.selectedRun) {
        this.selectedRun = app.selectedRun;
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

    /**
     * Enrich workers with isLeader and currentTask, then sort and set
     */
    enrichAndSetWorkers(workers: WorkerDisplay[]) {
      const tasks = dataCache.getTasks();
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
    },

    /**
     * Update current tasks when tasks change
     */
    updateCurrentTasks(tasks: Task[]) {
      const taskByWorker = new Map<string, string>();
      for (const task of tasks) {
        if (task.claimedBy && task.status === 'doing') {
          taskByWorker.set(task.claimedBy, task.id);
        }
      }

      // Update current task for each worker
      for (const worker of this.workers) {
        worker.currentTask = taskByWorker.get(worker.name) || null;
      }
    },

    // Use shared formatters
    formatTokens,
    formatElapsedTime,

    getContextClass(utilization: number | null | undefined): string {
      if (utilization == null) return 'text-wool-600';
      if (utilization >= 90) return 'text-terra';
      if (utilization >= 75) return 'text-amber-500';
      return 'text-wool-500';
    },

    async openTerminal(name: string) {
      if (!this.selectedRun || !window.tauriInvoke) return;

      try {
        await window.tauriInvoke('open_worker_terminal', {
          runName: this.selectedRun,
          workerName: name,
        });
      } catch (err) {
        const error = err as Error;
        console.error('Failed to open terminal:', error);
        window.toast?.error('Failed to open terminal');
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
      // Dispatch event to show worker output viewer
      window.dispatchEvent(new CustomEvent('show-worker-output', {
        detail: {
          runName: this.selectedRun,
          workerName: name,
        },
      }));
      this.closeWorkerDetail();
    },
  };
}
