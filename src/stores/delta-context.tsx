/**
 * Delta dispatch state management
 *
 * Manages the unified board tree state and dispatch operations.
 * Single tree: features (draft) → dispatch → tasks/checks (pending→working→done).
 */

import { invoke } from '../lib/invoke';
import { on } from '../lib/events';
import { createPoll } from '../lib/poll';
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
import { useRoute } from './route-context';
import type {
  BoardNodeTree,
  ProjectRun,
  BoardTreeResponse,
  BoardNode,
  CreateBoardNodeRequest,
  UpdateBoardNodeRequest,
  ShepherdRunResponse,
  BoardVersion,
  BoardDelivery,
  DeliveryAttempt,
} from '../lib/types';

// =============================================================================
// Types
// =============================================================================

interface DeltaState {
  // Tree data
  boardTree: () => BoardNodeTree[];
  projectRun: () => ProjectRun | null;

  // Delivery state
  boardVersions: () => BoardVersion[];
  currentDelivery: () => BoardDelivery | null;
  latestVersion: () => BoardVersion | null;

  // UI state
  loading: () => boolean;
  shepherdStartPending: () => boolean;
  deliveryPending: () => boolean;

  // Computed
  hasDraftNodes: () => boolean;
  hasDispatchedNodes: () => boolean;

  // Actions
  loadTree: (projectId: number, routeId: number) => Promise<void>;
  refreshTree: () => Promise<void>;
  createBoardNode: (request: CreateBoardNodeRequest) => Promise<BoardNode | null>;
  updateBoardNode: (nodeId: string, request: UpdateBoardNodeRequest) => Promise<BoardNode | null>;
  deleteBoardNode: (nodeId: string) => Promise<boolean>;
  moveBoardNode: (nodeId: string, newParentId: string | null, newPosition: number) => Promise<boolean>;
  resetTree: () => Promise<boolean>;
  startShepherdRun: () => Promise<ShepherdRunResponse | null>;

  // Delivery actions
  loadDeliveryState: () => Promise<void>;
  startDelivery: (targetBranch: string, resolveConflicts?: boolean, remoteUrl?: string) => Promise<BoardDelivery | null>;
  completeDelivery: (action: 'push' | 'pr' | 'merge', summary?: string, remoteUrl?: string) => Promise<BoardDelivery | null>;
  retryDelivery: () => Promise<DeliveryAttempt | null>;
  abandonDelivery: () => Promise<boolean>;
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

  // Tree state (single unified tree)
  const [boardTree, setBoardTree] = createSignal<BoardNodeTree[]>([]);
  const [projectRun, setProjectRun] = createSignal<ProjectRun | null>(null);

  // Delivery state
  const [boardVersions, setBoardVersions] = createSignal<BoardVersion[]>([]);
  const [currentDelivery, setCurrentDelivery] = createSignal<BoardDelivery | null>(null);
  const [latestVersion, setLatestVersion] = createSignal<BoardVersion | null>(null);

  // UI state
  const [loading, setLoading] = createSignal(false);
  const [shepherdStartPending, setShepherdStartPending] = createSignal(false);
  const [deliveryPending, setDeliveryPending] = createSignal(false);

  // Generation counter for skipping redundant tree polls
  const [treeGeneration, setTreeGeneration] = createSignal(0);

  // Computed: has any nodes with status=draft
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

