import { createEffect, createResource, on, onCleanup } from 'solid-js';
import { invoke } from '../lib/invoke';
import type { WorkTreeSnapshot } from '../lib/types';
import { useProject, useRoute } from '../stores';

export function useRouteWorkTree() {
  const project = useProject();
  const route = useRoute();

  const [snapshot, { refetch }] = createResource(
    () => {
      const projectId = project.selectedProjectId();
      const routeId = route.currentRouteId();
      return projectId && routeId ? { projectId, routeId } : null;
    },
    async (ctx) => {
      if (!ctx) return null;
      return invoke<WorkTreeSnapshot>('get_route_work_tree', ctx);
    },
  );

  createEffect(
    on(
      () => ({
        projectId: project.selectedProjectId(),
        routeId: route.currentRouteId(),
      }),
      (ctx) => {
        if (ctx.projectId && ctx.routeId) {
          void refetch();
        }
      },
      { defer: true },
    ),
  );

  createEffect(() => {
    const projectId = project.selectedProjectId();
    const routeId = route.currentRouteId();
    if (!projectId || !routeId) return;

    const interval = setInterval(() => {
      void refetch();
    }, 4000);

    onCleanup(() => clearInterval(interval));
  });

  return {
    snapshot,
    refreshWorkTree: refetch,
  };
}
