import { invoke } from './invoke';
import type { WorkerConcern, WorkerDisplay } from './types';

export async function getRouteWorkers(
  projectId: number,
  routeId: number,
): Promise<WorkerDisplay[]> {
  return invoke<WorkerDisplay[]>('get_route_workers', { projectId, routeId });
}

export async function getWorkerConcerns(
  projectId: number,
  routeId: number,
  options?: {
    includeResolved?: boolean;
    limit?: number;
  },
): Promise<WorkerConcern[]> {
  return invoke<WorkerConcern[]>('get_worker_concerns', {
    projectId,
    routeId,
    includeResolved: options?.includeResolved,
    limit: options?.limit,
  });
}

export async function markWorkerConcernRead(concernId: number): Promise<void> {
  return invoke('mark_worker_concern_read', { concernId });
}

export async function markRouteConcernsRead(projectId: number, routeId: number): Promise<void> {
  return invoke('mark_route_concerns_read', { projectId, routeId });
}

export async function resolveWorkerConcern(
  concernId: number,
  resolution?: string,
): Promise<WorkerConcern> {
  return invoke<WorkerConcern>('resolve_worker_concern', { concernId, resolution });
}

export function createPoller<T>(
  fetcher: () => Promise<T>,
  onUpdate: (data: T) => void,
  intervalMs = 2000,
): { start: () => void; stop: () => void } {
  let intervalId: ReturnType<typeof setInterval> | null = null;

  return {
    start() {
      if (intervalId) return;
      fetcher().then(onUpdate).catch(console.error);
      intervalId = setInterval(() => {
        fetcher().then(onUpdate).catch(console.error);
      }, intervalMs);
    },
    stop() {
      if (intervalId) {
        clearInterval(intervalId);
        intervalId = null;
      }
    },
  };
}
