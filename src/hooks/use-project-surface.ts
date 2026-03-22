import { type UnlistenFn, listen } from '@tauri-apps/api/event';
import { createEffect, createMemo, createResource, on, onCleanup } from 'solid-js';
import { invoke } from '../lib/invoke';
import type { ProjectSurfaceSnapshot } from '../lib/types';
import { useProject, useRoute } from '../stores';

export function useProjectSurface() {
  const project = useProject();
  const route = useRoute();

  const routeSignature = createMemo(() =>
    route
      .routes()
      .map((item) => `${item.id}:${item.updatedAt}`)
      .join('|'),
  );

  const [surface, { refetch }] = createResource(
    () => project.selectedProjectId(),
    async (projectId) => {
      if (!projectId) return null;
      return invoke<ProjectSurfaceSnapshot>('get_project_surface', { projectId });
    },
  );

  createEffect(
    on(
      () => ({
        projectId: project.selectedProjectId(),
        routeId: route.currentRouteId(),
        routeSignature: routeSignature(),
      }),
      ({ projectId }) => {
        if (projectId) {
          void refetch();
        }
      },
      { defer: true },
    ),
  );

  createEffect(() => {
    const projectId = project.selectedProjectId();
    if (!projectId) return;

    let unlistenFocus: UnlistenFn | undefined;

    void listen<{ projectId: number }>('project-focus-view-updated', (event) => {
      if (event.payload.projectId === projectId) {
        void refetch();
      }
    }).then((cleanup) => {
      unlistenFocus = cleanup;
    });

    const interval = setInterval(() => {
      void refetch();
    }, 5000);

    onCleanup(() => {
      clearInterval(interval);
      void unlistenFocus?.();
    });
  });

  return {
    surface,
    refreshSurface: refetch,
  };
}