  // Computed: has any dispatched (non-draft) nodes
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
    const routeId = route.activeRoute()?.id ?? route.routes()[0]?.id;
    if (projectId && routeId) {
      await loadTree(projectId, routeId);
    }
  };

  const createBoardNode = async (request: CreateBoardNodeRequest): Promise<BoardNode | null> => {
    const projectId = project.selectedProjectId();
    const routeId = route.activeRoute()?.id;
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
    const routeId = route.activeRoute()?.id;
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
    const routeId = route.activeRoute()?.id;
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
    const routeId = route.activeRoute()?.id;
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
    const routeId = route.activeRoute()?.id;
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
    const routeId = route.activeRoute()?.id;
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

  // ==========================================================================
  // Delivery Actions
  // ==========================================================================

  const loadDeliveryState = async () => {
    const projectId = project.selectedProjectId();
    const routeId = route.activeRoute()?.id;
    if (!projectId || !routeId) return;

    try {
      const [versions, latest, delivery] = await Promise.all([
        invoke<BoardVersion[]>('get_board_versions', { projectId, routeId }),
        invoke<BoardVersion | null>('get_latest_board_version', { projectId, routeId }),
        invoke<BoardDelivery | null>('get_current_board_delivery', { projectId, routeId }),
      ]);

      batch(() => {
        setBoardVersions(versions);
        setLatestVersion(latest);
        setCurrentDelivery(delivery);
      });
    } catch (e) {
      console.error('Failed to load delivery state:', e);
    }
  };

  const startDelivery = async (
    targetBranch: string,
    resolveConflicts = false,
    remoteUrl?: string
  ): Promise<BoardDelivery | null> => {
    const projectId = project.selectedProjectId();
    const routeId = route.activeRoute()?.id;
    const version = latestVersion();
    if (!projectId || !routeId || !version) {
      window.toast?.error('No version available for delivery');
      return null;
    }

    try {
      setDeliveryPending(true);
      const delivery = await invoke<BoardDelivery>('start_board_delivery', {
        projectId,
        routeId,
        versionId: version.id,
        targetBranch,
        resolveConflicts,
        remoteUrl: remoteUrl || null,
      });
      setCurrentDelivery(delivery);
      window.toast?.success(`Started delivery for v${version.versionNumber}`);
      return delivery;
    } catch (e) {
      console.error('Failed to start delivery:', e);
      window.toast?.error(`Failed to start delivery: ${e}`);
      return null;
    } finally {
      setDeliveryPending(false);
    }
  };

  const completeDelivery = async (
    action: 'push' | 'pr' | 'merge',
    summary?: string,
    remoteUrl?: string
  ): Promise<BoardDelivery | null> => {
    const projectId = project.selectedProjectId();
    const routeId = route.activeRoute()?.id;
    const delivery = currentDelivery();
    if (!projectId || !routeId || !delivery) {
      window.toast?.error('No active delivery');
      return null;
    }

    try {
      setDeliveryPending(true);
      const updated = await invoke<BoardDelivery>('complete_board_delivery', {
        projectId,
        routeId,
        deliveryId: delivery.id,
        action,
        summary,
        remoteUrl: remoteUrl || null,
      });
      setCurrentDelivery(updated);

      const actionLabels = { push: 'Pushed', pr: 'PR created', merge: 'Merged' };
      window.toast?.success(actionLabels[action]);
      return updated;
    } catch (e) {
      console.error('Failed to complete delivery:', e);
      window.toast?.error(`Failed to ${action}: ${e}`);
      return null;
    } finally {
      setDeliveryPending(false);
    }
  };

  const retryDelivery = async (): Promise<DeliveryAttempt | null> => {
    const projectId = project.selectedProjectId();
    const routeId = route.activeRoute()?.id;
    const delivery = currentDelivery();
    if (!projectId || !routeId || !delivery) {
      window.toast?.error('No delivery to retry');
      return null;
    }

    try {
      setDeliveryPending(true);
      const attempt = await invoke<DeliveryAttempt>('retry_board_delivery', {
        projectId,
        routeId,
        deliveryId: delivery.id,
      });
      await loadDeliveryState();
      window.toast?.success('Retry started');
      return attempt;
    } catch (e) {
      console.error('Failed to retry delivery:', e);
      window.toast?.error(`Failed to retry: ${e}`);
      return null;
    } finally {
      setDeliveryPending(false);
    }
  };

  const abandonDelivery = async (): Promise<boolean> => {
    const projectId = project.selectedProjectId();
    const routeId = route.activeRoute()?.id;
    const delivery = currentDelivery();
    if (!projectId || !routeId || !delivery) {
      window.toast?.error('No delivery to abandon');
      return false;
    }

    try {
      await invoke('abandon_board_delivery', {
        projectId,
        routeId,
        deliveryId: delivery.id,
      });
      setCurrentDelivery(null);
      window.toast?.success('Delivery abandoned');
      return true;
    } catch (e) {
      console.error('Failed to abandon delivery:', e);
      window.toast?.error(`Failed to abandon: ${e}`);
      return false;
    }
  };

  // ==========================================================================
  // Effects
  // ==========================================================================

  // Load tree and delivery state when project or route changes
  createEffect(() => {
    const projectId = project.selectedProjectId();
    // Use active route, or fall back to first route in list
    const routeId = route.activeRoute()?.id ?? route.routes()[0]?.id;
    if (projectId && routeId) {
      loadTree(projectId, routeId);
      loadDeliveryState();
    } else if (!projectId) {
      batch(() => {
        setBoardTree([]);
        setProjectRun(null);
        setBoardVersions([]);
        setLatestVersion(null);
        setCurrentDelivery(null);
      });
    }
  });

  // Listen for route changes and reload tree
  createEffect(() => {
    const projectId = project.selectedProjectId();
    if (!projectId) return;

    const cleanup = on('route-changed', (detail) => {
      if (detail.projectId === projectId && detail.routeId) {
        // Clear current tree to show loading state
        batch(() => {
          setBoardTree([]);
          setProjectRun(null);
        });
        // Reload tree for new route
        loadTree(projectId, detail.routeId);
        loadDeliveryState();
      }
    });

    onCleanup(cleanup);
  });

  // Poll for changes (generation-aware: skips full fetch if nothing changed)
  createEffect(() => {
    const projectId = project.selectedProjectId();
    const routeId = route.activeRoute()?.id ?? route.routes()[0]?.id;
    if (!projectId || !routeId) return;

    // Reset generation when project/route changes so first poll always fetches
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

          if (result === null) return; // No changes — skip store updates

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
    // Tree data
    boardTree,
    projectRun,

    // Delivery state
    boardVersions,
    currentDelivery,
    latestVersion,

    // UI state
    loading,
    shepherdStartPending,
    deliveryPending,

    // Computed
    hasDraftNodes,
    hasDispatchedNodes,

    // Tree actions
    loadTree,
    refreshTree,
    createBoardNode,
    updateBoardNode,
    deleteBoardNode,
    moveBoardNode,
    resetTree,
    startShepherdRun,

    // Delivery actions
    loadDeliveryState,
    startDelivery,
    completeDelivery,
    retryDelivery,
    abandonDelivery,
  };

  return <DeltaContext.Provider value={value}>{props.children}</DeltaContext.Provider>;
};

export default DeltaProvider;
