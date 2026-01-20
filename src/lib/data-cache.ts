/**
 * Shared Data Cache - Centralized polling and data management
 *
 * This module provides a single source of truth for frequently-accessed data,
 * reducing redundant API calls across components. Components subscribe to
 * data changes via events instead of polling independently.
 */

import type {
  HistoryEntry,
  RunDetail,
  RunSummary,
  Task,
  ThreadSummary,
  WorkerDisplay,
} from './types';

// Event names for data updates
export const DATA_EVENTS = {
  RUNS_UPDATED: 'data:runs-updated',
  RUN_DETAIL_UPDATED: 'data:run-detail-updated',
  WORKERS_UPDATED: 'data:workers-updated',
  TASKS_UPDATED: 'data:tasks-updated',
  THREADS_UPDATED: 'data:threads-updated',
  HISTORY_UPDATED: 'data:history-updated',
} as const;

interface CacheState {
  runs: RunSummary[];
  runDetail: RunDetail | null;
  workers: WorkerDisplay[];
  tasks: Task[];
  threads: ThreadSummary[];
  history: HistoryEntry[];
  selectedRun: string | null;
  lastFetch: {
    runs: number;
    runDetail: number;
    workers: number;
    tasks: number;
    threads: number;
    history: number;
  };
}

class DataCache {
  private state: CacheState = {
    runs: [],
    runDetail: null,
    workers: [],
    tasks: [],
    threads: [],
    history: [],
    selectedRun: null,
    lastFetch: {
      runs: 0,
      runDetail: 0,
      workers: 0,
      tasks: 0,
      threads: 0,
      history: 0,
    },
  };

  private pollInterval: ReturnType<typeof setInterval> | null = null;
  private readonly POLL_INTERVAL_MS = 2000;
  private readonly STALE_THRESHOLD_MS = 1500; // Data older than this is considered stale

  // Subscribers count - only poll when there are active subscribers
  private subscriberCount = 0;

  /**
   * Start the shared polling loop
   */
  start(): void {
    if (this.pollInterval) return;

    // Initial fetch
    this.fetchRuns();

    this.pollInterval = setInterval(() => {
      if (this.subscriberCount > 0) {
        this.poll();
      }
    }, this.POLL_INTERVAL_MS);

    // Listen for run selection changes
    window.addEventListener('run-selected', (e: Event) => {
      const customEvent = e as CustomEvent<string | null>;
      this.setSelectedRun(customEvent.detail);
    });
  }

  /**
   * Stop polling (called on app destroy)
   */
  stop(): void {
    if (this.pollInterval) {
      clearInterval(this.pollInterval);
      this.pollInterval = null;
    }
  }

  /**
   * Subscribe to data updates - increments subscriber count
   */
  subscribe(): () => void {
    this.subscriberCount++;
    return () => {
      this.subscriberCount--;
    };
  }

  /**
   * Set the currently selected run
   */
  setSelectedRun(runName: string | null): void {
    if (this.state.selectedRun === runName) return;

    this.state.selectedRun = runName;

    // Clear run-specific data when selection changes
    this.state.runDetail = null;
    this.state.workers = [];
    this.state.tasks = [];
    this.state.threads = [];
    this.state.history = [];

    // Reset fetch timestamps
    this.state.lastFetch.runDetail = 0;
    this.state.lastFetch.workers = 0;
    this.state.lastFetch.tasks = 0;
    this.state.lastFetch.threads = 0;
    this.state.lastFetch.history = 0;

    // Immediately fetch data for new run
    if (runName) {
      this.fetchRunData();
    }
  }

  /**
   * Main poll function - fetches stale data
   */
  private async poll(): Promise<void> {
    const now = Date.now();

    // Always fetch runs list
    if (now - this.state.lastFetch.runs > this.STALE_THRESHOLD_MS) {
      this.fetchRuns();
    }

    // Fetch run-specific data if a run is selected
    if (this.state.selectedRun) {
      this.fetchRunData();
    }
  }

  /**
   * Fetch runs list
   */
  private async fetchRuns(): Promise<void> {
    if (!window.tauriInvoke) return;

    try {
      const runs = await window.tauriInvoke<RunSummary[]>('get_runs');
      // Filter out null/undefined entries
      const filtered = (runs || []).filter((r): r is RunSummary => r != null);
      this.state.runs = filtered;
      this.state.lastFetch.runs = Date.now();
      this.emit(DATA_EVENTS.RUNS_UPDATED, filtered);
    } catch (e) {
      console.error('[DataCache] Failed to fetch runs:', e);
    }
  }

  /**
   * Fetch all data for the selected run in parallel
   */
  private async fetchRunData(): Promise<void> {
    if (!window.tauriInvoke || !this.state.selectedRun) return;

    const runName = this.state.selectedRun;
    const now = Date.now();

    // Batch fetch all run-specific data in parallel
    const promises: Promise<void>[] = [];

    if (now - this.state.lastFetch.runDetail > this.STALE_THRESHOLD_MS) {
      promises.push(this.fetchRunDetail(runName));
    }
    if (now - this.state.lastFetch.workers > this.STALE_THRESHOLD_MS) {
      promises.push(this.fetchWorkers(runName));
    }
    if (now - this.state.lastFetch.tasks > this.STALE_THRESHOLD_MS) {
      promises.push(this.fetchTasks(runName));
    }
    if (now - this.state.lastFetch.threads > this.STALE_THRESHOLD_MS) {
      promises.push(this.fetchThreads(runName));
    }
    if (now - this.state.lastFetch.history > this.STALE_THRESHOLD_MS) {
      promises.push(this.fetchHistory(runName));
    }

    await Promise.all(promises);
  }

