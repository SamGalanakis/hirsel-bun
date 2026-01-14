/**
 * Run List Alpine.js Component
 *
 * Displays the list of hirsel runs in the left sidebar panel.
 * Handles run selection, keyboard navigation, and context menu actions.
 */

import { getRuns, pauseRun, resumeRun, deleteRun, deliverRun } from '../lib/api';
import type { RunSummary, RunStatus } from '../lib/types';

/**
 * Status color class mapping for the status dot
 */
const STATUS_DOT_CLASS: Record<RunStatus, string> = {
  draft: 'status-idle',
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

/**
 * Status display text
 */
const STATUS_LABEL: Record<RunStatus, string> = {
  draft: 'Draft',
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
 * Format elapsed time in a human-readable way
 */
function formatElapsed(minutes: number | null | undefined): string {
  if (!minutes) return '0m';
  if (minutes < 60) return `${minutes}m`;
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return m > 0 ? `${h}h ${m}m` : `${h}h`;
}

/**
 * Format progress as percentage
 */
function formatProgress(done: number, total: number): number {
  if (total === 0) return 0;
  return Math.round((done / total) * 100);
}

/**
 * Run list component data and methods
 */
export interface RunListData {
  runs: RunSummary[];
  selectedRun: string | null;
  selectedIndex: number;
  loading: boolean;
  error: string | null;
  contextMenuVisible: boolean;
  contextMenuX: number;
  contextMenuY: number;
  contextMenuRun: string | null;
  pollInterval: ReturnType<typeof setInterval> | null;
}

/**
 * Alpine.js component factory for the run list
 */
export function runList(): RunListData & {
  init(): Promise<void>;
  destroy(): void;
  fetchRuns(): Promise<void>;
  selectRun(name: string): void;
  selectByIndex(index: number): void;
  navigateUp(): void;
  navigateDown(): void;
  getStatusDotClass(status: RunStatus): string;
  getStatusLabel(status: RunStatus): string;
  formatElapsed(minutes: number | null | undefined): string;
  formatProgress(done: number, total: number): number;
  showContextMenu(event: MouseEvent, runName: string): void;
  hideContextMenu(): void;
  contextPause(): Promise<void>;
  contextResume(): Promise<void>;
  contextDelete(): Promise<void>;
  contextDeliver(): Promise<void>;
} {
  return {
    runs: [],
    selectedRun: null,
    selectedIndex: -1,
    loading: true,
    error: null,
    contextMenuVisible: false,
    contextMenuX: 0,
    contextMenuY: 0,
    contextMenuRun: null,
    pollInterval: null,

    /**
     * Initialize the component, fetch runs, and set up polling
     */
    async init(): Promise<void> {
      await this.fetchRuns();

      // Poll for updates every 2 seconds
      this.pollInterval = setInterval(() => {
        this.fetchRuns();
      }, 2000);

      // Listen for keyboard events when this panel has focus
      document.addEventListener('keydown', (e: KeyboardEvent) => {
        // Only handle if not typing in an input
        const target = e.target as HTMLElement;
        if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') return;

        switch (e.key) {
          case 'j':
          case 'ArrowDown':
            this.navigateDown();
            e.preventDefault();
            break;
          case 'k':
          case 'ArrowUp':
            this.navigateUp();
            e.preventDefault();
            break;
          case 'Enter':
            if (this.selectedIndex >= 0 && this.runs[this.selectedIndex]) {
              this.selectRun(this.runs[this.selectedIndex].name);
            }
            break;
          case 'Escape':
            this.hideContextMenu();
            break;
        }
      });

      // Hide context menu on click outside
      document.addEventListener('click', () => {
        this.hideContextMenu();
      });

      // Listen for run selection events from other components
      window.addEventListener('run-selected', ((e: CustomEvent<string>) => {
        this.selectedRun = e.detail;
        const index = this.runs.findIndex(r => r.name === e.detail);
        if (index >= 0) this.selectedIndex = index;
      }) as EventListener);
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
     * Fetch runs from the backend
     */
    async fetchRuns(): Promise<void> {
      try {
        const runs = await getRuns();
        // Sort by status (active first) then by elapsed time (most recent first)
        this.runs = runs.sort((a, b) => {
          // Active statuses come first
          const activeStatuses = ['working', 'eval', 'waiting', 'paused'];
          const aActive = activeStatuses.includes(a.status);
          const bActive = activeStatuses.includes(b.status);
          if (aActive && !bActive) return -1;
          if (!aActive && bActive) return 1;
          // Then sort by elapsed time (descending)
          return (b.elapsedMinutes || 0) - (a.elapsedMinutes || 0);
        });
        this.loading = false;
        this.error = null;

        // Update selected index if selected run is still in list
        if (this.selectedRun) {
          const index = this.runs.findIndex(r => r.name === this.selectedRun);
          if (index >= 0) {
            this.selectedIndex = index;
          } else {
            // Run was deleted, clear selection
            this.selectedRun = null;
            this.selectedIndex = -1;
          }
        }
      } catch (err) {
        this.error = err instanceof Error ? err.message : String(err);
        this.loading = false;
      }
    },

    /**
     * Select a run by name
     */
    selectRun(name: string): void {
      this.selectedRun = name;
      this.selectedIndex = this.runs.findIndex(r => r.name === name);

      // Dispatch event for other components
      window.dispatchEvent(new CustomEvent('run-selected', { detail: name }));

      // Update global Alpine store if available
      if (typeof Alpine !== 'undefined' && Alpine.store) {
        const store = Alpine.store('app') as { selectedRun?: string } | undefined;
        if (store) {
          store.selectedRun = name;
        }
      }
    },

    /**
     * Select run by index
     */
    selectByIndex(index: number): void {
      if (index >= 0 && index < this.runs.length) {
        this.selectRun(this.runs[index].name);
      }
    },

    /**
     * Navigate to the previous run in the list
     */
    navigateUp(): void {
      if (this.runs.length === 0) return;
      const newIndex = this.selectedIndex <= 0 ? this.runs.length - 1 : this.selectedIndex - 1;
      this.selectByIndex(newIndex);
    },

    /**
     * Navigate to the next run in the list
     */
    navigateDown(): void {
      if (this.runs.length === 0) return;
      const newIndex = this.selectedIndex >= this.runs.length - 1 ? 0 : this.selectedIndex + 1;
      this.selectByIndex(newIndex);
    },

    /**
     * Get CSS class for status dot
     */
    getStatusDotClass(status: RunStatus): string {
      return STATUS_DOT_CLASS[status] || 'status-idle';
    },

    /**
     * Get display label for status
     */
    getStatusLabel(status: RunStatus): string {
      return STATUS_LABEL[status] || status;
    },

    /**
     * Format elapsed time
     */
    formatElapsed,

    /**
     * Format progress percentage
     */
    formatProgress,

    /**
     * Show context menu for a run
     */
    showContextMenu(event: MouseEvent, runName: string): void {
      event.preventDefault();
      event.stopPropagation();
      this.contextMenuX = event.clientX;
      this.contextMenuY = event.clientY;
      this.contextMenuRun = runName;
      this.contextMenuVisible = true;
    },

    /**
     * Hide context menu
     */
    hideContextMenu(): void {
      this.contextMenuVisible = false;
      this.contextMenuRun = null;
    },

    /**
     * Pause the context menu run
     */
    async contextPause(): Promise<void> {
      if (!this.contextMenuRun) return;
      try {
        await pauseRun(this.contextMenuRun);
        await this.fetchRuns();
      } catch (err) {
        console.error('Failed to pause run:', err);
      }
      this.hideContextMenu();
    },

    /**
     * Resume the context menu run
     */
    async contextResume(): Promise<void> {
      if (!this.contextMenuRun) return;
      try {
        await resumeRun(this.contextMenuRun);
        await this.fetchRuns();
      } catch (err) {
        console.error('Failed to resume run:', err);
      }
      this.hideContextMenu();
    },

    /**
     * Delete the context menu run
     */
    async contextDelete(): Promise<void> {
      if (!this.contextMenuRun) return;
      // Confirm before deleting
      if (!confirm(`Delete run "${this.contextMenuRun}"? This cannot be undone.`)) {
        this.hideContextMenu();
        return;
      }
      try {
        await deleteRun(this.contextMenuRun);
        // Clear selection if deleted run was selected
        if (this.selectedRun === this.contextMenuRun) {
          this.selectedRun = null;
          this.selectedIndex = -1;
          window.dispatchEvent(new CustomEvent('run-selected', { detail: null }));
        }
        await this.fetchRuns();
      } catch (err) {
        console.error('Failed to delete run:', err);
      }
      this.hideContextMenu();
    },

    /**
     * Deliver the context menu run
     */
    async contextDeliver(): Promise<void> {
      if (!this.contextMenuRun) return;
      try {
        const branchName = await deliverRun(this.contextMenuRun);
        alert(`Delivered to branch: ${branchName}`);
        await this.fetchRuns();
      } catch (err) {
        console.error('Failed to deliver run:', err);
        alert(`Failed to deliver: ${err instanceof Error ? err.message : err}`);
      }
      this.hideContextMenu();
    },
  };
}

/**
 * Register the component with Alpine.js
 *
 * Call this in your main script to make runList() available globally.
 */
export function registerRunListComponent(): void {
  if (typeof window !== 'undefined') {
    (window as unknown as Record<string, unknown>).runList = runList;
  }
}

// Auto-register if Alpine is already loaded
if (typeof window !== 'undefined' && typeof Alpine !== 'undefined') {
  registerRunListComponent();
}

// Declare Alpine global for TypeScript
declare const Alpine: {
  store: (name: string) => Record<string, unknown> | undefined;
};
