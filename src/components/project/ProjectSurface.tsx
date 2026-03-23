import { emit } from '../../lib/events';
import { type Component, Match, Show, Switch, createEffect, createSignal, on } from 'solid-js';
import { useProjectSurface } from '../../hooks';
import { useProject, useRoute, useWorkspace } from '../../stores';
import { WorkersPane } from '../workers';
import { WorkTreePane } from '../worktree';
import { Dropdown, Icon, type DropdownOption } from '../shared';
import { DeliveryDialog } from '../specflow/DeliveryDialog';
import { ForkRouteDialog } from '../specflow/ForkRouteDialog';

const MACHINERY_TABS = [
  { id: 'work', label: 'Work', icon: 'blocks' },
  { id: 'workers', label: 'Workers', icon: 'bot' },
] as const;

export const ProjectSurface: Component = () => {
  const project = useProject();
  const route = useRoute();
  const workspace = useWorkspace();
  const { surface } = useProjectSurface();
  const [showForkDialog, setShowForkDialog] = createSignal(false);
  const [showDeliveryDialog, setShowDeliveryDialog] = createSignal(false);
  const [syncPromptDismissed, setSyncPromptDismissed] = createSignal(false);
  const routeOptions = (): DropdownOption[] =>
    (surface()?.routes ?? []).map((routeSummary) => ({
      value: String(routeSummary.routeId),
      label: `${routeSummary.name} · ${routeSummary.status}`,
    }));
  const currentRouteSummary = () =>
    (surface()?.routes ?? []).find((item) => item.routeId === route.currentRouteId()) ?? null;
  const hasGeneratedFocus = () =>
    !['placeholder', 'seed'].includes(surface()?.focusView.source ?? '');
  const showSyncPrompt = () => !hasGeneratedFocus() && !syncPromptDismissed();

  createEffect(
    on(
      () => project.selectedProjectId(),
      () => setSyncPromptDismissed(false),
    ),
  );

  return (
    <section class="flex-1 flex min-h-0 flex-col bg-pasture-900">
      <header class="border-b border-pasture-700/30 bg-pasture-900/90 px-4 py-1.5 flex items-center gap-3">
        {/* Route selector */}
        <Show when={surface()?.routes.length}>
          <Dropdown
            value={route.currentRouteId()?.toString() ?? ''}
            options={routeOptions()}
            placeholder="Route"
            onChange={(value) => {
              const routeId = Number(value);
              if (Number.isFinite(routeId)) {
                void route.setActiveRoute(routeId);
              }
            }}
            triggerClass="border border-pasture-700/40 bg-transparent px-2 py-1 text-[11px] text-wool-300 hover:text-wool-100"
            panelClass="border border-pasture-700/40 bg-pasture-900 py-1 shadow-2xl"
            class="min-w-[140px]"
          />
        </Show>
        <div class="flex-1" />
        <button
          type="button"
          onClick={() => workspace.setMachineryOpen(!workspace.machineryOpen())}
          class="shrink-0 px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-wool-600 hover:text-wool-300"
        >
          {workspace.machineryOpen() ? 'Hide' : 'Machinery'}
        </button>
      </header>

      <div class="flex-1 min-h-0 flex flex-col">

        <div class="flex-1 min-h-0 bg-pasture-900/80 p-4">
          <Show
            when={surface()}
            fallback={
              <div class="flex h-full items-center justify-center border border-pasture-700/30 text-[11px] uppercase tracking-[0.15em] text-wool-600">
                Loading…
              </div>
            }
          >
            {(projectSurface) => (
              <Show
                when={hasGeneratedFocus()}
                fallback={
                  <div class="flex h-full items-center justify-center border border-pasture-700/30">
                    <div class="flex flex-col items-center gap-6 text-center">
                      {/* Architectural grid motif */}
                      <svg class="w-10 h-10 text-wool-700/50" viewBox="0 0 40 40" fill="none" stroke="currentColor" stroke-width="0.75">
                        <rect x="4" y="4" width="32" height="32" />
                        <line x1="4" y1="20" x2="36" y2="20" />
                        <line x1="20" y1="4" x2="20" y2="36" />
                        <rect x="12" y="12" width="16" height="16" opacity="0.35" />
                      </svg>
                      <p class="text-[10px] uppercase tracking-[0.25em] text-wool-600">
                        Awaiting project focus
                      </p>
                      <Show when={showSyncPrompt()}>
                        <div class="mt-1 flex items-center gap-2">
                          <button
                            type="button"
                            onClick={() => emit('start-project-sync', { force: true })}
                            class="border border-wool-700/50 px-4 py-1.5 text-[10px] uppercase tracking-[0.18em] text-wool-300 hover:border-wool-500 hover:text-wool-100"
                          >
                            <span class="flex items-center gap-2">
                              <Icon name="refresh-cw" class="h-3 w-3" />
                              <span>Sync</span>
                            </span>
                          </button>
                          <button
                            type="button"
                            onClick={() => setSyncPromptDismissed(true)}
                            class="px-3 py-1.5 text-[10px] uppercase tracking-[0.18em] text-wool-600 hover:text-wool-400"
                          >
                            Dismiss
                          </button>
                        </div>
                      </Show>
                    </div>
                  </div>
                }
              >
                <div class="h-full overflow-hidden border border-pasture-700/30">
                  <iframe
                    title={`Project focus for ${project.selectedProject()?.name ?? 'project'}`}
                    sandbox="allow-scripts"
                    srcdoc={projectSurface().focusView.html}
                    class="h-full w-full bg-transparent"
                  />
                </div>
              </Show>
            )}
          </Show>
        </div>

        <Show when={workspace.machineryOpen()}>
          <section class="h-[48%] min-h-[260px] border-t border-pasture-700/30 bg-pasture-900/96 backdrop-blur">
            <div class="flex flex-wrap items-center gap-3 border-b border-pasture-700/30 px-4 py-2">
              <div class="text-[10px] uppercase tracking-[0.25em] text-wool-600">Machinery</div>
              <div class="flex min-w-[260px] flex-1 items-center gap-2">
                <span class="text-[10px] uppercase tracking-[0.2em] text-wool-600">Route</span>
                <div class="min-w-[220px] max-w-[320px] flex-1">
                  <Dropdown
                    value={route.currentRouteId()?.toString() ?? ''}
                    options={routeOptions()}
                    placeholder="Select route"
                    triggerClass="w-full rounded-none border border-pasture-700/30 bg-pasture-800/50 px-3 py-1.5 text-left text-[12px] text-wool-200"
                    panelClass="rounded-none border border-pasture-700/30 bg-pasture-900 py-1 shadow-2xl"
                    onChange={(value) => {
                      const routeId = Number(value);
                      if (Number.isFinite(routeId)) {
                        void route.setActiveRoute(routeId);
                      }
                    }}
                  />
                </div>
                <Show when={currentRouteSummary()}>
                  {(summary) => (
                    <span class="rounded-none border border-pasture-700/30 bg-pasture-800/70 px-2.5 py-1 text-[11px] text-wool-400">
                      {summary().status}
                    </span>
                  )}
                </Show>
                <button
                  type="button"
                  onClick={() => setShowForkDialog(true)}
                  class="border border-pasture-700/30 bg-pasture-800/50 px-2.5 py-1.5 text-[11px] text-wool-400 hover:border-pasture-600 hover:text-wool-200"
                >
                  <span class="flex items-center gap-1.5">
                    <Icon name="copy-plus" class="w-3.5 h-3.5" />
                    <span>Fork</span>
                  </span>
                </button>
                <button
                  type="button"
                  onClick={() => setShowDeliveryDialog(true)}
                  class="border border-pasture-700/30 bg-pasture-800/50 px-2.5 py-1.5 text-[11px] text-wool-400 hover:border-pasture-600 hover:text-wool-200"
                >
                  <span class="flex items-center gap-1.5">
                    <Icon name="package" class="w-3.5 h-3.5" />
                    <span>Deliver</span>
                  </span>
                </button>
                <button
                  type="button"
                  onClick={() => {
                    const routeId = route.currentRouteId();
                    if (routeId) {
                      void route.archiveRoute(routeId);
                    }
                  }}
                  disabled={surface()?.routes.length === 1}
                  class="border border-pasture-700/30 bg-pasture-800/50 px-2.5 py-1.5 text-[11px] text-wool-400 hover:border-pasture-600 hover:text-wool-200 disabled:cursor-not-allowed disabled:opacity-30"
                >
                  <span class="flex items-center gap-1.5">
                    <Icon name="archive" class="w-3.5 h-3.5" />
                    <span>Archive</span>
                  </span>
                </button>
              </div>
              <div class="ml-auto flex items-center gap-0.5">
                {MACHINERY_TABS.map((tab) => (
                  <button
                    type="button"
                    onClick={() => workspace.setActiveMachineryTab(tab.id)}
                    class="px-2.5 py-1 text-[11px]"
                    classList={{
                      'text-wool-100 border-b border-wool-300': workspace.activeMachineryTab() === tab.id,
                      'text-wool-600 hover:text-wool-300':
                        workspace.activeMachineryTab() !== tab.id,
                    }}
                  >
                    <span class="flex items-center gap-1.5">
                      <Icon name={tab.icon} class="w-3 h-3" />
                      <span>{tab.label}</span>
                    </span>
                  </button>
                ))}
              </div>
            </div>

            <div class="h-[calc(100%-49px)] min-h-0">
              <Switch>
                <Match when={workspace.activeMachineryTab() === 'work'}>
                  <WorkTreePane />
                </Match>
                <Match when={workspace.activeMachineryTab() === 'workers'}>
                  <WorkersPane />
                </Match>
              </Switch>
            </div>
          </section>
        </Show>
      </div>
      <Show when={showForkDialog()}>
        <ForkRouteDialog onClose={() => setShowForkDialog(false)} />
      </Show>
      <Show when={showDeliveryDialog()}>
        <DeliveryDialog onClose={() => setShowDeliveryDialog(false)} />
      </Show>
    </section>
  );
};
