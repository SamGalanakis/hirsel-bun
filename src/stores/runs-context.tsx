/**
 * Runs context - manages run list, run detail, workers, threads, history
 *
 * Centralizes all run-related polling so components can read from the store
 * instead of polling independently.
 */
import { invoke } from '../lib/invoke';
import { emit, on } from '../lib/events';
import { createPoll } from '../lib/poll';
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
  HistoryEntry,
  RunDetail,
  RunSummary,
  ThreadSummary,
  WorkerDisplay,
} from '../lib/types';

interface RunsState {
  runs: RunSummary[];
  runDetail: RunDetail | null;
  workers: WorkerDisplay[];
  threads: ThreadSummary[];
  history: HistoryEntry[];
}

interface RunsContextValue {
  // Data
  runs: () => RunSummary[];
  runDetail: () => RunDetail | null;
  workers: () => WorkerDisplay[];
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
  invalidateThreads: () => Promise<void>;
  invalidateHistory: () => Promise<void>;

  // Subscribe/unsubscribe for polling
  subscribe: () => () => void;
}

const RunsContext = createContext<RunsContextValue>();

/** Active polling interval — 5s when runs are active, 15s when idle */
const ACTIVE_POLL_MS = 5000;
const IDLE_POLL_MS = 15000;

export const RunsProvider: ParentComponent = (props) => {
  const [state, setState] = createStore<RunsState>({
    runs: [],
    runDetail: null,
    workers: [],
    threads: [],
    history: [],
  });

  const [selectedRun, setSelectedRunSignal] = createSignal<string | null>(null);
  const [loading, setLoading] = createSignal(true);
  const [subscriberCount, setSubscriberCount] = createSignal(0);
  const [runsGeneration, setRunsGeneration] = createSignal(0);

  /** Whether any run is in an active state (working/eval) */
  const hasActiveRuns = () =>
    state.runs.some((r) => r.status === 'working' || r.status === 'eval');

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
      threads: [],
      history: [],
    });
    emit('run-selected', null);
    emit('draft-selected', null);
  };

  const setSelectedRun = (runName: string | null) => {
    const previousRun = selectedRun();
    if (previousRun === runName) return;

    setSelectedRunSignal(runName);

    // Clear run-specific data when selection changes
    setState({
      runDetail: null,
      workers: [],
      threads: [],
      history: [],
    });

    // Fetch data for new run
    if (runName) {
      fetchRunData(runName);
    }

    // Dispatch events
    emit('run-selected', runName);

    // Handle draft selection
    const run = state.runs.find((r) => r.name === runName);
    if (run?.status === 'draft') {
      emit('draft-selected', runName);
    } else {
      emit('draft-selected', null);
    }
  };

  // Generation-aware poll for runs list
  const pollRuns = async () => {
    try {
      const result = await invoke<[RunSummary[], number] | null>('get_runs_if_changed', {
        lastGeneration: runsGeneration(),
      });
      if (result === null) return; // No changes
      const [runs, gen] = result;
      const filtered = (runs || []).filter((r): r is RunSummary => r != null);
      setState('runs', reconcile(filtered));
      setRunsGeneration(gen);
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

  // Polling — adaptive interval based on active run state
  createEffect(() => {
    const interval = hasActiveRuns() ? ACTIVE_POLL_MS : IDLE_POLL_MS;

    createPoll(
      async () => {
        if (subscriberCount() <= 0) return;
        await pollRuns();
        const currentRun = selectedRun();
        if (currentRun) {
          await fetchRunData(currentRun);
        }
      },
      { interval, immediate: true },
    );
  });

  // Listen for run-selected events from other sources
  createEffect(() => {
    const cleanup = on('run-selected', (runName) => {
      // Only update if different (to avoid loops)
      if (runName !== selectedRun()) {
        setSelectedRunSignal(runName);
        if (runName) {
          fetchRunData(runName);
        } else {
          setState({
            runDetail: null,
            workers: [],
            threads: [],
            history: [],
          });
        }
      }
    });

    onCleanup(cleanup);
  });

  const subscribe = () => {
    setSubscriberCount((c) => c + 1);
    return () => setSubscriberCount((c) => c - 1);
  };

  const value: RunsContextValue = {
    runs: () => state.runs,
    runDetail: () => state.runDetail,
    workers: () => state.workers,
    threads: () => state.threads,
    history: () => state.history,
    selectedRun,
    setSelectedRun,
    loading,
    invalidateRuns: () => {
      setRunsGeneration(0);
      return fetchRuns();
    },
    invalidateRunDetail: () => {
      const run = selectedRun();
      return run ? fetchRunDetail(run) : Promise.resolve();
    },
    invalidateWorkers: () => {
      const run = selectedRun();
      return run ? fetchWorkers(run) : Promise.resolve();
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
