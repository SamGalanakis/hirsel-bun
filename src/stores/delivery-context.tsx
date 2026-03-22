import {
  batch,
  createContext,
  createEffect,
  on,
  type ParentComponent,
  useContext,
} from 'solid-js';
import { createSignal } from 'solid-js';
import { invoke } from '../lib/invoke';
import type { BoardDelivery, BoardVersion, DeliveryAttempt } from '../lib/types';
import { useProject } from './project-context';
import { useRoute } from './route-context';

interface DeliveryState {
  boardVersions: () => BoardVersion[];
  currentDelivery: () => BoardDelivery | null;
  latestVersion: () => BoardVersion | null;
  deliveryPending: () => boolean;
  loadDeliveryState: () => Promise<void>;
  startDelivery: (
    targetBranch: string,
    resolveConflicts?: boolean,
    remoteUrl?: string,
  ) => Promise<BoardDelivery | null>;
  completeDelivery: (
    action: 'push' | 'pr' | 'merge',
    summary?: string,
    remoteUrl?: string,
  ) => Promise<BoardDelivery | null>;
  retryDelivery: () => Promise<DeliveryAttempt | null>;
  abandonDelivery: () => Promise<boolean>;
}

const DeliveryContext = createContext<DeliveryState>();

export const DeliveryProvider: ParentComponent = (props) => {
  const project = useProject();
  const route = useRoute();

  const [boardVersions, setBoardVersions] = createSignal<BoardVersion[]>([]);
  const [currentDelivery, setCurrentDelivery] = createSignal<BoardDelivery | null>(null);
  const [latestVersion, setLatestVersion] = createSignal<BoardVersion | null>(null);
  const [deliveryPending, setDeliveryPending] = createSignal(false);

  const loadDeliveryState = async () => {
    const projectId = project.selectedProjectId();
    const routeId = route.currentRouteId();
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
    remoteUrl?: string,
  ): Promise<BoardDelivery | null> => {
    const projectId = project.selectedProjectId();
    const routeId = route.currentRouteId();
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
    remoteUrl?: string,
  ): Promise<BoardDelivery | null> => {
    const projectId = project.selectedProjectId();
    const routeId = route.currentRouteId();
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
      console.error(`Failed to ${action}:`, e);
      window.toast?.error(`Failed to ${action}: ${e}`);
      return null;
    } finally {
      setDeliveryPending(false);
    }
  };

  const retryDelivery = async (): Promise<DeliveryAttempt | null> => {
    const projectId = project.selectedProjectId();
    const routeId = route.currentRouteId();
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
    const routeId = route.currentRouteId();
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

  createEffect(
    on(
      () => ({
        projectId: project.selectedProjectId(),
        routeId: route.currentRouteId(),
      }),
      ({ projectId, routeId }) => {
        if (projectId && routeId) {
          void loadDeliveryState();
          return;
        }

        batch(() => {
          setBoardVersions([]);
          setLatestVersion(null);
          setCurrentDelivery(null);
        });
      },
    ),
  );

  const value: DeliveryState = {
    boardVersions,
    currentDelivery,
    latestVersion,
    deliveryPending,
    loadDeliveryState,
    startDelivery,
    completeDelivery,
    retryDelivery,
    abandonDelivery,
  };

  return <DeliveryContext.Provider value={value}>{props.children}</DeliveryContext.Provider>;
};

export const useDelivery = () => {
  const ctx = useContext(DeliveryContext);
  if (!ctx) {
    throw new Error('useDelivery must be used within a DeliveryProvider');
  }
  return ctx;
};
