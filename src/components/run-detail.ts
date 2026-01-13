/**
 * Run Detail Alpine.js Component
 *
 * Displays detailed information about the selected run including:
 * - Header with name and status badge
 * - Elapsed time and time limit progress
 * - Worker count and task progress
 * - Project path and branch info
 * - Action buttons (pause/resume/deliver)
 */

import {
  getRunDetail,
  getTasks,
  getWorkers,
  pauseRun,
  resumeRun,
  deliverRun,
  getDiffStats,
} from '../lib/api';
import type { RunDetail, RunStatus, Task, Worker } from '../lib/types';

/**
 * Status badge class mapping
 */
const STATUS_BADGE_CLASSES: Record<RunStatus, string> = {
  idle: 'bg-wool-700/20 text-wool-500',
  working: 'bg-amber-500/20 text-amber-500',
  paused: 'bg-golden/20 text-golden',
  runaway: 'bg-terra/20 text-terra',
  timed_out: 'bg-terra/20 text-terra',
  eval: 'bg-amber-400/20 text-amber-400',
  eval_failed: 'bg-terra/20 text-terra',
  waiting: 'bg-golden/20 text-golden',
  done: 'bg-sage/20 text-sage',
  delivered: 'bg-sage/20 text-sage',
  merged: 'bg-sage/20 text-sage',
};

/**
 * Status display labels
 */
const STATUS_LABELS: Record<RunStatus, string> = {
  idle: 'Idle',
  working: 'Working',
  paused: 'Paused',
  runaway: 'Runaway',
  timed_out: 'Timed Out',
  eval: 'Evaluating',
  eval_failed: 'Eval Failed',
  waiting: 'Waiting',
  done: 'Done',
  delivered: 'Delivered',
  merged: 'Merged',
};

/**
 * Format elapsed time
 */
function formatElapsed(minutes: number | null | undefined): string {
  if (!minutes) return '0m';
  if (minutes < 60) return `${minutes}m`;
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return m > 0 ? `${h}h ${m}m` : `${h}h`;
}

/**
 * Format time remaining
 */
function formatTimeRemaining(limit: number | null, elapsed: number | null): string {
  if (!limit) return '';
  const remaining = limit - (elapsed || 0);
  if (remaining <= 0) return 'Time up!';
  if (remaining < 60) return `${remaining}m remaining`;
  const h = Math.floor(remaining / 60);
  const m = remaining % 60;
  return m > 0 ? `${h}h ${m}m remaining` : `${h}h remaining`;
}

/**
 * Calculate time progress percentage
 */
function calculateTimeProgress(elapsed: number | null, limit: number | null): number {
  if (!limit || !elapsed) return 0;
  return Math.min(100, Math.round((elapsed / limit) * 100));
}

/**
 * Run detail component data
 */
export interface RunDetailData {
  runName: string | null;
  detail: RunDetail | null;
  tasks: Task[];
  workers: Worker[];
  diffStats: { filesChanged: number; insertions: number; deletions: number } | null;
  loading: boolean;
  error: string | null;
  pollInterval: ReturnType<typeof setInterval> | null;
}

/**
 * Alpine.js component factory for run details
 */
