/**
 * Worker Panel Alpine.js Component
 *
 * Displays workers for the selected run with status indicators,
 * leader badge, current task, and context utilization metrics.
 * Allows clicking on a worker to attach to its tmux session.
 */

import { getWorkers, attachWorker, getWorkerMetrics, getTasks } from '../lib/api';
import type { Worker, WorkerStatus, WorkerDisplay, Task } from '../lib/types';

/**
 * Status color class mapping for worker status dots
 */
const STATUS_DOT_CLASS: Record<WorkerStatus, string> = {
  idle: 'status-idle',
  working: 'status-working',
  waiting: 'status-waiting',
  awaiting: 'status-waiting',
  paused: 'status-waiting',
  done: 'status-done',
  error: 'status-error',
};

/**
 * Status display labels
 */
const STATUS_LABEL: Record<WorkerStatus, string> = {
  idle: 'Idle',
  working: 'Working',
  waiting: 'Waiting',
  awaiting: 'Awaiting',
  paused: 'Paused',
  done: 'Done',
  error: 'Error',
};

/**
 * Format context utilization as a percentage bar
 */
function formatContextUtilization(utilization: number | null): string {
  if (utilization === null || utilization === undefined) return '';
  return `${Math.round(utilization * 100)}%`;
}

/**
 * Worker panel component data interface
 */
export interface WorkerPanelData {
  workers: WorkerDisplay[];
  loading: boolean;
  error: string | null;
  selectedRun: string | null;
  pollInterval: ReturnType<typeof setInterval> | null;
}

/**
 * Alpine.js component factory for the worker panel
 */
