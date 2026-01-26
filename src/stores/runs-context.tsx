/**
 * Runs context - replaces the Alpine DataCache with SolidJS reactivity
 */
import { invoke } from '@tauri-apps/api/core';
import {
  type ParentComponent,
  createContext,
  createEffect,
  createSignal,
  onCleanup,
  useContext,
} from 'solid-js';
import { createStore, reconcile } from 'solid-js/store';
import type {
  DeliveryState,
  HistoryEntry,
  MergeInfo,
  PrInfo,
  PushResult,
  RunDetail,
  RunSummary,
  Task,
  ThreadSummary,
  WorkerDisplay,
} from '../lib/types';

interface RunsState {
  runs: RunSummary[];
  runDetail: RunDetail | null;
  workers: WorkerDisplay[];
  tasks: Task[];
  threads: ThreadSummary[];
  history: HistoryEntry[];
}

interface RunsContextValue {
  // Data
  runs: () => RunSummary[];
  runDetail: () => RunDetail | null;
  workers: () => WorkerDisplay[];
  tasks: () => Task[];
  threads: () => ThreadSummary[];
  history: () => HistoryEntry[];

  // Selection
  selectedRun: () => string | null;
  setSelectedRun: (runName: string | null) => void;

  // Loading states
  loading: () => boolean;

  // Invalidation
  invalidateRuns: () => Promise<void>;
  invalidateRunDetail: () => Promise<void>;
  invalidateWorkers: () => Promise<void>;
  invalidateTasks: () => Promise<void>;
  invalidateThreads: () => Promise<void>;
  invalidateHistory: () => Promise<void>;

  // Subscribe/unsubscribe for polling
  subscribe: () => () => void;

  // Delivery state
  deliveryState: () => DeliveryState | null;
  deliveryLoading: () => boolean;
  refreshDeliveryState: (runName: string, targetBranch: string) => Promise<void>;

  // Delivery actions
  pushBranch: (runName: string) => Promise<PushResult>;
  createPr: (runName: string, targetBranch: string, title: string, body: string) => Promise<PrInfo>;
  autoMerge: (runName: string, targetBranch: string, title: string, body: string) => Promise<MergeInfo>;
  abandon: (runName: string) => Promise<void>;
  markDelivered: (runName: string) => Promise<void>;

  // Delivery modal state
  showDeliveryModal: () => boolean;
  deliveryModalRun: () => string | null;
  deliveryModalTargetBranch: () => string | null;
  openDeliveryModal: (runName: string, targetBranch: string) => void;
  closeDeliveryModal: () => void;
}

const RunsContext = createContext<RunsContextValue>();

const POLL_INTERVAL_MS = 2000;

