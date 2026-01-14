/**
 * Run detail panel Alpine component
 */

import { marked } from 'marked';
import {
  formatElapsed,
  formatTimeRemaining,
  calculateTimeProgress,
} from '../utils/formatters';
import {
  getStatusBadgeClass,
  getStatusLabel,
  canPause,
  canResume,
  canDeliver,
} from '../utils/status';
import type { RunDetail, Task, WorkerDisplay } from '../types';

interface DiffStats {
  insertions: number;
  deletions: number;
}

/**
 * Run detail component
 */
export function runDetail() {
  return {
    runName: null as string | null,
    detail: null as RunDetail | null,
    tasks: [] as Task[],
    workers: [] as WorkerDisplay[],
    diffStats: null as DiffStats | null,
    evalSpec: null as string | null,
    evalSpecLoading: false,
    loading: false,
    error: null as string | null,
    pollInterval: null as ReturnType<typeof setInterval> | null,
    activeTab: 'overview' as 'overview' | 'tasks' | 'spec' | 'eval' | 'eval-spec' | 'messages',

    // Formatting helpers
    formatElapsed,
    formatTimeRemaining,
    calculateTimeProgress,
    getStatusBadgeClass,
    getStatusLabel,

    canPause() {
      return canPause(this.detail?.status);
    },

    canResume() {
      return canResume(this.detail?.status);
    },

    canDeliver() {
      return canDeliver(this.detail?.status);
    },

    async loadEvalSpec() {
      if (!this.runName || this.evalSpec !== null || this.evalSpecLoading) return;

      this.evalSpecLoading = true;
      try {
        if (window.tauriInvoke) {
          this.evalSpec = await window.tauriInvoke<string | null>('get_eval_spec', {
            runName: this.runName,
          });
        }
      } catch (err) {
        console.error('[runDetail] Error loading eval spec:', err);
        this.evalSpec = null;
      } finally {
        this.evalSpecLoading = false;
      }
    },

    getTaskProgress() {
      const done = this.tasks.filter(t => t.status === 'done').length;
      const total = this.tasks.length;
      const percentage = total > 0 ? Math.round((done / total) * 100) : 0;
      return { done, total, percentage };
    },

    getWorkerCount() {
      const activeStatuses = ['working', 'waiting', 'eval'];
      const active = this.workers.filter(w => activeStatuses.includes(w.status)).length;
      return { active, total: this.workers.length };
    },

    async init() {
      window.addEventListener('run-selected', async (e: Event) => {
        const customEvent = e as CustomEvent<string | null>;
        if (customEvent.detail) {
          await this.loadRunDetail(customEvent.detail);
        } else {
          this.clearRunDetail();
        }
      });

      const self = this;
      // @ts-expect-error Alpine.js $watch magic property
      this.$watch('$root.selectedRun', async (newValue: string | null, oldValue: string | null) => {
        if (newValue && newValue !== oldValue) {
          await self.loadRunDetail(newValue);
        } else if (!newValue) {
          self.clearRunDetail();
        }
      });

      // @ts-expect-error Alpine.js $root magic property
      if (this.$root?.selectedRun) {
        // @ts-expect-error Alpine.js $root magic property
        await this.loadRunDetail(this.$root.selectedRun);
      }

      // Watch for tab changes to load eval spec on demand
      // @ts-expect-error Alpine.js $watch magic property
      this.$watch('activeTab', async (newTab: string) => {
        if (newTab === 'eval-spec' && this.runName) {
          await this.loadEvalSpec();
        }
      });

      // Listen for tab switch events from keyboard shortcuts
      window.addEventListener('switch-tab', (e: Event) => {
        const customEvent = e as CustomEvent<string>;
        if (customEvent.detail && this.runName) {
          this.activeTab = customEvent.detail as typeof this.activeTab;
        }
      });
    },

    destroy() {
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }
    },

    async loadRunDetail(name: string) {
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }

      this.runName = name;
      this.loading = true;
      this.error = null;
      // Reset eval spec for the new run
      this.evalSpec = null;
      this.evalSpecLoading = false;

      // If already on eval-spec tab, load it after fetching run detail
      const wasOnEvalSpecTab = this.activeTab === 'eval-spec';

      try {
        if (window.tauriInvoke) {
          const [detail, tasks, workers] = await Promise.all([
            window.tauriInvoke<RunDetail>('get_run_detail', { runName: name }),
            window.tauriInvoke<Task[]>('get_tasks', { runName: name }),
            window.tauriInvoke<WorkerDisplay[]>('get_workers', { runName: name }),
          ]);
          this.detail = detail;
          this.tasks = tasks;
          this.workers = workers;

          try {
            this.diffStats = await window.tauriInvoke<DiffStats>('get_diff_stats', {
              runName: name,
            });
          } catch {
            this.diffStats = null;
          }

          this.loading = false;

          // Load eval spec if we were already on that tab
          if (wasOnEvalSpecTab) {
            await this.loadEvalSpec();
          }

          // Poll for updates
          this.pollInterval = setInterval(async () => {
            if (!this.runName) return;
            try {
              const [detail, tasks, workers] = await Promise.all([
                window.tauriInvoke<RunDetail>('get_run_detail', { runName: this.runName }),
                window.tauriInvoke<Task[]>('get_tasks', { runName: this.runName }),
                window.tauriInvoke<WorkerDisplay[]>('get_workers', { runName: this.runName }),
              ]);
              this.detail = detail;
              this.tasks = tasks;
              this.workers = workers;
            } catch (err) {
              console.error('Failed to poll:', err);
            }
          }, 2000);
        } else {
          this.loading = false;
        }
      } catch (err) {
        const error = err as Error;
        console.error('[runDetail] Error loading run detail:', error);
        this.error = error.message || String(error);
        this.loading = false;
      }
    },

    clearRunDetail() {
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }
      this.runName = null;
      this.detail = null;
      this.tasks = [];
      this.workers = [];
      this.diffStats = null;
      this.evalSpec = null;
      this.evalSpecLoading = false;
      this.loading = false;
      this.error = null;
    },

    async handlePause() {
      if (!this.runName || !this.canPause()) return;
      try {
        if (window.tauriInvoke) {
          await window.tauriInvoke('pause_run', { runName: this.runName });
          await this.loadRunDetail(this.runName);
        }
      } catch (err) {
        const error = err as Error;
        window.toast?.error('Failed to pause', error.message || String(error));
      }
    },

    async handleResume() {
      if (!this.runName || !this.canResume()) return;
      try {
        if (window.tauriInvoke) {
          await window.tauriInvoke('resume_run', { runName: this.runName });
          await this.loadRunDetail(this.runName);
        }
      } catch (err) {
        const error = err as Error;
        window.toast?.error('Failed to resume', error.message || String(error));
      }
    },

    async handleDeliver() {
      if (!this.runName || !this.canDeliver()) return;
      try {
        if (window.tauriInvoke) {
          const branch = await window.tauriInvoke<string>('deliver_run', {
            runName: this.runName,
          });
          window.toast.success(`Delivered to branch: ${branch}`, 'Delivery complete');
          await this.loadRunDetail(this.runName);
        }
      } catch (err) {
        const error = err as Error;
        console.error('Failed to deliver:', error);
        window.toast.error(error.message || String(error), 'Failed to deliver');
      }
    },

    /**
     * Render markdown content to HTML
     */
    renderMarkdown(content: string | null | undefined): string {
      if (!content) return '<p class="text-wool-500 italic">No content</p>';
      return marked(content) as string;
    },
  };
}