export function runDetail(): RunDetailData & {
  init(): void;
  destroy(): void;
  loadRunDetail(name: string): Promise<void>;
  clearRunDetail(): void;
  getStatusBadgeClass(status: RunStatus | undefined): string;
  getStatusLabel(status: RunStatus | undefined): string;
  formatElapsed(minutes: number | null | undefined): string;
  formatTimeRemaining(limit: number | null, elapsed: number | null): string;
  calculateTimeProgress(elapsed: number | null, limit: number | null): number;
  getTaskProgress(): { done: number; total: number; percentage: number };
  getWorkerCount(): { active: number; total: number };
  canPause(): boolean;
  canResume(): boolean;
  canDeliver(): boolean;
  handlePause(): Promise<void>;
  handleResume(): Promise<void>;
  handleDeliver(): Promise<void>;
} {
  return {
    runName: null,
    detail: null,
    tasks: [],
    workers: [],
    diffStats: null,
    loading: false,
    error: null,
    pollInterval: null,

    /**
     * Initialize the component
     */
    init(): void {
      // Listen for run selection events
      window.addEventListener('run-selected', ((e: CustomEvent<string | null>) => {
        if (e.detail) {
          this.loadRunDetail(e.detail);
        } else {
          this.clearRunDetail();
        }
      }) as EventListener);

      // Start polling if a run is already selected
      if (this.runName) {
        this.loadRunDetail(this.runName);
      }
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
     * Load run detail data
     */
    async loadRunDetail(name: string): Promise<void> {
      // Stop existing polling
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }

      this.runName = name;
      this.loading = true;
      this.error = null;

      try {
        // Fetch all data in parallel
        const [detail, tasks, workers] = await Promise.all([
          getRunDetail(name),
          getTasks(name),
          getWorkers(name),
        ]);

        this.detail = detail;
        this.tasks = tasks;
        this.workers = workers;

        // Try to get diff stats (may fail if no changes)
        try {
          this.diffStats = await getDiffStats(name);
        } catch {
          this.diffStats = null;
        }

        this.loading = false;

        // Update parent app state if available
        this.updateAppState();

        // Start polling for updates every 2 seconds
        this.pollInterval = setInterval(async () => {
          if (!this.runName) return;
          try {
            const [detail, tasks, workers] = await Promise.all([
              getRunDetail(this.runName),
              getTasks(this.runName),
              getWorkers(this.runName),
            ]);
            this.detail = detail;
            this.tasks = tasks;
            this.workers = workers;
            this.updateAppState();
          } catch (err) {
            console.error('Failed to poll run detail:', err);
          }
        }, 2000);
      } catch (err) {
        this.error = err instanceof Error ? err.message : String(err);
        this.loading = false;
      }
    },

    /**
     * Update the parent app state with current data
     */
    updateAppState(): void {
      if (typeof Alpine !== 'undefined' && Alpine.store) {
        const store = Alpine.store('app') as Record<string, unknown> | undefined;
        if (store) {
          store.runDetail = this.detail;
          store.tasksDone = this.tasks.filter(t => t.status === 'done').length;
          store.tasksTotal = this.tasks.length;
        }
      }
    },

    /**
     * Clear run detail when no run is selected
     */
    clearRunDetail(): void {
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }
      this.runName = null;
      this.detail = null;
      this.tasks = [];
      this.workers = [];
      this.diffStats = null;
      this.loading = false;
      this.error = null;
    },

    /**
     * Get CSS class for status badge
     */
    getStatusBadgeClass(status: RunStatus | undefined): string {
      if (!status) return STATUS_BADGE_CLASSES.idle;
      return STATUS_BADGE_CLASSES[status] || STATUS_BADGE_CLASSES.idle;
    },

    /**
     * Get display label for status
     */
    getStatusLabel(status: RunStatus | undefined): string {
      if (!status) return 'Unknown';
      return STATUS_LABELS[status] || status;
    },

    /**
     * Format elapsed time
     */
    formatElapsed,

    /**
     * Format time remaining
     */
    formatTimeRemaining,

    /**
     * Calculate time progress percentage
     */
    calculateTimeProgress,

    /**
     * Get task progress summary
     */
    getTaskProgress(): { done: number; total: number; percentage: number } {
      const done = this.tasks.filter(t => t.status === 'done').length;
      const total = this.tasks.length;
      const percentage = total > 0 ? Math.round((done / total) * 100) : 0;
      return { done, total, percentage };
    },

    /**
     * Get worker count summary
     */
    getWorkerCount(): { active: number; total: number } {
      const activeStatuses = ['working', 'waiting', 'eval'];
      const active = this.workers.filter(w => activeStatuses.includes(w.status)).length;
      return { active, total: this.workers.length };
    },

    /**
     * Check if run can be paused
     */
    canPause(): boolean {
      const pauseableStatuses: RunStatus[] = ['working', 'eval', 'waiting'];
      return !!this.detail && pauseableStatuses.includes(this.detail.status);
    },

    /**
     * Check if run can be resumed
     */
    canResume(): boolean {
      const resumeableStatuses: RunStatus[] = ['paused', 'timed_out', 'runaway'];
      return !!this.detail && resumeableStatuses.includes(this.detail.status);
    },

    /**
     * Check if run can be delivered
     */
    canDeliver(): boolean {
      const deliverableStatuses: RunStatus[] = ['done', 'paused', 'timed_out'];
      return !!this.detail && deliverableStatuses.includes(this.detail.status);
    },

    /**
     * Handle pause action
     */
    async handlePause(): Promise<void> {
      if (!this.runName || !this.canPause()) return;
      try {
        await pauseRun(this.runName);
        // Refresh detail
        await this.loadRunDetail(this.runName);
      } catch (err) {
        console.error('Failed to pause run:', err);
        alert(`Failed to pause: ${err instanceof Error ? err.message : err}`);
      }
    },

    /**
     * Handle resume action
     */
    async handleResume(): Promise<void> {
      if (!this.runName || !this.canResume()) return;
      try {
        await resumeRun(this.runName);
        // Refresh detail
        await this.loadRunDetail(this.runName);
      } catch (err) {
        console.error('Failed to resume run:', err);
        alert(`Failed to resume: ${err instanceof Error ? err.message : err}`);
      }
    },

    /**
     * Handle deliver action
     */
    async handleDeliver(): Promise<void> {
      if (!this.runName || !this.canDeliver()) return;
      try {
        const branchName = await deliverRun(this.runName);
        alert(`Delivered to branch: ${branchName}`);
        // Refresh detail
        await this.loadRunDetail(this.runName);
      } catch (err) {
        console.error('Failed to deliver run:', err);
        alert(`Failed to deliver: ${err instanceof Error ? err.message : err}`);
      }
    },
  };
}

/**
 * Register the component with Alpine.js
 */
export function registerRunDetailComponent(): void {
  if (typeof window !== 'undefined') {
    (window as unknown as Record<string, unknown>).runDetail = runDetail;
  }
}

// Auto-register if Alpine is already loaded
if (typeof window !== 'undefined' && typeof Alpine !== 'undefined') {
  registerRunDetailComponent();
}

// Declare Alpine global for TypeScript
declare const Alpine: {
  store: (name: string) => Record<string, unknown> | undefined;
};