export const RunsProvider: ParentComponent = (props) => {
  const [state, setState] = createStore<RunsState>({
    runs: [],
    runDetail: null,
    workers: [],
    tasks: [],
    threads: [],
    history: [],
  });

  const [selectedRun, setSelectedRunSignal] = createSignal<string | null>(null);
  const [loading, setLoading] = createSignal(true);
  const [subscriberCount, setSubscriberCount] = createSignal(0);

  // Delivery state
  const [deliveryState, setDeliveryState] = createSignal<DeliveryState | null>(null);
  const [deliveryLoading, setDeliveryLoading] = createSignal(false);

  // Delivery modal state
  const [showDeliveryModal, setShowDeliveryModal] = createSignal(false);
  const [deliveryModalRun, setDeliveryModalRun] = createSignal<string | null>(null);
  const [deliveryModalTargetBranch, setDeliveryModalTargetBranch] = createSignal<string | null>(null);

  // Fetch runs list
  const fetchRuns = async () => {
    try {
      const runs = await invoke<RunSummary[]>('get_runs');
      const filtered = (runs || []).filter((r): r is RunSummary => r != null);
      setState('runs', reconcile(filtered));
      setLoading(false);

      // Check if selected run still exists
      const currentRun = selectedRun();
      if (currentRun && !filtered.some((r) => r.name === currentRun)) {
        handleRunDeleted(currentRun);
      }
    } catch (e) {
      console.error('[RunsContext] Failed to fetch runs:', e);
    }
  };

  // Fetch run-specific data
  const fetchRunDetail = async (runName: string) => {
    try {
      const detail = await invoke<RunDetail>('get_run_detail', { runName });
      if (selectedRun() === runName) {
        setState('runDetail', detail);
      }
    } catch {
      // Errors likely because run was deleted
    }
  };

  const fetchWorkers = async (runName: string) => {
    try {
      const workers = await invoke<WorkerDisplay[]>('get_workers', { runName });
      if (selectedRun() === runName) {
        const filtered = (workers || []).filter((w): w is WorkerDisplay => w != null);
        setState('workers', reconcile(filtered));
      }
    } catch {
      // Errors likely because run was deleted
    }
  };

  const fetchTasks = async (runName: string) => {
    try {
      const tasks = await invoke<Task[]>('get_tasks', { runName });
      if (selectedRun() === runName) {
        const filtered = (tasks || []).filter((t): t is Task => t != null);
        setState('tasks', reconcile(filtered));
      }
    } catch {
      // Errors likely because run was deleted
    }
  };

  const fetchThreads = async (runName: string) => {
    try {
      const threads = await invoke<ThreadSummary[]>('get_threads', { runName });
      if (selectedRun() === runName) {
        const filtered = (threads || []).filter((t): t is ThreadSummary => t != null);
        setState('threads', reconcile(filtered));
      }
    } catch {
      // Errors likely because run was deleted
    }
  };

  const fetchHistory = async (runName: string) => {
    try {
      const history = await invoke<HistoryEntry[]>('get_history', { runName, limit: 100 });
      if (selectedRun() === runName) {
        const filtered = (history || []).filter((h): h is HistoryEntry => h != null);
        setState('history', reconcile(filtered));
      }
    } catch {
      // Errors likely because run was deleted
    }
  };

  const fetchRunData = async (runName: string) => {
    await Promise.all([
      fetchRunDetail(runName),
      fetchWorkers(runName),
      fetchTasks(runName),
      fetchThreads(runName),
      fetchHistory(runName),
    ]);
  };

  const handleRunDeleted = (deletedRunName: string) => {
    console.log(`[RunsContext] Run '${deletedRunName}' was deleted, clearing selection`);
    setSelectedRunSignal(null);
    setState({
      runDetail: null,
      workers: [],
      tasks: [],
      threads: [],
      history: [],
    });
    window.dispatchEvent(new CustomEvent('run-selected', { detail: null }));
    window.dispatchEvent(new CustomEvent('draft-selected', { detail: null }));
  };

  const setSelectedRun = (runName: string | null) => {
    const previousRun = selectedRun();
    if (previousRun === runName) return;

    setSelectedRunSignal(runName);

    // Clear run-specific data when selection changes
    setState({
      runDetail: null,
      workers: [],
      tasks: [],
      threads: [],
      history: [],
    });

    // Fetch data for new run
    if (runName) {
      fetchRunData(runName);
    }

    // Dispatch events
    window.dispatchEvent(new CustomEvent('run-selected', { detail: runName }));

    // Handle draft selection
    const run = state.runs.find((r) => r.name === runName);
    if (run?.status === 'draft') {
      window.dispatchEvent(new CustomEvent('draft-selected', { detail: runName }));
    } else {
      window.dispatchEvent(new CustomEvent('draft-selected', { detail: null }));
    }
  };

  // Polling
  createEffect(() => {
    // Initial fetch
    fetchRuns();

    // Set up polling
    const interval = setInterval(() => {
      if (subscriberCount() > 0) {
        fetchRuns();
        const currentRun = selectedRun();
        if (currentRun) {
          fetchRunData(currentRun);
        }
      }
    }, POLL_INTERVAL_MS);

    onCleanup(() => clearInterval(interval));
  });

  // Listen for run-selected events from other sources
  createEffect(() => {
    const handler = (e: Event) => {
      const customEvent = e as CustomEvent<string | null>;
      const runName = customEvent.detail;
      // Only update if different (to avoid loops)
      if (runName !== selectedRun()) {
        setSelectedRunSignal(runName);
        if (runName) {
          fetchRunData(runName);
        } else {
          setState({
            runDetail: null,
            workers: [],
            tasks: [],
            threads: [],
            history: [],
          });
        }
      }
    };

    window.addEventListener('run-selected', handler);
    onCleanup(() => window.removeEventListener('run-selected', handler));
  });

  const subscribe = () => {
    setSubscriberCount((c) => c + 1);
    return () => setSubscriberCount((c) => c - 1);
  };

  // Delivery methods
  const refreshDeliveryState = async (runName: string, targetBranch: string) => {
    setDeliveryLoading(true);
    try {
      const state = await invoke<DeliveryState>('get_delivery_state', {
        runName,
        targetBranch,
      });
      setDeliveryState(state);
    } catch (e) {
      console.error('Failed to get delivery state:', e);
      setDeliveryState(null);
    } finally {
      setDeliveryLoading(false);
    }
  };

  const pushBranch = async (runName: string): Promise<PushResult> => {
    const result = await invoke<PushResult>('push_run_branch', { runName });
    return result;
  };

  const createPr = async (
    runName: string,
    targetBranch: string,
    title: string,
    body: string
  ): Promise<PrInfo> => {
    const result = await invoke<PrInfo>('create_run_pr', {
      runName,
      targetBranch,
      title,
      body,
    });
    return result;
  };

  const autoMerge = async (
    runName: string,
    targetBranch: string,
    title: string,
    body: string
  ): Promise<MergeInfo> => {
    const result = await invoke<MergeInfo>('auto_merge_run', {
      runName,
      targetBranch,
      title,
      body,
    });
    return result;
  };

  const abandon = async (runName: string): Promise<void> => {
    // Backend command not yet available - stub for now
    console.warn('abandon not yet implemented');
  };

  const markDelivered = async (runName: string): Promise<void> => {
    // Backend command not yet available - stub for now
    console.warn('markDelivered not yet implemented');
  };

  // Delivery modal controls
  const openDeliveryModal = (runName: string, targetBranch: string) => {
    setDeliveryModalRun(runName);
    setDeliveryModalTargetBranch(targetBranch);
    setShowDeliveryModal(true);
  };

  const closeDeliveryModal = () => {
    setShowDeliveryModal(false);
    setDeliveryModalRun(null);
    setDeliveryModalTargetBranch(null);
    setDeliveryState(null);
  };

  const value: RunsContextValue = {
    runs: () => state.runs,
    runDetail: () => state.runDetail,
    workers: () => state.workers,
    tasks: () => state.tasks,
    threads: () => state.threads,
    history: () => state.history,
    selectedRun,
    setSelectedRun,
    loading,
    invalidateRuns: fetchRuns,
    invalidateRunDetail: () => {
      const run = selectedRun();
      return run ? fetchRunDetail(run) : Promise.resolve();
    },
    invalidateWorkers: () => {
      const run = selectedRun();
      return run ? fetchWorkers(run) : Promise.resolve();
    },
    invalidateTasks: () => {
      const run = selectedRun();
      return run ? fetchTasks(run) : Promise.resolve();
    },
    invalidateThreads: () => {
      const run = selectedRun();
      return run ? fetchThreads(run) : Promise.resolve();
    },
    invalidateHistory: () => {
      const run = selectedRun();
      return run ? fetchHistory(run) : Promise.resolve();
    },
    subscribe,

    // Delivery state
    deliveryState,
    deliveryLoading,
    refreshDeliveryState,

    // Delivery actions
    pushBranch,
    createPr,
    autoMerge,
    abandon,
    markDelivered,

    // Delivery modal
    showDeliveryModal,
    deliveryModalRun,
    deliveryModalTargetBranch,
    openDeliveryModal,
    closeDeliveryModal,
  };

  return <RunsContext.Provider value={value}>{props.children}</RunsContext.Provider>;
};

export function useRuns(): RunsContextValue {
  const context = useContext(RunsContext);
  if (!context) {
    throw new Error('useRuns must be used within a RunsProvider');
  }
  return context;
}
