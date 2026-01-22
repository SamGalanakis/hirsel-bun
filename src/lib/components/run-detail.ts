/**
 * Run detail panel Alpine component
 */

import DOMPurify from 'dompurify';
import { marked } from 'marked';
import type { Eval, RunDetail, Task, WorkerDisplay } from '../types';
import { calculateTimeProgress, formatElapsed, formatTimeRemaining } from '../utils/formatters';
import {
  canDeliver,
  canPause,
  canResume,
  getStatusBadgeClass,
  getStatusLabel,
} from '../utils/status';

// Helper to sort evals by startedAt descending (most recent first)
function sortEvals(evals: Eval[]): Eval[] {
  return (evals || [])
    .filter((e): e is Eval => e != null && e.startedAt != null)
    .sort((a, b) => new Date(b.startedAt).getTime() - new Date(a.startedAt).getTime());
}

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
    evals: [] as Eval[],
    diffStats: null as DiffStats | null,
    evalSpec: null as string | null,
    evalSpecLoading: false,
    loading: false,
    error: null as string | null,
    pollInterval: null as ReturnType<typeof setInterval> | null,
    activeTab: 'overview' as 'overview' | 'tasks' | 'specs' | 'evals' | 'chat' | 'config',

    // Eval detail view
    selectedEval: null as Eval | null,
    evalLogContent: null as string | null,
    evalLogLoading: false,

    // Deliver modal state
    deliverModalOpen: false,
    deliverBranch: '',
    deliverLoading: false,

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

    async loadEvals() {
      if (!this.runName) return;
      try {
        if (window.tauriInvoke) {
          const evals = await window.tauriInvoke<Eval[]>('get_evals', {
            runName: this.runName,
          });
          this.evals = sortEvals(evals);
        }
      } catch (err) {
        console.error('[runDetail] Error loading evals:', err);
        this.evals = [];
      }
    },

    selectEval(evalItem: Eval) {
      this.selectedEval = evalItem;
    },

    attachEval() {
      if (!this.selectedEval || !this.runName) return;
      // Open the worker output viewer with the eval name as worker name
      // (eval events are stored in worker_events table with eval_name as worker_name)
      window.dispatchEvent(
        new CustomEvent('show-worker-output', {
          detail: {
            runName: this.runName,
            workerName: this.selectedEval.evalName || `eval_${this.selectedEval.id}`,
          },
        }),
      );
    },

    clearSelectedEval() {
      this.selectedEval = null;
      this.evalLogContent = null;
    },

    async loadEvalLog(evalItem: Eval) {
      if (!evalItem.logFile || this.evalLogLoading) return;
      this.evalLogLoading = true;
      try {
        if (window.tauriInvoke) {
          const response = await window.tauriInvoke<{ content: string; exists: boolean }>(
            'get_eval_log_by_path',
            {
              runName: this.runName,
              logFile: evalItem.logFile,
            },
          );
          this.evalLogContent = response.exists ? response.content : null;
        }
      } catch (err) {
        console.error('[runDetail] Error loading eval log:', err);
        this.evalLogContent = null;
      } finally {
        this.evalLogLoading = false;
      }
    },

    getEvalStatusClass(status: string) {
      switch (status) {
        case 'passed':
          return 'text-sage';
        case 'failed':
          return 'text-terra';
        case 'running':
          return 'text-amber-500';
        default:
          return 'text-wool-500';
      }
    },

    getEvalStatusIcon(status: string) {
      switch (status) {
        case 'passed':
          return 'check-circle';
        case 'failed':
          return 'x-circle';
        case 'running':
          return 'loader';
        default:
          return 'circle';
      }
    },

    formatEvalTime(timestamp: string | null): string {
      if (!timestamp) return '';
      const date = new Date(timestamp);
      return date.toLocaleString('en-US', {
        month: 'short',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
      });
    },

    getEvalDuration(evalItem: Eval | null): string {
      if (!evalItem || !evalItem.startedAt) return '';
      const start = new Date(evalItem.startedAt);
      const end = evalItem.finishedAt ? new Date(evalItem.finishedAt) : new Date();
      const durationMs = end.getTime() - start.getTime();
      const seconds = Math.floor(durationMs / 1000);
      if (seconds < 60) return `${seconds}s`;
      const minutes = Math.floor(seconds / 60);
      const remainingSeconds = seconds % 60;
      return `${minutes}m ${remainingSeconds}s`;
    },

    getTaskProgress() {
      const done = this.tasks.filter((t) => t && t.status === 'done').length;
      const total = this.tasks.filter((t) => t != null).length;
      const percentage = total > 0 ? Math.round((done / total) * 100) : 0;
      return { done, total, percentage };
    },

    getWorkerCount() {
      const activeStatuses = ['working', 'waiting', 'eval'];
      const active = this.workers.filter((w) => w && activeStatuses.includes(w.status)).length;
      return { active, total: this.workers.filter((w) => w != null).length };
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
      // @ts-expect-error Alpine.js $watch magic property
      this.$watch('$root.selectedRun', async (newValue: string | null, oldValue: string | null) => {
        if (newValue && newValue !== oldValue) {
          await this.loadRunDetail(newValue);
        } else if (!newValue) {
          this.clearRunDetail();
        }
      });

      // @ts-expect-error Alpine.js $root magic property
      if (this.$root?.selectedRun) {
        // @ts-expect-error Alpine.js $root magic property
        await this.loadRunDetail(this.$root.selectedRun);
      }

      // Watch for tab changes to load eval spec on demand and auto-select latest eval
      // @ts-expect-error Alpine.js $watch magic property
      this.$watch('activeTab', async (newTab: string) => {
        if (newTab === 'specs' && this.runName && !this.evalSpec) {
          await this.loadEvalSpec();
        }
        // Auto-select the latest eval when switching to evals tab
        if (newTab === 'evals' && this.evals.length > 0) {
          await this.selectEval(this.evals[0]);
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
      // Reset eval state for the new run
      this.evalSpec = null;
      this.evalSpecLoading = false;
      this.evals = [];
      this.selectedEval = null;
      this.evalLogContent = null;

      // If already on specs tab, load eval spec after fetching run detail
      const wasOnSpecsTab = this.activeTab === 'specs';

      try {
        if (window.tauriInvoke) {
          const [detail, tasks, workers, evals] = await Promise.all([
            window.tauriInvoke<RunDetail>('get_run_detail', { runName: name }),
            window.tauriInvoke<Task[]>('get_tasks', { runName: name }),
            window.tauriInvoke<WorkerDisplay[]>('get_workers', { runName: name }),
            window.tauriInvoke<Eval[]>('get_evals', { runName: name }),
          ]);
          this.detail = detail;
          this.tasks = (tasks || []).filter((t): t is Task => t != null);
          this.workers = (workers || []).filter((w): w is WorkerDisplay => w != null);
          this.evals = sortEvals(evals);

          try {
            this.diffStats = await window.tauriInvoke<DiffStats>('get_diff_stats', {
              runName: name,
            });
          } catch {
            this.diffStats = null;
          }

          this.loading = false;

          // Load eval spec if we were already on specs tab
          if (wasOnSpecsTab) {
            await this.loadEvalSpec();
          }

          // Poll for updates
          this.pollInterval = setInterval(async () => {
            if (!this.runName) return;
            try {
              const [detail, tasks, workers, evals] = await Promise.all([
                window.tauriInvoke<RunDetail>('get_run_detail', { runName: this.runName }),
                window.tauriInvoke<Task[]>('get_tasks', { runName: this.runName }),
                window.tauriInvoke<WorkerDisplay[]>('get_workers', { runName: this.runName }),
                window.tauriInvoke<Eval[]>('get_evals', { runName: this.runName }),
              ]);
              this.detail = detail;
              this.tasks = (tasks || []).filter((t): t is Task => t != null);
              this.workers = (workers || []).filter((w): w is WorkerDisplay => w != null);
              this.evals = sortEvals(evals);
            } catch {
              // Errors likely because run was deleted - dataCache.fetchRuns handles cleanup
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
      this.evals = [];
      this.diffStats = null;
      this.evalSpec = null;
      this.evalSpecLoading = false;
      this.selectedEval = null;
      this.evalLogContent = null;
      this.evalLogLoading = false;
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
        window.toast?.error('Failed to pause run');
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
        window.toast?.error('Failed to resume run');
      }
    },

    openDeliverModal() {
      if (!this.runName || !this.canDeliver()) return;
      // Default to saved branch, or generate default branch name
      this.deliverBranch = this.detail?.branch || `hirsel/${this.runName}`;
      this.deliverModalOpen = true;
      this.deliverLoading = false;
    },

    closeDeliverModal() {
      this.deliverModalOpen = false;
      this.deliverBranch = '';
      this.deliverLoading = false;
    },

    async handleDeliver() {
      // Open modal instead of delivering directly
      this.openDeliverModal();
    },

    async confirmDeliver() {
      if (!this.runName || !this.deliverBranch.trim()) return;
      this.deliverLoading = true;
      try {
        if (window.tauriInvoke) {
          const branch = await window.tauriInvoke<string>('deliver_run', {
            runName: this.runName,
            branchName: this.deliverBranch.trim(),
          });
          window.toast.success(`Delivered to ${branch}`);
          this.closeDeliverModal();
          await this.loadRunDetail(this.runName);
        }
      } catch (err) {
        const error = err as Error;
        console.error('Failed to deliver:', error);
        window.toast.error('Failed to deliver');
        this.deliverLoading = false;
      }
    },

    /**
     * Render markdown content to HTML (sanitized for XSS protection)
     */
    renderMarkdown(content: string | null | undefined): string {
      if (!content) return '<p class="text-wool-500 italic">No content</p>';
      return DOMPurify.sanitize(marked(content) as string);
    },
  };
}
