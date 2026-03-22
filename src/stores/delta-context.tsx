/**
 * Delta dispatch state management
 *
 * Manages the unified board tree state and dispatch operations.
 * Single tree: features (draft) → dispatch → tasks/checks (pending→working→done).
 */

import { invoke } from '../lib/invoke';
import { createPoll } from '../lib/poll';
import {
  createContext,
  useContext,
  type ParentComponent,
  createSignal,
  createEffect,
  batch,
  on,
} from 'solid-js';
import { useProject } from './project-context';
import { useRoute } from './route-context';
import type {
  BoardNodeTree,
  ProjectRun,
  BoardTreeResponse,
  BoardNode,
  CreateBoardNodeRequest,
  UpdateBoardNodeRequest,
  ShepherdRunResponse,
} from '../lib/types';

// =============================================================================
// Types
// =============================================================================

interface DeltaState {
  boardTree: () => BoardNodeTree[];
  projectRun: () => ProjectRun | null;
  loading: () => boolean;
  shepherdStartPending: () => boolean;
  hasDraftNodes: () => boolean;
  hasDispatchedNodes: () => boolean;
  loadTree: (projectId: number, routeId: number) => Promise<void>;
  refreshTree: () => Promise<void>;
  createBoardNode: (request: CreateBoardNodeRequest) => Promise<BoardNode | null>;
  updateBoardNode: (nodeId: string, request: UpdateBoardNodeRequest) => Promise<BoardNode | null>;
  deleteBoardNode: (nodeId: string) => Promise<boolean>;
  moveBoardNode: (nodeId: string, newParentId: string | null, newPosition: number) => Promise<boolean>;
  resetTree: () => Promise<boolean>;
  startShepherdRun: () => Promise<ShepherdRunResponse | null>;
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
  const route = useRoute();

  const [boardTree, setBoardTree] = createSignal<BoardNodeTree[]>([]);
  const [projectRun, setProjectRun] = createSignal<ProjectRun | null>(null);
  const [loading, setLoading] = createSignal(false);
  const [shepherdStartPending, setShepherdStartPending] = createSignal(false);
  const [treeGeneration, setTreeGeneration] = createSignal(0);
  const hasDraftNodes = () => {
    const trees = boardTree();
    const check = (nodes: BoardNodeTree[]): boolean => {
      for (const node of nodes) {
        if (node.status === 'draft') return true;
        if (check(node.children)) return true;
      }
      return false;
    };
    return check(trees);
  };

  const hasDispatchedNodes = () => {
    const trees = boardTree();
    const check = (nodes: BoardNodeTree[]): boolean => {
      for (const node of nodes) {
        if (node.status !== 'draft') return true;
        if (check(node.children)) return true;
      }
      return false;
    };
    return check(trees);
  };

  // ==========================================================================
  // Actions
  // ==========================================================================

  const loadTree = async (projectId: number, routeId: number) => {
    try {
      setLoading(true);
      const response = await invoke<BoardTreeResponse>('get_board_tree', { projectId, routeId });
      batch(() => {
        setBoardTree(response.tree);
        setProjectRun(response.projectRun);
        setTreeGeneration(response.generation);
        setLoading(false);
      });
    } catch (e) {
      console.error('Failed to load board tree:', e);
      setLoading(false);
    }
  };

  const refreshTree = async () => {
    const projectId = project.selectedProjectId();
    const routeId = route.currentRouteId();
    if (projectId && routeId) {
      await loadTree(projectId, routeId);
    }
  };

  const createBoardNode = async (request: CreateBoardNodeRequest): Promise<BoardNode | null> => {
    const projectId = project.selectedProjectId();
    const routeId = route.currentRouteId();
    if (!projectId || !routeId) return null;

    try {
      const node = await invoke<BoardNode>('create_board_node', { projectId, routeId, request });
      await refreshTree();
      return node;
    } catch (e) {
      console.error('Failed to create board node:', e);
      window.toast?.error(`Failed to create node: ${e}`);
      return null;
    }
  };

  const updateBoardNode = async (
    nodeId: string,
    request: UpdateBoardNodeRequest
  ): Promise<BoardNode | null> => {
    const projectId = project.selectedProjectId();
    const routeId = route.currentRouteId();
    if (!projectId || !routeId) return null;

    try {
      const node = await invoke<BoardNode>('update_board_node', { projectId, routeId, nodeId, request });
      await refreshTree();
      return node;
    } catch (e) {
      console.error('Failed to update board node:', e);
      window.toast?.error(`Failed to update node: ${e}`);
      return null;
    }
  };