export function workerPanel(): WorkerPanelData & {
  init(): Promise<void>;
  destroy(): void;
  onRunSelected(runName: string | null): Promise<void>;
  fetchWorkers(): Promise<void>;
  attachToWorker(workerName: string): Promise<void>;
  getStatusDotClass(status: WorkerStatus): string;
  getStatusLabel(status: WorkerStatus): string;
  formatContextUtilization(utilization: number | null): string;
  getContextBarWidth(utilization: number | null): string;
  getContextBarColor(utilization: number | null): string;
} {
  return {
    workers: [],
    loading: false,
    error: null,
    selectedRun: null,
    pollInterval: null,

    /**
     * Initialize component and set up event listeners
     */
    async init(): Promise<void> {
      // Listen for run selection events
      window.addEventListener('run-selected', ((e: CustomEvent<string | null>) => {
        this.onRunSelected(e.detail);
      }) as EventListener);

      // Check if a run is already selected (from parent scope or global state)
      const globalState = (window as unknown as Record<string, unknown>).__hirselRunName;
      if (typeof globalState === 'string') {
        await this.onRunSelected(globalState);
      }
    },

    /**
     * Cleanup when component is destroyed
     */
    destroy(): void {
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }
    },

    /**
     * Handle run selection change
     */
    async onRunSelected(runName: string | null): Promise<void> {
      // Stop existing polling
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }

      this.selectedRun = runName;

      if (!runName) {
        this.workers = [];
        this.loading = false;
        this.error = null;
        return;
      }

      // Fetch workers immediately
      await this.fetchWorkers();

      // Start polling for updates every 2 seconds
      this.pollInterval = setInterval(() => {
        this.fetchWorkers();
      }, 2000);
    },

    /**
     * Fetch workers from the backend
     */
    async fetchWorkers(): Promise<void> {
      if (!this.selectedRun) return;

      // Only show loading on first fetch
      if (this.workers.length === 0) {
        this.loading = true;
      }

      try {
        const workers = await getWorkers(this.selectedRun);

        // Also fetch tasks to map claimed tasks to workers
        let tasks: Task[] = [];
        try {
          tasks = await getTasks(this.selectedRun);
        } catch {
          // Ignore task fetch errors
        }

        // Create task map for quick lookup
        const taskByWorker = new Map<string, string>();
        for (const task of tasks) {
          if (task.claimedBy && task.status === 'doing') {
            taskByWorker.set(task.claimedBy, task.id);
          }
        }

        // Enrich workers with display properties
        const enrichedWorkers: WorkerDisplay[] = await Promise.all(
          workers.map(async (worker): Promise<WorkerDisplay> => {
            // Determine if this worker is the leader (first worker or has leader indicator)
            const isLeader = worker.name.endsWith('-0') ||
                           worker.name === workers[0]?.name;

            // Get current task from task map
            const currentTask = taskByWorker.get(worker.name) || null;

            // Try to fetch metrics (may fail for inactive workers)
            let metrics = {
              contextUtilization: null as number | null,
              inputTokens: null as number | null,
              outputTokens: null as number | null,
              turns: null as number | null,
            };

            if (worker.status === 'working' || worker.status === 'waiting') {
              try {
                const m = await getWorkerMetrics(this.selectedRun!, worker.name);
                metrics = {
                  contextUtilization: m.contextUtilization,
                  inputTokens: m.inputTokens,
                  outputTokens: m.outputTokens,
                  turns: m.turns,
                };
              } catch {
                // Metrics not available, use defaults
              }
            }

            return {
              ...worker,
              isLeader,
              currentTask,
              contextUtilization: metrics.contextUtilization,
              inputTokens: metrics.inputTokens,
              outputTokens: metrics.outputTokens,
              turns: metrics.turns,
            };
          })
        );

        // Sort: active workers first, then by name
        this.workers = enrichedWorkers.sort((a, b) => {
          const activeStatuses: WorkerStatus[] = ['working', 'waiting', 'awaiting'];
          const aActive = activeStatuses.includes(a.status);
          const bActive = activeStatuses.includes(b.status);
          if (aActive && !bActive) return -1;
          if (!aActive && bActive) return 1;
          return a.name.localeCompare(b.name);
        });

        this.loading = false;
        this.error = null;
      } catch (err) {
        this.error = err instanceof Error ? err.message : String(err);
        this.loading = false;
      }
    },

    /**
     * Attach to a worker's tmux session
     */
    async attachToWorker(workerName: string): Promise<void> {
      if (!this.selectedRun) return;

      try {
        await attachWorker(this.selectedRun, workerName);
      } catch (err) {
        console.error('Failed to attach to worker:', err);
        // Could show a toast notification here
      }
    },

    /**
     * Get CSS class for status dot
     */
    getStatusDotClass(status: WorkerStatus): string {
      return STATUS_DOT_CLASS[status] || 'status-idle';
    },

    /**
     * Get display label for status
     */
    getStatusLabel(status: WorkerStatus): string {
      return STATUS_LABEL[status] || status;
    },

    /**
     * Format context utilization percentage
     */
    formatContextUtilization,

    /**
     * Get width for context utilization bar
     */
    getContextBarWidth(utilization: number | null): string {
      if (utilization === null || utilization === undefined) return '0%';
      return `${Math.round(utilization * 100)}%`;
    },

    /**
     * Get color class for context utilization bar
     */
    getContextBarColor(utilization: number | null): string {
      if (utilization === null || utilization === undefined) return 'bg-wool-700';
      if (utilization >= 0.9) return 'bg-terra';
      if (utilization >= 0.7) return 'bg-golden';
      return 'bg-sage';
    },
  };
}

/**
 * Register the component with Alpine.js
 */
export function registerWorkerPanelComponent(): void {
  if (typeof window !== 'undefined') {
    (window as unknown as Record<string, unknown>).workerPanel = workerPanel;
  }
}

// Auto-register if Alpine is already loaded
if (typeof window !== 'undefined' && typeof Alpine !== 'undefined') {
  registerWorkerPanelComponent();
}

// Declare Alpine global for TypeScript
declare const Alpine: {
  store: (name: string) => Record<string, unknown> | undefined;
};
