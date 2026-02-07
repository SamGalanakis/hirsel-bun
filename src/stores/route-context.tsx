/**
 * Route context for managing parallel exploration branches
 *
 * Routes allow users to fork their board at any point and explore
 * different implementation approaches without losing work.
 */
import { invoke } from '../lib/invoke';
import { emit } from '../lib/events';
import {
  type ParentComponent,
  batch,
  createContext,
  createEffect,
  createSignal,
  onCleanup,
  useContext,
} from 'solid-js';
import { useProject } from './project-context';
import type { Route, RouteTree } from '../lib/types';

// =============================================================================
// Types
// =============================================================================

interface RouteContextValue {
  // State
  routes: () => Route[];
  routeTree: () => RouteTree[];
  activeRoute: () => Route | null;
  loading: () => boolean;

  // Actions
  loadRoutes: () => Promise<void>;
  setActiveRoute: (routeId: number) => Promise<boolean>;
  createRoute: (
    name: string,
    parentRouteId?: number | null,
    parentVersionId?: number | null
  ) => Promise<Route | null>;
  deleteRoute: (routeId: number) => Promise<boolean>;
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

  // State
  const [routes, setRoutes] = createSignal<Route[]>([]);
  const [routeTree, setRouteTree] = createSignal<RouteTree[]>([]);
  const [activeRoute, setActiveRouteState] = createSignal<Route | null>(null);
  const [loading, setLoading] = createSignal(false);

  // ==========================================================================
  // Actions
  // ==========================================================================

  const loadRoutes = async () => {
    const projectId = project.selectedProjectId();
    if (!projectId) {
      batch(() => {
        setRoutes([]);
        setRouteTree([]);
        setActiveRouteState(null);
      });
      return;
    }

    try {
      setLoading(true);

      const [routesList, tree, active] = await Promise.all([
        invoke<Route[]>('list_routes', { projectId }),
        invoke<RouteTree[]>('get_route_tree', { projectId }),
        invoke<Route>('get_active_route', { projectId }).catch(() => null),
      ]);

      batch(() => {
        setRoutes(routesList);
        setRouteTree(tree);
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

      // Get the route details
      const route = await invoke<Route>('get_route', { projectId, routeId });
      setActiveRouteState(route);

      // Emit event for delta context to reload
      emit('route-changed', { projectId, routeId });

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
        parentRouteId: parentRouteId ?? activeRoute()?.id ?? null,
        parentVersionId: parentVersionId ?? null,
      });

      // Reload routes list
      await loadRoutes();

      window.toast?.success(`Created route "${name}"`);
      return route;
    } catch (e) {
      console.error('Failed to create route:', e);
      window.toast?.error(`Failed to create route: ${e}`);
      return null;
    }
  };

  const deleteRoute = async (routeId: number): Promise<boolean> => {
    const projectId = project.selectedProjectId();
    if (!projectId) return false;

    try {
      await invoke('delete_route', { projectId, routeId });

      // Reload routes list
      await loadRoutes();

      window.toast?.success('Route deleted');
      return true;
    } catch (e) {
      console.error('Failed to delete route:', e);
      window.toast?.error(`Failed to delete route: ${e}`);
      return false;
    }
  };

  // ==========================================================================
  // Effects
  // ==========================================================================

  // Load routes when project changes
  createEffect(() => {
    const projectId = project.selectedProjectId();
    if (projectId) {
      loadRoutes();
    } else {
      batch(() => {
        setRoutes([]);
        setRouteTree([]);
        setActiveRouteState(null);
      });
    }
  });

  // ==========================================================================
  // Context Value
  // ==========================================================================

  const value: RouteContextValue = {
    routes,
    routeTree,
    activeRoute,
    loading,
    loadRoutes,
    setActiveRoute,
    createRoute,
    deleteRoute,
  };

  return <RouteContext.Provider value={value}>{props.children}</RouteContext.Provider>;
};

export default RouteProvider;