  const deleteBoardNode = async (nodeId: string): Promise<boolean> => {
    const projectId = project.selectedProjectId();
    const routeId = route.currentRouteId();
    if (!projectId || !routeId) return false;

    try {
      await invoke('delete_board_node', { projectId, routeId, nodeId });
      await refreshTree();
      return true;
    } catch (e) {
      console.error('Failed to delete board node:', e);
      window.toast?.error(`Failed to delete node: ${e}`);
      return false;
    }
  };

  const moveBoardNode = async (
    nodeId: string,
    newParentId: string | null,
    newPosition: number
  ): Promise<boolean> => {
    const projectId = project.selectedProjectId();
    const routeId = route.currentRouteId();
    if (!projectId || !routeId) return false;

    try {
      await invoke('move_board_node', { projectId, routeId, nodeId, newParentId, newPosition });
      await refreshTree();
      return true;
    } catch (e) {
      console.error('Failed to move board node:', e);
      window.toast?.error(`Failed to move node: ${e}`);
      return false;
    }
  };

  const resetTree = async (): Promise<boolean> => {
    const projectId = project.selectedProjectId();
    const routeId = route.currentRouteId();
    if (!projectId || !routeId) return false;

    try {
      await invoke('reset_project_tree', { projectId, routeId });
      await refreshTree();
      window.toast?.success('Tree reset successfully');
      return true;
    } catch (e) {
      console.error('Failed to reset tree:', e);
      window.toast?.error(`Failed to reset tree: ${e}`);
      return false;
    }
  };

  const startShepherdRun = async (): Promise<ShepherdRunResponse | null> => {
    const projectId = project.selectedProjectId();
    const routeId = route.currentRouteId();
    if (!projectId || !routeId) return null;

    try {
      setShepherdStartPending(true);
      const response = await invoke<ShepherdRunResponse>('start_shepherd_run', { projectId, routeId });
      await refreshTree();
      window.toast?.success(`Shepherd started ${response.nodeCount} nodes`);
      return response;
    } catch (e) {
      console.error('Failed to start Shepherd run:', e);
      window.toast?.error(`Failed to start Shepherd run: ${e}`);
      return null;
    } finally {
      setShepherdStartPending(false);
    }
  };

  createEffect(
    on(
      () => ({
        projectId: project.selectedProjectId(),
        routeId: route.currentRouteId(),
      }),
      ({ projectId, routeId }) => {
        if (projectId && routeId) {
          batch(() => {
            setBoardTree([]);
            setProjectRun(null);
            setTreeGeneration(0);
          });
          void loadTree(projectId, routeId);
          return;
        }

        batch(() => {
          setBoardTree([]);
          setProjectRun(null);
          setTreeGeneration(0);
        });
      },
    ),
  );

  createEffect(() => {
    const projectId = project.selectedProjectId();
    const routeId = route.currentRouteId();
    if (!projectId || !routeId) return;

    setTreeGeneration(0);

    createPoll(
      async () => {
        if (shepherdStartPending()) return;

        try {
          const result = await invoke<BoardTreeResponse | null>(
            'sync_and_get_shepherd_view_if_changed',
            {
              projectId,
              routeId,
              lastGeneration: treeGeneration(),
            },
          );

          if (result === null) return;

          batch(() => {
            setBoardTree(result.tree);
            setProjectRun(result.projectRun);
            setTreeGeneration(result.generation);
          });
        } catch (e) {
          console.warn('Board tree poll failed:', e);
        }
      },
      { interval: 5000, immediate: false },
    );
  });

  // ==========================================================================
  // Context Value
  // ==========================================================================

  const value: DeltaState = {
    boardTree,
    projectRun,
    loading,
    shepherdStartPending,
    hasDraftNodes,
    hasDispatchedNodes,
    loadTree,
    refreshTree,
    createBoardNode,
    updateBoardNode,
    deleteBoardNode,
    moveBoardNode,
    resetTree,
    startShepherdRun,
  };

  return <DeltaContext.Provider value={value}>{props.children}</DeltaContext.Provider>;
};

export default DeltaProvider;
