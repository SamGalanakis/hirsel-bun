/**
 * Dispatch context for managing board-to-run dispatch workflow
 */
import { invoke } from '@tauri-apps/api/core';
import {
  type ParentComponent,
  createContext,
  createSignal,
  useContext,
} from 'solid-js';
import type {
  DispatchPreview,
  DispatchInfo,
  TaskRun,
  DispatchConfig,
} from '../lib/types';

interface DispatchContextValue {
  // Preview
  preview: () => DispatchPreview | null;
  isLoadingPreview: () => boolean;
  loadPreview: (projectId: number, taskId: string) => Promise<void>;
  clearPreview: () => void;

  // Dispatch
  isDispatching: () => boolean;
  prepareDispatch: (
    projectId: number,
    taskId: string,
    config?: DispatchConfig
  ) => Promise<DispatchInfo>;
  recordDispatch: (
    projectId: number,
    taskId: string,
    runName: string
  ) => Promise<void>;

  // Task runs
  taskRuns: () => Map<string, TaskRun[]>;
  isLoadingTaskRuns: () => boolean;
  loadTaskRuns: (projectId: number, taskId: string) => Promise<TaskRun[]>;
  loadAllTaskRuns: (projectId: number) => Promise<void>;
  refreshTaskRuns: (projectId: number) => Promise<void>;
  getRunsForTask: (taskId: string) => TaskRun[];

  // Dispatch modal state
  showDispatchModal: () => boolean;
  dispatchTaskId: () => string | null;
  openDispatchModal: (taskId: string) => void;
  closeDispatchModal: () => void;
}

const DispatchContext = createContext<DispatchContextValue>();

export const DispatchProvider: ParentComponent = (props) => {
  // Preview state
  const [preview, setPreview] = createSignal<DispatchPreview | null>(null);
  const [isLoadingPreview, setIsLoadingPreview] = createSignal(false);

  // Dispatch state
  const [isDispatching, setIsDispatching] = createSignal(false);

  // Task runs state (map from taskId -> runs)
  const [taskRuns, setTaskRuns] = createSignal<Map<string, TaskRun[]>>(new Map());
  const [isLoadingTaskRuns, setIsLoadingTaskRuns] = createSignal(false);

  // Modal state
  const [showDispatchModal, setShowDispatchModal] = createSignal(false);
  const [dispatchTaskId, setDispatchTaskId] = createSignal<string | null>(null);

  // Load preview for a task
  const loadPreview = async (projectId: number, taskId: string) => {
    setIsLoadingPreview(true);
    try {
      const result = await invoke<DispatchPreview>('preview_dispatch', {
        projectId,
        taskId,
      });
      setPreview(result);
    } catch (e) {
      console.error('Failed to load dispatch preview:', e);
      setPreview(null);
    } finally {
      setIsLoadingPreview(false);
    }
  };

  const clearPreview = () => {
    setPreview(null);
  };

  // Prepare a dispatch (generate spec/eval content)
  const prepareDispatch = async (
    projectId: number,
    taskId: string,
    config?: DispatchConfig
  ): Promise<DispatchInfo> => {
    setIsDispatching(true);
    try {
      const result = await invoke<DispatchInfo>('prepare_dispatch', {
        projectId,
        taskId,
        runName: config?.runName ?? null,
        targetBranch: config?.targetBranch ?? null,
        workerScale: config?.workerScale ?? null,
        timeLimitMinutes: config?.timeLimitMinutes ?? null,
      });
      return result;
    } finally {
      setIsDispatching(false);
    }
  };

  // Record a dispatch in the task_runs table
  const recordDispatch = async (
    projectId: number,
    taskId: string,
    runName: string
  ) => {
    await invoke('record_dispatch', {
      projectId,
      taskId,
      runName,
    });

    // Refresh the task runs for this task
    await loadTaskRuns(projectId, taskId);
  };

  // Load runs for a specific task
  const loadTaskRuns = async (projectId: number, taskId: string): Promise<TaskRun[]> => {
    try {
      const runs = await invoke<TaskRun[]>('get_task_runs', {
        projectId,
        taskId,
      });

      // Update the map
      setTaskRuns((prev) => {
        const newMap = new Map(prev);
        newMap.set(taskId, runs);
        return newMap;
      });

      return runs;
    } catch (e) {
      console.error(`Failed to load task runs for ${taskId}:`, e);
      return [];
    }
  };

  // Load all task runs for a project
  const loadAllTaskRuns = async (projectId: number) => {
    setIsLoadingTaskRuns(true);
    try {
      const allRuns = await invoke<TaskRun[]>('get_all_task_runs', {
        projectId,
      });

      // Group by taskId
      const runsByTask = new Map<string, TaskRun[]>();
      for (const run of allRuns) {
        const existing = runsByTask.get(run.taskId) || [];
        existing.push(run);
        runsByTask.set(run.taskId, existing);
      }

      setTaskRuns(runsByTask);
    } catch (e) {
      console.error('Failed to load all task runs:', e);
    } finally {
      setIsLoadingTaskRuns(false);
    }
  };

  // Refresh task runs for a project
  const refreshTaskRuns = async (projectId: number) => {
    await loadAllTaskRuns(projectId);
  };

  // Get runs for a specific task from cache
  const getRunsForTask = (taskId: string): TaskRun[] => {
    return taskRuns().get(taskId) || [];
  };

  // Modal controls
  const openDispatchModal = (taskId: string) => {
    setDispatchTaskId(taskId);
    setShowDispatchModal(true);
  };

  const closeDispatchModal = () => {
    setShowDispatchModal(false);
    setDispatchTaskId(null);
    clearPreview();
  };

  const value: DispatchContextValue = {
    // Preview
    preview,
    isLoadingPreview,
    loadPreview,
    clearPreview,

    // Dispatch
    isDispatching,
    prepareDispatch,
    recordDispatch,

    // Task runs
    taskRuns,
    isLoadingTaskRuns,
    loadTaskRuns,
    loadAllTaskRuns,
    refreshTaskRuns,
    getRunsForTask,

    // Modal
    showDispatchModal,
    dispatchTaskId,
    openDispatchModal,
    closeDispatchModal,
  };

  return (
    <DispatchContext.Provider value={value}>
      {props.children}
    </DispatchContext.Provider>
  );
};

export function useDispatch() {
  const context = useContext(DispatchContext);
  if (!context) {
    throw new Error('useDispatch must be used within a DispatchProvider');
  }
  return context;
}

export { DispatchContext };
