/**
 * Route context for managing parallel exploration branches
 *
 * Routes allow users to fork their board at any point and explore
 * different implementation approaches without losing work.
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

interface RouteContextValue {
  routes: () => Route[];
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

  const [routes, setRoutes] = createSignal<Route[]>([]);
  const [activeRoute, setActiveRouteState] = createSignal<Route | null>(null);
  const [loading, setLoading] = createSignal(false);
  const currentRoute = () => activeRoute() ?? routes()[0] ?? null;
  const currentRouteId = () => currentRoute()?.id ?? null;

  const loadRoutes = async () => {
    const projectId = project.selectedProjectId();
    if (!projectId) {
      batch(() => {
        setRoutes([]);
        setActiveRouteState(null);
      });
      return;
    }

    try {
      setLoading(true);

      const [routesList, active] = await Promise.all([
        invoke<Route[]>('list_routes', { projectId }),
        invoke<Route>('get_active_route', { projectId }).catch(() => null),
      ]);

      batch(() => {
        setRoutes(routesList);
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

  const deleteRoute = async (routeId: number): Promise<boolean> => {
    const projectId = project.selectedProjectId();
    if (!projectId) return false;

    try {
      await invoke('delete_route', { projectId, routeId });

      await loadRoutes();

      window.toast?.success('Route deleted');
      return true;
    } catch (e) {
      console.error('Failed to delete route:', e);
      window.toast?.error(`Failed to delete route: ${e}`);
      return false;
    }
  };

  createEffect(() => {
    const projectId = project.selectedProjectId();
    if (projectId) {
      void loadRoutes();
    } else {
      batch(() => {
        setRoutes([]);
        setActiveRouteState(null);
      });
    }
  });

  const value: RouteContextValue = {
    routes,
    activeRoute,
    currentRoute,
    currentRouteId,
    loading,
    loadRoutes,
    setActiveRoute,
    createRoute,
    deleteRoute,
  };

  return <RouteContext.Provider value={value}>{props.children}</RouteContext.Provider>;
};

export default RouteProvider;
