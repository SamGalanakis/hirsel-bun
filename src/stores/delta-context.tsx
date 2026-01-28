/**
 * Delta dispatch state management
 *
 * Manages the draft/live tree state and delta dispatch operations.
 */

import { invoke } from '@tauri-apps/api/core';
import {
  createContext,
  useContext,
  type ParentComponent,
  createSignal,
  createEffect,
  onCleanup,
  batch,
} from 'solid-js';
import { useProject } from './project-context';
import type {
  DraftNodeTree,
  LiveNodeTree,
  TreeDiff,
  ProjectRun,
  DualTreeResponse,
  CreateDraftNodeRequest,
  UpdateDraftNodeRequest,
  DeltaDispatchResponse,
  DraftNode,
} from '../lib/types';

// =============================================================================
// Types
// =============================================================================

interface DeltaState {
  // Tree data
  draftTree: () => DraftNodeTree[];
  liveTree: () => LiveNodeTree[];
  diff: () => TreeDiff | null;
  projectRun: () => ProjectRun | null;

  // UI state
  loading: () => boolean;
  dispatchPending: () => boolean;
  showDeltaIndicators: () => boolean;

  // Computed
  hasDiff: () => boolean;

  // Actions
  loadTrees: (projectId: number) => Promise<void>;
  refreshTrees: () => Promise<void>;
  createDraftNode: (request: CreateDraftNodeRequest) => Promise<DraftNode | null>;
  updateDraftNode: (nodeId: string, request: UpdateDraftNodeRequest) => Promise<DraftNode | null>;
  deleteDraftNode: (nodeId: string) => Promise<boolean>;
  moveDraftNode: (nodeId: string, newParentId: string | null, newPosition: number) => Promise<boolean>;
  dispatch: () => Promise<DeltaDispatchResponse | null>;
  toggleDeltaIndicators: () => void;
}

// =============================================================================
// Context
// =============================================================================

const DeltaContext = createContext<DeltaState>();

export const useDelta = () => {
  const ctx = useContext(DeltaContext);
  if (!ctx) {
    throw new Error('useDelta must be used within a DeltaProvider');
  }
  return ctx;
};

// =============================================================================
// Provider
// =============================================================================

export const DeltaProvider: ParentComponent = (props) => {
  const project = useProject();

  // Tree state
  const [draftTree, setDraftTree] = createSignal<DraftNodeTree[]>([]);
  const [liveTree, setLiveTree] = createSignal<LiveNodeTree[]>([]);
  const [diff, setDiff] = createSignal<TreeDiff | null>(null);
  const [projectRun, setProjectRun] = createSignal<ProjectRun | null>(null);

  // UI state
  const [loading, setLoading] = createSignal(false);
  const [dispatchPending, setDispatchPending] = createSignal(false);
  const [showDeltaIndicators, setShowDeltaIndicators] = createSignal(true);

  // Computed
  const hasDiff = () => {
    const d = diff();
    if (!d) return false;
    return d.newNodes.length > 0 || d.modifiedNodes.length > 0 || d.deletedNodes.length > 0;
  };

  // ==========================================================================
  // Actions
  // ==========================================================================

  const loadTrees = async (projectId: number) => {
    try {
      setLoading(true);
      const response = await invoke<DualTreeResponse>('get_dual_trees', { projectId });
      batch(() => {
        setDraftTree(response.draft);
        setLiveTree(response.live);
        setDiff(response.diff);
        setProjectRun(response.projectRun);
        setLoading(false);
      });
    } catch (e) {
      console.error('Failed to load trees:', e);
      setLoading(false);
    }
  };

  const refreshTrees = async () => {
    const projectId = project.selectedProjectId();
    if (projectId) {
      await loadTrees(projectId);
    }
  };

  const createDraftNode = async (request: CreateDraftNodeRequest): Promise<DraftNode | null> => {
    const projectId = project.selectedProjectId();
    if (!projectId) return null;

    try {
      const node = await invoke<DraftNode>('create_draft_node', { projectId, request });
      await refreshTrees();
      return node;
    } catch (e) {
      console.error('Failed to create draft node:', e);
      window.toast?.error(`Failed to create node: ${e}`);
      return null;
    }
  };

  const updateDraftNode = async (
    nodeId: string,
    request: UpdateDraftNodeRequest
  ): Promise<DraftNode | null> => {
    const projectId = project.selectedProjectId();
    if (!projectId) return null;

    try {
      const node = await invoke<DraftNode>('update_draft_node', { projectId, nodeId, request });
      await refreshTrees();
      return node;
    } catch (e) {
      console.error('Failed to update draft node:', e);
      window.toast?.error(`Failed to update node: ${e}`);
      return null;
    }
  };

  const deleteDraftNode = async (nodeId: string): Promise<boolean> => {
    const projectId = project.selectedProjectId();
    if (!projectId) return false;

    try {
      await invoke('delete_draft_node', { projectId, nodeId });
      await refreshTrees();
      return true;
    } catch (e) {
      console.error('Failed to delete draft node:', e);
      window.toast?.error(`Failed to delete node: ${e}`);
      return false;
    }
  };

  const moveDraftNode = async (
    nodeId: string,
    newParentId: string | null,
    newPosition: number
  ): Promise<boolean> => {
    const projectId = project.selectedProjectId();
    if (!projectId) return false;

    try {
      await invoke('move_draft_node', { projectId, nodeId, newParentId, newPosition });
      await refreshTrees();
      return true;
    } catch (e) {
      console.error('Failed to move draft node:', e);
      window.toast?.error(`Failed to move node: ${e}`);
      return false;
    }
  };

  const dispatch = async (): Promise<DeltaDispatchResponse | null> => {
    const projectId = project.selectedProjectId();
    if (!projectId) return null;

    try {
      setDispatchPending(true);
      const response = await invoke<DeltaDispatchResponse>('dispatch_deltas', { projectId });
      await refreshTrees();
      window.toast?.success(`Dispatched ${response.deltaCount} delta tasks`);
      return response;
    } catch (e) {
      console.error('Failed to dispatch:', e);
      window.toast?.error(`Failed to dispatch: ${e}`);
      return null;
    } finally {
      setDispatchPending(false);
    }
  };

  const toggleDeltaIndicators = () => {
    setShowDeltaIndicators((prev) => !prev);
  };

  // ==========================================================================
  // Effects
  // ==========================================================================

  // Load trees when project changes
  createEffect(() => {
    const projectId = project.selectedProjectId();
    if (projectId) {
      loadTrees(projectId);
    } else {
      batch(() => {
        setDraftTree([]);
        setLiveTree([]);
        setDiff(null);
        setProjectRun(null);
      });
    }
  });

  // Poll for changes
  createEffect(() => {
    const projectId = project.selectedProjectId();
    if (!projectId) return;

    const interval = setInterval(async () => {
      // Only refresh if not currently dispatching
      if (!dispatchPending()) {
        await refreshTrees();
      }
    }, 3000);

    onCleanup(() => clearInterval(interval));
  });

  // ==========================================================================
  // Context Value
  // ==========================================================================

  const value: DeltaState = {
    draftTree,
    liveTree,
    diff,
    projectRun,
    loading,
    dispatchPending,
    showDeltaIndicators,
    hasDiff,
    loadTrees,
    refreshTrees,
    createDraftNode,
    updateDraftNode,
    deleteDraftNode,
    moveDraftNode,
    dispatch,
    toggleDeltaIndicators,
  };

  return <DeltaContext.Provider value={value}>{props.children}</DeltaContext.Provider>;
};

export default DeltaProvider;
