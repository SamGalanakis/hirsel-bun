/**
 * Run list sidebar Alpine component
 */

import { formatElapsed, formatProgress, formatRelativeTime } from '../utils/formatters';
import { getStatusBadgeClass, getStatusLabel, getProgressBarClass } from '../utils/status';
import { createDraft } from '../api';
import type { RunSummary } from '../types';

declare const Alpine: {
  store: (name: string) => { selectedRun?: string | null } | undefined;
};

/**
 * Run list component
 */
export function runList() {
  return {
    runs: [] as RunSummary[],
    selectedRun: null as string | null,
    selectedIndex: -1,
    loading: true,
    error: null as string | null,
    contextMenuVisible: false,
    contextMenuX: 0,
    contextMenuY: 0,
    contextMenuRun: null as string | null,
    pollInterval: null as ReturnType<typeof setInterval> | null,

    // Formatting helpers
    formatElapsed,
    formatProgress,
    formatRelativeTime,
    getStatusBadgeClass,
    getStatusLabel,
    getProgressBarClass,

    /**
     * Get the appropriate time display for a run based on its status
     * - Draft: shows relative creation time (e.g., "2h ago")
     * - Completed: shows duration (e.g., "1h 30m")
     * - Active: shows elapsed time (e.g., "45m")
     */
    getTimeDisplay(run: RunSummary): string {
      const completedStatuses = ['done', 'delivered', 'merged', 'timed_out', 'eval_failed', 'runaway'];

      if (run.status === 'draft') {
        return formatRelativeTime(run.createdAt);
      }

      if (completedStatuses.includes(run.status)) {
        // For completed runs, show duration
        return formatElapsed(run.elapsedMinutes);
      }

      // For active runs, show elapsed time
      return formatElapsed(run.elapsedMinutes);
    },

    getStatusDotClass(status: string) {
      const classes: Record<string, string> = {
        draft: 'status-draft',
        idle: 'status-idle',
        working: 'status-working',
        paused: 'status-waiting',
        runaway: 'status-error',
        timed_out: 'status-error',
        eval: 'status-working',
        eval_failed: 'status-error',
        waiting: 'status-waiting',
        done: 'status-done',
        delivered: 'status-done',
        merged: 'status-done',
      };
      return classes[status] || 'status-idle';
    },

    isDraft(status: string) {
      return status === 'draft';
    },

    formatTimeRemaining(limit: number | null, elapsed: number | null) {
      if (!limit) return '';
      const remaining = limit - (elapsed || 0);
      if (remaining <= 0) return '⚠ Time up!';
      if (remaining < 60) return remaining + 'm left';
      const h = Math.floor(remaining / 60);
      const m = remaining % 60;
      return h + 'h' + (m > 0 ? ' ' + m + 'm' : '') + ' left';
    },

    async init() {
      await this.fetchRuns(true);

      this.pollInterval = setInterval(() => {
        this.fetchRuns(false);
      }, 2000);

      window.addEventListener('run-selected', (e: Event) => {
        const customEvent = e as CustomEvent<string | null>;
        this.selectedRun = customEvent.detail;
        const index = this.runs.findIndex(r => r.name === customEvent.detail);
        if (index >= 0) this.selectedIndex = index;
      });

      document.addEventListener('click', () => {
        this.hideContextMenu();
      });
    },

    destroy() {
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }
    },

    async fetchRuns(autoSelectFirst = false) {
      try {
        if (window.tauriInvoke) {
          this.runs = await window.tauriInvoke<RunSummary[]>('get_runs');

          if (autoSelectFirst && this.runs.length > 0 && !this.selectedRun) {
            // Delay auto-select to ensure app-state event listener is ready
            setTimeout(() => {
              if (!this.selectedRun && this.runs.length > 0) {
                const firstRun = this.runs[0];
                this.selectRunWithDraft(firstRun.name, firstRun.status);
              }
            }, 50);
          }
        } else {
          setTimeout(() => this.fetchRuns(autoSelectFirst), 100);
          return;
        }
        this.loading = false;
        this.error = null;
      } catch (err) {
        const error = err as Error;
        console.error('[fetchRuns] error:', error);
        this.error = error.message || String(error);
        this.loading = false;
        this.runs = [];
      }
    },

    selectRun(name: string) {
      this.selectedRun = name;
      this.selectedIndex = this.runs.findIndex(r => r.name === name);
      if (typeof Alpine !== 'undefined' && Alpine.store && Alpine.store('app')) {
        const store = Alpine.store('app');
        if (store) store.selectedRun = name;
      }
      window.dispatchEvent(new CustomEvent('run-selected', { detail: name }));
    },

    // Context menu
    showContextMenu(event: MouseEvent, runName: string) {
      event.preventDefault();
      event.stopPropagation();
      this.contextMenuX = event.clientX;
      this.contextMenuY = event.clientY;
      this.contextMenuRun = runName;
      this.contextMenuVisible = true;
    },

    hideContextMenu() {
      this.contextMenuVisible = false;
      this.contextMenuRun = null;
    },

    async contextPause() {
      if (!this.contextMenuRun) return;
      try {
        if (window.tauriInvoke) {
          await window.tauriInvoke('pause_run', { runName: this.contextMenuRun });
          await this.fetchRuns();
        }
      } catch (err) {
        const error = err as Error;
        window.toast.error('Failed to pause run');
      }
      this.hideContextMenu();
    },

    async contextResume() {
      if (!this.contextMenuRun) return;
      try {
        if (window.tauriInvoke) {
          await window.tauriInvoke('resume_run', { runName: this.contextMenuRun });
          await this.fetchRuns();
        }
      } catch (err) {
        const error = err as Error;
        window.toast.error('Failed to resume run');
      }
      this.hideContextMenu();
    },

    async contextDelete() {
      if (!this.contextMenuRun) return;
      const runToDelete = this.contextMenuRun;
      this.hideContextMenu();

      const confirmed = await (window as any).confirmDialog?.delete(runToDelete, 'run')
        ?? confirm(`Delete run "${runToDelete}"?`);
      if (!confirmed) return;

      try {
        if (window.tauriInvoke) {
          await window.tauriInvoke('delete_run', { runName: runToDelete });
          if (this.selectedRun === runToDelete) {
            this.selectedRun = null;
            this.selectedIndex = -1;
            window.dispatchEvent(new CustomEvent('run-selected', { detail: null }));
          }
          await this.fetchRuns();
        }
      } catch (err) {
        const error = err as Error;
        window.toast.error('Failed to delete run');
      }
    },

    async contextDeliver() {
      if (!this.contextMenuRun) return;
      try {
        if (window.tauriInvoke) {
          const branch = await window.tauriInvoke<string>('deliver_run', {
            runName: this.contextMenuRun,
          });
          window.toast.success(`Delivered to ${branch}`);
          await this.fetchRuns();
        }
      } catch (err) {
        const error = err as Error;
        window.toast.error('Failed to deliver');
      }
      this.hideContextMenu();
    },

    /**
     * Create a new draft run and select it
     */
    async createNewDraft() {
      try {
        const detail = await createDraft();
        window.toast.success(`Draft "${detail.name}" created`);

        // Refresh runs list
        await this.fetchRuns(false);

        // Select the new draft
        this.selectRun(detail.name);

        // Dispatch draft-selected event for the draft editor
        window.dispatchEvent(new CustomEvent('draft-selected', { detail: detail.name }));
      } catch (err) {
        const error = err as Error;
        window.toast.error('Failed to create draft');
      }
    },

    /**
     * Override selectRun to dispatch draft-selected for drafts
     */
    selectRunWithDraft(name: string, status: string) {
      this.selectedRun = name;
      this.selectedIndex = this.runs.findIndex(r => r.name === name);
      if (typeof Alpine !== 'undefined' && Alpine.store && Alpine.store('app')) {
        const store = Alpine.store('app');
        if (store) store.selectedRun = name;
      }
      window.dispatchEvent(new CustomEvent('run-selected', { detail: name }));

      // Also dispatch draft-selected if this is a draft
      if (status === 'draft') {
        window.dispatchEvent(new CustomEvent('draft-selected', { detail: name }));
      } else {
        // Clear draft selection when selecting a non-draft run
        window.dispatchEvent(new CustomEvent('draft-selected', { detail: null }));
      }
    },
  };
}
