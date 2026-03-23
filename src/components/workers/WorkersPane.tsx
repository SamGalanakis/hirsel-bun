import { emit } from '../../lib/events';
import {
  getRouteWorkers,
  getWorkerConcerns,
  markRouteConcernsRead,
  markWorkerConcernRead,
  resolveWorkerConcern,
} from '../../lib/api';
import { createPoll } from '../../lib/poll';
import type { WorkerConcern, WorkerDisplay } from '../../lib/types';
import { useProject, useRoute } from '../../stores';
import { WorkerCard } from '../runs/WorkerCard';
import { WorkerDetailModal } from '../runs/WorkerDetailModal';
import { Icon } from '../shared';
import { type Component, For, Match, Show, Switch, createEffect, createSignal } from 'solid-js';

const severityClass = (severity: string) => {
  switch (severity) {
    case 'critical':
      return 'border-terra/50 bg-terra/10 text-terra';
    case 'high':
      return 'border-amber-500/40 bg-amber-500/10 text-amber-300';
    case 'medium':
      return 'border-sky-500/40 bg-sky-500/10 text-sky-300';
    default:
      return 'border-pasture-600/60 bg-pasture-800/70 text-wool-400';
  }
};

export const WorkersPane: Component = () => {
  const project = useProject();
  const route = useRoute();
  const [concerns, setConcerns] = createSignal<WorkerConcern[]>([]);
  const [workers, setWorkers] = createSignal<WorkerDisplay[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [selectedWorker, setSelectedWorker] = createSignal<WorkerDisplay | null>(null);
  const metricsAvailable = () => workers().some((worker) => worker.contextUtilization != null);

  const loadWorkers = async () => {
    const projectId = project.selectedProjectId();
    const routeId = route.currentRouteId();
    if (!projectId || !routeId) {
      setWorkers([]);
      return;
    }

    try {
      const result = await getRouteWorkers(projectId, routeId);
      setWorkers(result);
    } catch (error) {
      console.error('Failed to load route workers:', error);
    }
  };

  const loadConcerns = async () => {
    const projectId = project.selectedProjectId();
    const routeId = route.currentRouteId();
    if (!projectId || !routeId) {
      setConcerns([]);
      setLoading(false);
      return;
    }

    try {
      setLoading(true);
      await loadWorkers();
      const result = await getWorkerConcerns(projectId, routeId, { includeResolved: true, limit: 100 });
      setConcerns(result.filter((concern) => concern.kind !== 'progress'));
    } catch (error) {
      console.error('Failed to load worker concerns:', error);
    } finally {
      setLoading(false);
    }
  };

  createEffect(() => {
    const projectId = project.selectedProjectId();
    const routeId = route.currentRouteId();
    if (!projectId || !routeId) {
      setConcerns([]);
      return;
    }

    createPoll(loadConcerns, { interval: 4000, immediate: true });
  });

  const handleResolve = async (concern: WorkerConcern) => {
    try {
      await resolveWorkerConcern(concern.id);
      await markWorkerConcernRead(concern.id);
      await loadConcerns();
    } catch (error) {
      console.error('Failed to resolve concern:', error);
      window.toast?.error(`Failed to resolve concern: ${error}`);
    }
  };

  const handleMarkAllRead = async () => {
    const projectId = project.selectedProjectId();
    const routeId = route.currentRouteId();
    if (!projectId || !routeId) return;
    try {
      await markRouteConcernsRead(projectId, routeId);
      window.toast?.success('Marked route concerns as read');
    } catch (error) {
      console.error('Failed to mark route concerns read:', error);
    }
  };

  return (
    <div class="h-full min-h-0 grid grid-cols-[320px_minmax(0,1fr)] bg-pasture-900/85">
      <aside class="min-h-0 border-r border-pasture-700/60">
        <div class="border-b border-pasture-700/60 px-5 py-5">
          <p class="text-[11px] uppercase tracking-[0.18em] text-amber-400">Workers</p>
          <p class="mt-2 text-2xl text-wool-100">{workers().length}</p>
          <p class="mt-1 text-sm text-wool-500">
            {workers().filter((worker) => worker.status === 'working').length} active on this route
          </p>
        </div>

        <div class="min-h-0 overflow-y-auto p-4 space-y-3">
          <Show
            when={workers().length > 0}
            fallback={
              <div class="rounded-none border border-pasture-700/60 bg-pasture-800/40 px-4 py-6 text-sm text-wool-500">
                No workers are attached to this route.
              </div>
            }
          >
            <For each={workers()}>
              {(worker) => (
                <WorkerCard
                  worker={worker}
                  metricsAvailable={metricsAvailable()}
                  onClick={() => setSelectedWorker(worker)}
                  onDoubleClick={() => {
                    const projectId = project.selectedProjectId();
                    const routeId = route.currentRouteId();
                    if (projectId && routeId) {
                      emit('show-worker-output', { projectId, routeId, workerName: worker.name });
                    }
                  }}
                />
              )}
            </For>
          </Show>
        </div>
      </aside>

      <section class="min-h-0 flex flex-col">
        <div class="flex items-center gap-3 border-b border-pasture-700/60 px-5 py-3">
          <div>
          <p class="text-[11px] uppercase tracking-[0.18em] text-amber-400">Escalations</p>
          <p class="mt-1 text-sm text-wool-500">
              Worker blockers, risks, and decision requests for the selected route.
            </p>
          </div>
          <div class="ml-auto flex items-center gap-2">
            <button
              type="button"
              onClick={() => void handleMarkAllRead()}
              class="rounded-none border border-pasture-700/60 bg-pasture-800/60 px-3 py-1.5 text-xs text-wool-300 hover:bg-pasture-700"
            >
              Mark route read
            </button>
          </div>
        </div>

        <div class="min-h-0 flex-1 overflow-y-auto p-4">
          <Show
            when={!loading()}
            fallback={
              <div class="flex h-full items-center justify-center text-sm text-wool-500">
                Loading worker concerns…
              </div>
            }
          >
            <Switch>
              <Match when={concerns().length === 0}>
                <div class="flex h-full items-center justify-center rounded-none border border-pasture-700/60 bg-pasture-800/30 text-sm text-wool-500">
                  No worker escalations on this route.
                </div>
              </Match>
              <Match when={concerns().length > 0}>
                <div class="space-y-3">
                  <For each={concerns()}>
                    {(concern) => (
                      <article class="rounded-none border border-pasture-700/60 bg-pasture-800/45 px-4 py-3">
                        <div class="flex flex-wrap items-start gap-2">
                          <span class={`rounded-none border px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] ${severityClass(concern.severity)}`}>
                            {concern.severity}
                          </span>
                          <span class="rounded-none border border-pasture-700/60 bg-pasture-900/60 px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-wool-500">
                            {concern.kind.replaceAll('_', ' ')}
                          </span>
                          <span class="ml-auto text-xs text-wool-500">
                            {concern.workerName} · {new Date(concern.updatedAt).toLocaleString()}
                          </span>
                        </div>

                        <p class="mt-3 text-sm font-medium text-wool-100">{concern.summary}</p>
                        <Show when={concern.details}>
                          <p class="mt-2 whitespace-pre-wrap text-sm leading-6 text-wool-400">
                            {concern.details}
                          </p>
                        </Show>

                        <div class="mt-3 flex items-center gap-2">
                          <Show when={concern.status !== 'resolved'}>
                            <button
                              type="button"
                              onClick={() => void handleResolve(concern)}
                              class="rounded-none border border-pasture-700/60 bg-pasture-900/60 px-2.5 py-1 text-xs text-wool-200 hover:bg-pasture-700"
                            >
                              Resolve
                            </button>
                          </Show>
                          <button
                            type="button"
                            onClick={() => void markWorkerConcernRead(concern.id)}
                            class="rounded-none border border-pasture-700/60 bg-pasture-900/60 px-2.5 py-1 text-xs text-wool-400 hover:bg-pasture-700 hover:text-wool-200"
                          >
                            Mark read
                          </button>
                          <button
                            type="button"
                            onClick={() => {
                              const projectId = project.selectedProjectId();
                              const routeId = route.currentRouteId();
                              if (projectId && routeId) {
                                emit('show-worker-output', {
                                  projectId,
                                  routeId,
                                  workerName: concern.workerName,
                                });
                              }
                            }}
                            class="rounded-none border border-pasture-700/60 bg-pasture-900/60 px-2.5 py-1 text-xs text-wool-400 hover:bg-pasture-700 hover:text-wool-200"
                          >
                            Spectate
                          </button>
                          <Show when={concern.status === 'resolved'}>
                            <span class="ml-auto flex items-center gap-1 text-xs text-sage">
                              <Icon name="check" class="h-3.5 w-3.5" />
                              Resolved
                            </span>
                          </Show>
                        </div>
                      </article>
                    )}
                  </For>
                </div>
              </Match>
            </Switch>
          </Show>
        </div>
      </section>

      <Show when={selectedWorker()}>
        {(worker) => (
          <WorkerDetailModal
            worker={worker()}
            metricsAvailable={metricsAvailable()}
            onClose={() => setSelectedWorker(null)}
            onAttach={() => {
              const projectId = project.selectedProjectId();
              const routeId = route.currentRouteId();
              if (projectId && routeId) {
                emit('show-worker-output', { projectId, routeId, workerName: worker().name });
              }
            }}
          />
        )}
      </Show>
    </div>
  );
};
