/**
 * Run list sidebar Alpine component
 */

import { cloneRun, createDraft } from '../api';
import { DATA_EVENTS, dataCache } from '../data-cache';
import type { RunSummary } from '../types';
import { formatElapsed, formatProgress, formatRelativeTime } from '../utils/formatters';
import { getProgressBarClass, getStatusBadgeClass, getStatusLabel } from '../utils/status';

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
    contextMenuStatus: null as string | null,
    // Clone dialog state
    cloneDialogVisible: false,
    cloneSourceRun: null as string | null,
    cloneNewName: '',
    cloneLoading: false,
    _eventCleanups: [] as (() => void)[],
    _cacheUnsubscribe: null as (() => void) | null,

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
      const completedStatuses = [
        'done',
        'delivered',
        'merged',
        'timed_out',
        'eval_failed',
        'runaway',
      ];

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
      if (remaining < 60) return `${remaining}m left`;
      const h = Math.floor(remaining / 60);
      const m = remaining % 60;
      return `${h}h${m > 0 ? ` ${m}m` : ''} left`;
    },

    async init() {
      // Subscribe to shared cache
      this._cacheUnsubscribe = dataCache.subscribe();

      // Listen for runs updates from cache
      const runsUpdatedHandler = (e: Event) => {
        const customEvent = e as CustomEvent<RunSummary[]>;
        this.runs = customEvent.detail;
        this.loading = false;
        this.error = null;

        // Auto-select first run if none selected
        if (this.runs.length > 0 && !this.selectedRun) {
          setTimeout(() => {
            if (!this.selectedRun && this.runs.length > 0) {
              const firstRun = this.runs[0];
              this.selectRunWithDraft(firstRun.name, firstRun.status);
            }
          }, 50);
        }
      };
      window.addEventListener(DATA_EVENTS.RUNS_UPDATED, runsUpdatedHandler);
      this._eventCleanups.push(() =>
        window.removeEventListener(DATA_EVENTS.RUNS_UPDATED, runsUpdatedHandler),
      );

      // Get initial data from cache
      const cachedRuns = dataCache.getRuns();
      if (cachedRuns.length > 0) {
        this.runs = cachedRuns;
        this.loading = false;
      }

      // Listen for run selection
      const runSelectedHandler = (e: Event) => {
        const customEvent = e as CustomEvent<string | null>;
        this.selectedRun = customEvent.detail;
        if (customEvent.detail) {
          const index = this.runs.findIndex((r) => r.name === customEvent.detail);
          if (index >= 0) this.selectedIndex = index;
        } else {
          // Selection cleared (e.g., run was deleted)
          this.selectedIndex = -1;
        }
      };
      window.addEventListener('run-selected', runSelectedHandler);
      this._eventCleanups.push(() =>
        window.removeEventListener('run-selected', runSelectedHandler),
      );

      // Close context menu on click
      const clickHandler = () => {
        this.hideContextMenu();
      };
      document.addEventListener('click', clickHandler);
      this._eventCleanups.push(() => document.removeEventListener('click', clickHandler));
    },

    destroy() {
      this._eventCleanups.forEach((fn) => fn());
      this._eventCleanups = [];
      if (this._cacheUnsubscribe) {
        this._cacheUnsubscribe();
        this._cacheUnsubscribe = null;
      }
    },

    selectRun(name: string) {
      this.selectedRun = name;
      this.selectedIndex = this.runs.findIndex((r) => r.name === name);
      if (typeof Alpine !== 'undefined' && Alpine.store && Alpine.store('app')) {
        const store = Alpine.store('app');
        if (store) store.selectedRun = name;
      }
      window.dispatchEvent(new CustomEvent('run-selected', { detail: name }));
    },

    // Context menu
    showContextMenu(event: MouseEvent, run: RunSummary) {
      event.preventDefault();
      event.stopPropagation();
      this.contextMenuX = event.clientX;
      this.contextMenuY = event.clientY;
      this.contextMenuRun = run.name;
      this.contextMenuStatus = run.status;
      this.contextMenuVisible = true;
    },

    hideContextMenu() {
      this.contextMenuVisible = false;
      this.contextMenuRun = null;
      this.contextMenuStatus = null;
    },

    // Context menu visibility helpers
    canPause(): boolean {
      const status = this.contextMenuStatus;
      // Can pause active runs (working, idle, waiting, eval)
      return ['working', 'idle', 'waiting', 'eval'].includes(status || '');
    },

    canResume(): boolean {
      const status = this.contextMenuStatus;
      // Can resume paused runs
      return status === 'paused';
    },

    canDeliver(): boolean {
      const status = this.contextMenuStatus;
      // Can deliver completed runs (done, but not already delivered/merged)
      // Also allow delivering paused runs
      return ['done', 'paused', 'working', 'idle', 'waiting', 'timed_out'].includes(status || '');
    },

    canClone(): boolean {
      // Can clone any run
      return this.contextMenuRun !== null;
    },

    isDraftRun(): boolean {
      return this.contextMenuStatus === 'draft';
    },

    async contextPause() {
      if (!this.contextMenuRun) return;
      try {
        if (window.tauriInvoke) {
          await window.tauriInvoke('pause_run', { runName: this.contextMenuRun });
          await dataCache.invalidateRuns();
        }
      } catch (err) {
        window.toast.error('Failed to pause run');
      }
      this.hideContextMenu();
    },

    async contextResume() {
      if (!this.contextMenuRun) return;
      try {
        if (window.tauriInvoke) {
          await window.tauriInvoke('resume_run', { runName: this.contextMenuRun });
          await dataCache.invalidateRuns();
        }
      } catch (err) {
        window.toast.error('Failed to resume run');
      }
      this.hideContextMenu();
    },

    async contextDelete() {
      if (!this.contextMenuRun) return;
      const runToDelete = this.contextMenuRun;
      this.hideContextMenu();

      const confirmed =
        (await window.confirmDialog?.delete(runToDelete, 'run')) ??
        confirm(`Delete run "${runToDelete}"?`);
      if (!confirmed) return;

      try {
        if (window.tauriInvoke) {
          await window.tauriInvoke('delete_run', { runName: runToDelete });
          if (this.selectedRun === runToDelete) {
            this.selectedRun = null;
            this.selectedIndex = -1;
            window.dispatchEvent(new CustomEvent('run-selected', { detail: null }));
          }
          await dataCache.invalidateRuns();
        }
      } catch (err) {
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
          await dataCache.invalidateRuns();
        }
      } catch (err) {
        window.toast.error('Failed to deliver');
      }
      this.hideContextMenu();
    },

    /**
     * Show clone dialog for context menu run
     */
    showCloneDialog() {
      if (!this.contextMenuRun) return;
      this.cloneSourceRun = this.contextMenuRun;
      this.cloneNewName = `${this.contextMenuRun}-copy`;
      this.cloneDialogVisible = true;
      this.hideContextMenu();

      // Focus the input after dialog opens
      setTimeout(() => {
        const input = document.getElementById('clone-name-input') as HTMLInputElement;
        if (input) {
          input.focus();
          input.select();
        }
      }, 100);
    },

    /**
     * Hide clone dialog
     */
    hideCloneDialog() {
      this.cloneDialogVisible = false;
      this.cloneSourceRun = null;
      this.cloneNewName = '';
      this.cloneLoading = false;
    },

    /**
     * Execute the clone operation
     */
    async executeClone() {
      if (!this.cloneSourceRun || !this.cloneNewName.trim()) return;

      this.cloneLoading = true;
      try {
        const detail = await cloneRun(this.cloneSourceRun, this.cloneNewName.trim());
        window.toast.success(`Cloned to "${detail.name}"`);

        // Refresh runs list via cache
        await dataCache.invalidateRuns();

        // Select the new draft
        this.selectRun(detail.name);

        // Dispatch draft-selected event for the draft editor
        window.dispatchEvent(new CustomEvent('draft-selected', { detail: detail.name }));

        this.hideCloneDialog();
      } catch (err) {
        // Tauri returns error strings directly, not Error objects
        const message =
          typeof err === 'string' ? err : (err as Error).message || 'Failed to clone run';
        window.toast.error(message, 'Failed to clone run');
        this.cloneLoading = false;
      }
    },

    /**
     * Create a new draft run and select it
     */
    async createNewDraft() {
      try {
        const detail = await createDraft();
        window.toast.success(`Draft "${detail.name}" created`);

        // Refresh runs list via cache
        await dataCache.invalidateRuns();

        // Select the new draft
        this.selectRun(detail.name);

        // Notify draft editor this is a newly created draft (for edit mode)
        window.dispatchEvent(new CustomEvent('draft-created'));
        // Dispatch draft-selected event for the draft editor
        window.dispatchEvent(new CustomEvent('draft-selected', { detail: detail.name }));
      } catch (err) {
        window.toast.error('Failed to create draft');
      }
    },

    /**
     * Override selectRun to dispatch draft-selected for drafts
     */
    selectRunWithDraft(name: string, status: string) {
      this.selectedRun = name;
      this.selectedIndex = this.runs.findIndex((r) => r.name === name);
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
