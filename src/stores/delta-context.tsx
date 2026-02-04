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
import { useRoute } from './route-context';
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
  BoardVersion,
  BoardDelivery,
  DeliveryAttempt,
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

  // Delivery state
  boardVersions: () => BoardVersion[];
  currentDelivery: () => BoardDelivery | null;
  latestVersion: () => BoardVersion | null;

  // UI state
  loading: () => boolean;
  dispatchPending: () => boolean;
  showDeltaIndicators: () => boolean;
  deliveryPending: () => boolean;

  // Computed
  hasDiff: () => boolean;

  // Actions
  loadTrees: (projectId: number, routeId: number) => Promise<void>;
  refreshTrees: () => Promise<void>;
  createDraftNode: (request: CreateDraftNodeRequest) => Promise<DraftNode | null>;
  updateDraftNode: (nodeId: string, request: UpdateDraftNodeRequest) => Promise<DraftNode | null>;
  deleteDraftNode: (nodeId: string) => Promise<boolean>;
  moveDraftNode: (nodeId: string, newParentId: string | null, newPosition: number) => Promise<boolean>;
  resetTree: () => Promise<boolean>;
  dispatch: () => Promise<DeltaDispatchResponse | null>;
  toggleDeltaIndicators: () => void;

  // Delivery actions
  loadDeliveryState: () => Promise<void>;
  startDelivery: (targetBranch: string, resolveConflicts?: boolean) => Promise<BoardDelivery | null>;
  completeDelivery: (action: 'push' | 'pr' | 'merge', summary?: string) => Promise<BoardDelivery | null>;
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

  // Tree state
  const [draftTree, setDraftTree] = createSignal<DraftNodeTree[]>([]);
  const [liveTree, setLiveTree] = createSignal<LiveNodeTree[]>([]);
  const [diff, setDiff] = createSignal<TreeDiff | null>(null);
  const [projectRun, setProjectRun] = createSignal<ProjectRun | null>(null);

  // Delivery state
  const [boardVersions, setBoardVersions] = createSignal<BoardVersion[]>([]);
  const [currentDelivery, setCurrentDelivery] = createSignal<BoardDelivery | null>(null);
  const [latestVersion, setLatestVersion] = createSignal<BoardVersion | null>(null);

  // UI state
  const [loading, setLoading] = createSignal(false);
  const [dispatchPending, setDispatchPending] = createSignal(false);
  const [showDeltaIndicators, setShowDeltaIndicators] = createSignal(true);
  const [deliveryPending, setDeliveryPending] = createSignal(false);

  // Computed
  const hasDiff = () => {
    const d = diff();
    if (!d) return false;
    return d.newNodes.length > 0 || d.modifiedNodes.length > 0 || d.deletedNodes.length > 0;
  };

  // ==========================================================================
  // Actions
  // ==========================================================================

  const loadTrees = async (projectId: number, routeId: number) => {
    try {
      setLoading(true);
      const response = await invoke<DualTreeResponse>('get_dual_trees', { projectId, routeId });
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
    const routeId = route.activeRoute()?.id ?? route.routes()[0]?.id;
    if (projectId && routeId) {
      await loadTrees(projectId, routeId);
    }
  };

  const createDraftNode = async (request: CreateDraftNodeRequest): Promise<DraftNode | null> => {
    const projectId = project.selectedProjectId();
    const routeId = route.activeRoute()?.id;
    if (!projectId || !routeId) return null;

    try {
      const node = await invoke<DraftNode>('create_draft_node', { projectId, routeId, request });
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
    const routeId = route.activeRoute()?.id;
    if (!projectId || !routeId) return null;

    try {
      const node = await invoke<DraftNode>('update_draft_node', { projectId, routeId, nodeId, request });
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
    const routeId = route.activeRoute()?.id;
    if (!projectId || !routeId) return false;

    try {
      await invoke('delete_draft_node', { projectId, routeId, nodeId });
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
    const routeId = route.activeRoute()?.id;
    if (!projectId || !routeId) return false;

    try {
      await invoke('move_draft_node', { projectId, routeId, nodeId, newParentId, newPosition });
      await refreshTrees();
      return true;
    } catch (e) {
      console.error('Failed to move draft node:', e);
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
      await refreshTrees();
      window.toast?.success('Tree reset successfully');
      return true;
    } catch (e) {
      console.error('Failed to reset tree:', e);
      window.toast?.error(`Failed to reset tree: ${e}`);
      return false;
    }
  };

  const dispatch = async (): Promise<DeltaDispatchResponse | null> => {
    const projectId = project.selectedProjectId();
    const routeId = route.activeRoute()?.id;
    if (!projectId || !routeId) return null;

    try {
      setDispatchPending(true);
      const response = await invoke<DeltaDispatchResponse>('dispatch_deltas', { projectId, routeId });
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
    resolveConflicts = false
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
    summary?: string
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

  // Load trees and delivery state when project or route changes
  createEffect(() => {
    const projectId = project.selectedProjectId();
    // Use active route, or fall back to first route in list
    const routeId = route.activeRoute()?.id ?? route.routes()[0]?.id;
    if (projectId && routeId) {
      loadTrees(projectId, routeId);
      loadDeliveryState();
    } else if (!projectId) {
      batch(() => {
        setDraftTree([]);
        setLiveTree([]);
        setDiff(null);
        setProjectRun(null);
        setBoardVersions([]);
        setLatestVersion(null);
        setCurrentDelivery(null);
      });
    }
  });

  // Listen for route changes and reload trees
  createEffect(() => {
    const projectId = project.selectedProjectId();
    if (!projectId) return;

    const handleRouteChange = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.projectId === projectId && detail?.routeId) {
        // Clear current trees to show loading state
        batch(() => {
          setDraftTree([]);
          setLiveTree([]);
          setDiff(null);
          setProjectRun(null);
        });
        // Reload trees for new route
        loadTrees(projectId, detail.routeId);
        loadDeliveryState();
      }
    };

    window.addEventListener('route-changed', handleRouteChange);
    onCleanup(() => window.removeEventListener('route-changed', handleRouteChange));
  });

  // Poll for changes (including Gyp sync)
  createEffect(() => {
    const projectId = project.selectedProjectId();
    const routeId = route.activeRoute()?.id ?? route.routes()[0]?.id;
    if (!projectId || !routeId) return;

    const interval = setInterval(async () => {
      // Only refresh if not currently dispatching
      if (dispatchPending()) return;

      try {
        // Sync Gyp file changes first
        await invoke('sync_gyp_changes', { projectId, routeId });

        // Load trees and compare before updating to avoid flicker
        const response = await invoke<DualTreeResponse>('get_dual_trees', { projectId, routeId });

        // Only update if data actually changed (simple JSON comparison)
        const newDraftJson = JSON.stringify(response.draft);
        const newLiveJson = JSON.stringify(response.live);
        const currentDraftJson = JSON.stringify(draftTree());
        const currentLiveJson = JSON.stringify(liveTree());

        if (newDraftJson !== currentDraftJson || newLiveJson !== currentLiveJson) {
          batch(() => {
            setDraftTree(response.draft);
            setLiveTree(response.live);
            setDiff(response.diff);
            setProjectRun(response.projectRun);
          });
        }
      } catch (e) {
        console.warn('Delta tree poll failed:', e);
      }
    }, 2000);

    onCleanup(() => clearInterval(interval));
  });

  // ==========================================================================
  // Context Value
  // ==========================================================================

  const value: DeltaState = {
    // Tree data
    draftTree,
    liveTree,
    diff,
    projectRun,

    // Delivery state
    boardVersions,
    currentDelivery,
    latestVersion,

    // UI state
    loading,
    dispatchPending,
    showDeltaIndicators,
    deliveryPending,

    // Computed
    hasDiff,

    // Tree actions
    loadTrees,
    refreshTrees,
    createDraftNode,
    updateDraftNode,
    deleteDraftNode,
    moveDraftNode,
    resetTree,
    dispatch,
    toggleDeltaIndicators,

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
