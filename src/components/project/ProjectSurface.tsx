import { type Component, Match, Show, Switch } from 'solid-js';
import { useProjectSurface } from '../../hooks';
import { emit } from '../../lib/events';
import { useProject, useRoute, useWorkspace } from '../../stores';
import { SpecBoard } from '../specflow/SpecBoard';
import { MessagingPanel } from '../messaging';
import { Dropdown, Icon, type DropdownOption } from '../shared';

const MACHINERY_TABS = [
  { id: 'board', label: 'Board', icon: 'blocks' },
  { id: 'workers', label: 'Workers', icon: 'message-circle' },
] as const;

export const ProjectSurface: Component = () => {
  const project = useProject();
  const route = useRoute();
  const workspace = useWorkspace();
  const { surface } = useProjectSurface();
  const routeOptions = (): DropdownOption[] =>
    (surface()?.routes ?? []).map((routeSummary) => ({
      value: String(routeSummary.routeId),
      label: `${routeSummary.name} · ${routeSummary.status}`,
    }));
  const currentRouteSummary = () =>
    (surface()?.routes ?? []).find((item) => item.routeId === route.currentRouteId()) ?? null;

  return (
    <section class="flex-1 flex min-h-0 flex-col bg-pasture-900">
      <header class="border-b border-pasture-700/60 bg-pasture-900/85 px-4 py-1.5 flex items-center gap-3">
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
            triggerClass="border border-pasture-700/60 bg-transparent px-2 py-1 text-[12px] text-wool-300 hover:text-wool-100 transition-none"
            panelClass="border border-pasture-700/60 bg-pasture-900 py-1 shadow-2xl"
            class="min-w-[140px]"
          />
        </Show>
        <div class="flex-1" />
        <button
          type="button"
          onClick={() => workspace.setMachineryOpen(!workspace.machineryOpen())}
          class="shrink-0 px-2 py-1 text-[11px] uppercase tracking-[0.1em] text-wool-500 hover:text-wool-300 transition-none"
        >
          {workspace.machineryOpen() ? 'Hide' : 'Machinery'}
        </button>
      </header>

      <div class="flex-1 min-h-0 flex flex-col">

        <div class="flex-1 min-h-0 bg-pasture-950/60 p-4">
          <Show
            when={surface()}
            fallback={
              <div class="flex h-full items-center justify-center rounded-3xl border border-pasture-700/60 bg-pasture-900/70 text-sm text-wool-500">
                Loading project focus…
              </div>
            }
          >
            {(projectSurface) => (
              <div class="h-full overflow-hidden rounded-3xl border border-pasture-700/60 bg-pasture-950/70 shadow-[0_24px_80px_rgba(0,0,0,0.28)]">
                <iframe
                  title={`Project focus for ${project.selectedProject()?.name ?? 'project'}`}
                  sandbox=""
                  srcdoc={projectSurface().focusView.html}
                  class="h-full w-full bg-transparent"
                />
              </div>
            )}
          </Show>
        </div>

        <Show when={workspace.machineryOpen()}>
          <section class="h-[48%] min-h-[260px] border-t border-pasture-700/60 bg-pasture-900/96 backdrop-blur">
            <div class="flex flex-wrap items-center gap-3 border-b border-pasture-700/60 px-4 py-2.5">
              <div class="text-[11px] uppercase tracking-[0.18em] text-wool-500">Machinery</div>
              <div class="flex min-w-[260px] flex-1 items-center gap-2">
                <span class="text-[11px] uppercase tracking-[0.18em] text-wool-600">Route</span>
                <div class="min-w-[220px] max-w-[320px] flex-1">
                  <Dropdown
                    value={route.currentRouteId()?.toString() ?? ''}
                    options={routeOptions()}
                    placeholder="Select route"
                    triggerClass="w-full rounded-none border border-pasture-700/60 bg-pasture-800/70 px-3 py-2 text-left text-sm text-wool-200"
                    panelClass="rounded-none border border-pasture-700/60 bg-pasture-900 py-1 shadow-2xl"
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
                    <span class="rounded-none border border-pasture-700/60 bg-pasture-800/70 px-2.5 py-1 text-[11px] text-wool-400">
                      {summary().status}
                    </span>
                  )}
                </Show>
                <button
                  type="button"
                  onClick={() => emit('open-fork-dialog')}
                  class="rounded-none border border-pasture-700/60 bg-pasture-800/70 px-3 py-2 text-sm text-wool-300 transition-colors hover:bg-pasture-700"
                >
                  <span class="flex items-center gap-2">
                    <Icon name="copy-plus" class="w-4 h-4" />
                    <span>Fork route</span>
                  </span>
                </button>
              </div>
              <div class="ml-auto flex items-center gap-1">
                {MACHINERY_TABS.map((tab) => (
                  <button
                    type="button"
                    onClick={() => workspace.setActiveMachineryTab(tab.id)}
                    class="rounded-none px-3 py-1.5 text-xs transition-colors"
                    classList={{
                      'bg-amber-500/15 text-amber-300': workspace.activeMachineryTab() === tab.id,
                      'text-wool-500 hover:bg-pasture-800 hover:text-wool-300':
                        workspace.activeMachineryTab() !== tab.id,
                    }}
                  >
                    <span class="flex items-center gap-1.5">
                      <Icon name={tab.icon} class="w-3.5 h-3.5" />
                      <span>{tab.label}</span>
                    </span>
                  </button>
                ))}
              </div>
            </div>

            <div class="h-[calc(100%-49px)] min-h-0">
              <Switch>
                <Match when={workspace.activeMachineryTab() === 'board'}>
                  <SpecBoard />
                </Match>
                <Match when={workspace.activeMachineryTab() === 'workers'}>
                  <MessagingPanel />
                </Match>
              </Switch>
            </div>
          </section>
        </Show>
      </div>
    </section>
  );
};
