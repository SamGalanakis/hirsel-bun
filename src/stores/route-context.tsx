/**
 * Route context for managing parallel exploration branches.
 *
 * Routes allow users to fork a project's line of work and explore
 * different implementation approaches without losing context.
 */
import { invoke } from '../lib/invoke';
import {
  type ParentComponent,
  batch,
  createContext,
  createEffect,
  createSignal,
  useContext,
} from 'solid-js';
import { useProject } from './project-context';
import type { Route } from '../lib/types';

// =============================================================================
// Types
// =============================================================================

interface RouteSettingsUpdate {
  timeLimitMinutes: number | null;
  humanInTheLoop: boolean;
  targetBranch: string | null;
}

interface RouteContextValue {
  routes: () => Route[];
  archivedRoutes: () => Route[];
  activeRoute: () => Route | null;
  currentRoute: () => Route | null;
  currentRouteId: () => number | null;
  loading: () => boolean;
  loadRoutes: () => Promise<void>;
  setActiveRoute: (routeId: number) => Promise<boolean>;
  createRoute: (
    name: string,
    parentRouteId?: number | null,
    parentVersionId?: number | null
  ) => Promise<Route | null>;
  archiveRoute: (routeId: number) => Promise<boolean>;
  updateRouteSettings: (
    routeId: number,
    settings: RouteSettingsUpdate
  ) => Promise<Route | null>;
  setDefaultRouteRepo: (routeId: number, repoId: number) => Promise<Route | null>;
}

// =============================================================================
// Context
// =============================================================================

const RouteContext = createContext<RouteContextValue>();

export const useRoute = () => {
  const ctx = useContext(RouteContext);
  if (!ctx) {
    throw new Error('useRoute must be used within a RouteProvider');
  }
  return ctx;
};

// =============================================================================
// Provider
// =============================================================================

export const RouteProvider: ParentComponent = (props) => {
  const project = useProject();

  const [routes, setRoutes] = createSignal<Route[]>([]);
  const [archivedRoutes, setArchivedRoutes] = createSignal<Route[]>([]);
  const [activeRoute, setActiveRouteState] = createSignal<Route | null>(null);
  const [loading, setLoading] = createSignal(false);
  const currentRoute = () => activeRoute() ?? routes()[0] ?? null;
  const currentRouteId = () => currentRoute()?.id ?? null;

  const loadRoutes = async () => {
    const projectId = project.selectedProjectId();
    if (!projectId) {
      batch(() => {
        setRoutes([]);
        setArchivedRoutes([]);
        setActiveRouteState(null);
      });
      return;
    }

    try {
      setLoading(true);

      const [routesList, archivedList, active] = await Promise.all([
        invoke<Route[]>('list_routes', { projectId }),
        invoke<Route[]>('list_archived_routes', { projectId }).catch(() => []),
        invoke<Route>('get_active_route', { projectId }).catch(() => null),
      ]);

      batch(() => {
        setRoutes(routesList);
        setArchivedRoutes(archivedList);
        setActiveRouteState(active);
      });
    } catch (e) {
      console.error('Failed to load routes:', e);
    } finally {
      setLoading(false);
    }
  };

  const setActiveRoute = async (routeId: number): Promise<boolean> => {
    const projectId = project.selectedProjectId();
    if (!projectId) return false;

    try {
      await invoke('set_active_route', { projectId, routeId });

      const route = await invoke<Route>('get_route', { projectId, routeId });
      setActiveRouteState(route);

      return true;
    } catch (e) {
      console.error('Failed to set active route:', e);
      window.toast?.error(`Failed to switch route: ${e}`);
      return false;
    }
  };

  const createRoute = async (
    name: string,
    parentRouteId?: number | null,
    parentVersionId?: number | null
  ): Promise<Route | null> => {
    const projectId = project.selectedProjectId();
    if (!projectId) return null;

    try {
      const route = await invoke<Route>('create_route', {
        projectId,
        name,
        parentRouteId: parentRouteId ?? currentRouteId(),
        parentVersionId: parentVersionId ?? null,
      });

      await loadRoutes();

      window.toast?.success(`Created route "${name}"`);
      return route;
    } catch (e) {
      console.error('Failed to create route:', e);
      window.toast?.error(`Failed to create route: ${e}`);
      return null;
    }
  };

  const archiveRoute = async (routeId: number): Promise<boolean> => {
    const projectId = project.selectedProjectId();
    if (!projectId) return false;

    try {
      await invoke('archive_route', { projectId, routeId });

      await loadRoutes();

      window.toast?.success('Route archived');
      return true;
    } catch (e) {
      console.error('Failed to archive route:', e);
      window.toast?.error(`Failed to archive route: ${e}`);
      return false;
    }
  };

  const updateRouteSettings = async (
    routeId: number,
    settings: RouteSettingsUpdate
  ): Promise<Route | null> => {
    const projectId = project.selectedProjectId();
    if (!projectId) return null;

    try {
      const updated = await invoke<Route>('update_route_settings', {
        projectId,
        routeId,
        timeLimitMinutes: settings.timeLimitMinutes,
        humanInTheLoop: settings.humanInTheLoop,
        targetBranch: settings.targetBranch,
      });

      await loadRoutes();

      return updated;
    } catch (e) {
      console.error('Failed to update route settings:', e);
      window.toast?.error(`Failed to update route settings: ${e}`);
      return null;
    }
  };

  const setDefaultRouteRepo = async (routeId: number, repoId: number): Promise<Route | null> => {
    const projectId = project.selectedProjectId();
    if (!projectId) return null;

    try {
      const updated = await invoke<Route>('set_default_route_repo', {
        projectId,
        routeId,
        repoId,
      });

      await loadRoutes();

      return updated;
    } catch (e) {
      console.error('Failed to set default route repo:', e);
      window.toast?.error(`Failed to set default repo: ${e}`);
      return null;
    }
  };

  createEffect(() => {
    const projectId = project.selectedProjectId();
    if (projectId) {
      void loadRoutes();
    } else {
      batch(() => {
        setRoutes([]);
        setArchivedRoutes([]);
        setActiveRouteState(null);
      });
    }
  });

  const value: RouteContextValue = {
    routes,
    archivedRoutes,
    activeRoute,
    currentRoute,
    currentRouteId,
    loading,
    loadRoutes,
    setActiveRoute,
    createRoute,
    archiveRoute,
    updateRouteSettings,
    setDefaultRouteRepo,
  };

  return <RouteContext.Provider value={value}>{props.children}</RouteContext.Provider>;
};

export default RouteProvider;