  private async fetchRunDetail(runName: string): Promise<void> {
    try {
      const detail = await window.tauriInvoke<RunDetail>('get_run_detail', { runName });
      if (this.state.selectedRun === runName) {
        this.state.runDetail = detail;
        this.state.lastFetch.runDetail = Date.now();
        this.emit(DATA_EVENTS.RUN_DETAIL_UPDATED, detail);
      }
    } catch (e) {
      console.error('[DataCache] Failed to fetch run detail:', e);
    }
  }

  private async fetchWorkers(runName: string): Promise<void> {
    try {
      const workers = await window.tauriInvoke<WorkerDisplay[]>('get_workers', { runName });
      if (this.state.selectedRun === runName) {
        // Filter out null/undefined entries
        const filtered = (workers || []).filter((w): w is WorkerDisplay => w != null);
        this.state.workers = filtered;
        this.state.lastFetch.workers = Date.now();
        this.emit(DATA_EVENTS.WORKERS_UPDATED, filtered);
      }
    } catch (e) {
      console.error('[DataCache] Failed to fetch workers:', e);
    }
  }

  private async fetchTasks(runName: string): Promise<void> {
    try {
      const tasks = await window.tauriInvoke<Task[]>('get_tasks', { runName });
      if (this.state.selectedRun === runName) {
        // Filter out null/undefined entries
        const filtered = (tasks || []).filter((t): t is Task => t != null);
        this.state.tasks = filtered;
        this.state.lastFetch.tasks = Date.now();
        this.emit(DATA_EVENTS.TASKS_UPDATED, filtered);
      }
    } catch (e) {
      console.error('[DataCache] Failed to fetch tasks:', e);
    }
  }

  private async fetchThreads(runName: string): Promise<void> {
    try {
      const threads = await window.tauriInvoke<ThreadSummary[]>('get_threads', { runName });
      if (this.state.selectedRun === runName) {
        // Filter out null/undefined entries
        const filtered = (threads || []).filter((t): t is ThreadSummary => t != null);
        this.state.threads = filtered;
        this.state.lastFetch.threads = Date.now();
        this.emit(DATA_EVENTS.THREADS_UPDATED, filtered);
      }
    } catch (e) {
      console.error('[DataCache] Failed to fetch threads:', e);
    }
  }

  private async fetchHistory(runName: string): Promise<void> {
    try {
      const history = await window.tauriInvoke<HistoryEntry[]>('get_history', {
        runName,
        limit: 100,
      });
      if (this.state.selectedRun === runName) {
        // Filter out null/undefined entries
        const filtered = (history || []).filter((h): h is HistoryEntry => h != null);
        this.state.history = filtered;
        this.state.lastFetch.history = Date.now();
        this.emit(DATA_EVENTS.HISTORY_UPDATED, filtered);
      }
    } catch (e) {
      console.error('[DataCache] Failed to fetch history:', e);
    }
  }

  /**
   * Emit a data update event
   */
  private emit<T>(eventName: string, data: T): void {
    window.dispatchEvent(new CustomEvent(eventName, { detail: data }));
  }

  // Public getters for current state (for initial render)
  getRuns(): RunSummary[] {
    return this.state.runs;
  }

  getRunDetail(): RunDetail | null {
    return this.state.runDetail;
  }

  getWorkers(): WorkerDisplay[] {
    return this.state.workers;
  }

  getTasks(): Task[] {
    return this.state.tasks;
  }

  getThreads(): ThreadSummary[] {
    return this.state.threads;
  }

  getHistory(): HistoryEntry[] {
    return this.state.history;
  }

  getSelectedRun(): string | null {
    return this.state.selectedRun;
  }

  /**
   * Force refresh specific data (useful after mutations)
   */
  async invalidateRuns(): Promise<void> {
    this.state.lastFetch.runs = 0;
    await this.fetchRuns();
  }

  async invalidateRunDetail(): Promise<void> {
    if (this.state.selectedRun) {
      this.state.lastFetch.runDetail = 0;
      await this.fetchRunDetail(this.state.selectedRun);
    }
  }

  async invalidateWorkers(): Promise<void> {
    if (this.state.selectedRun) {
      this.state.lastFetch.workers = 0;
      await this.fetchWorkers(this.state.selectedRun);
    }
  }

  async invalidateTasks(): Promise<void> {
    if (this.state.selectedRun) {
      this.state.lastFetch.tasks = 0;
      await this.fetchTasks(this.state.selectedRun);
    }
  }

  async invalidateThreads(): Promise<void> {
    if (this.state.selectedRun) {
      this.state.lastFetch.threads = 0;
      await this.fetchThreads(this.state.selectedRun);
    }
  }

  async invalidateHistory(): Promise<void> {
    if (this.state.selectedRun) {
      this.state.lastFetch.history = 0;
      await this.fetchHistory(this.state.selectedRun);
    }
  }
}

// Singleton instance
export const dataCache = new DataCache();
