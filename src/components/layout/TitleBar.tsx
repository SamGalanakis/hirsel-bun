/**
 * Application title bar component with integrated breadcrumbs
 */
import { type Component, Show, For, createSignal, createEffect, onCleanup } from 'solid-js';
import { useApp, useProject, useRuns } from '../../stores';
import { Notifications } from './Notifications';
import { initLucideIcons } from '../../lib/icons';

export const TitleBar: Component = () => {
  const app = useApp();
  const project = useProject();
  const runs = useRuns();

  const [projectSelectorOpen, setProjectSelectorOpen] = createSignal(false);

  // Reinitialize icons when dialog opens
  createEffect(() => {
    if (projectSelectorOpen()) {
      queueMicrotask(() => initLucideIcons());
    }
  });

  // Close on Escape key
  createEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && projectSelectorOpen()) {
        setProjectSelectorOpen(false);
      }
    };
    document.addEventListener('keydown', handleKeyDown);
    onCleanup(() => document.removeEventListener('keydown', handleKeyDown));
  });

  const openProjectSelector = () => {
    setProjectSelectorOpen(true);
  };

  const selectProject = (proj: { id: number; name: string }) => {
    project.selectProject(proj);
    setProjectSelectorOpen(false);
  };

  return (
    <header class="title-bar flex items-center justify-between px-4 py-2 border-b border-pasture-600 select-none">
      {/* Breadcrumbs on the left */}
      <nav class="breadcrumb">
        <ol>
          {/* Projects root - opens project selector dialog */}
          <li>
            <button
              onClick={openProjectSelector}
              class="breadcrumb-link flex items-center gap-1.5 hover:text-wool-200"
            >
              <i data-lucide="folder" class="w-3.5 h-3.5" />
              Projects
            </button>
          </li>

          {/* Project name (when selected) */}
          <Show when={project.selectedProjectId()}>
            <li class="breadcrumb-separator" aria-hidden="true">
              <i data-lucide="chevron-right" class="w-3 h-3" />
            </li>
            <li>
              <button
                onClick={() => {
                  runs.setSelectedRun(null);
                  project.setActiveProjectView('board');
                }}
                class="breadcrumb-link flex items-center gap-1.5"
                classList={{
                  'text-wool-100 font-medium': !runs.selectedRun(),
                  'hover:text-wool-200': !!runs.selectedRun(),
                }}
              >
                {project.selectedProject()?.name}
              </button>
            </li>
          </Show>

          {/* Runs breadcrumb (when viewing runs or a specific run) */}
          <Show when={project.selectedProjectId() && (project.activeProjectView() === 'runs' || runs.selectedRun())}>
            <li class="breadcrumb-separator" aria-hidden="true">
              <i data-lucide="chevron-right" class="w-3 h-3" />
            </li>
          </Show>

          {/* "Runs" label when on runs view but no run selected */}
          <Show when={project.selectedProjectId() && project.activeProjectView() === 'runs' && !runs.selectedRun()}>
            <li>
              <span class="breadcrumb-page">Runs</span>
            </li>
          </Show>

          {/* "Runs" link when a run is selected */}
          <Show when={runs.selectedRun()}>
            <li>
              <button
                onClick={() => runs.setSelectedRun(null)}
                class="breadcrumb-link hover:text-wool-200"
              >
                Runs
              </button>
            </li>
          </Show>

          {/* Selected run name */}
          <Show when={runs.selectedRun()}>
            <li class="breadcrumb-separator" aria-hidden="true">
              <i data-lucide="chevron-right" class="w-3 h-3" />
            </li>
            <li>
              <span class="breadcrumb-page">{runs.selectedRun()}</span>
            </li>
          </Show>
        </ol>
      </nav>

      {/* Project Selector Dialog - Centered Modal */}
      <Show when={projectSelectorOpen()}>
        <div
          class="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
          onClick={() => setProjectSelectorOpen(false)}
        >
          <div
            class="w-96 bg-pasture-800 border border-pasture-600 rounded-lg shadow-xl"
            onClick={(e) => e.stopPropagation()}
          >
            {/* Header */}
            <div class="flex items-center justify-between p-4 border-b border-pasture-600">
              <h2 class="text-lg font-medium text-wool-100">Select Project</h2>
              <button
                onClick={() => setProjectSelectorOpen(false)}
                class="p-1 rounded hover:bg-pasture-700 text-wool-400"
              >
                <i data-lucide="x" class="w-5 h-5" />
              </button>
            </div>

            {/* Search */}
            <div class="p-4 border-b border-pasture-600">
              <div class="flex items-center gap-2 px-3 py-2 bg-pasture-900 border border-pasture-600 rounded-md focus-within:border-amber-500/50">
                <i data-lucide="search" class="w-4 h-4 text-wool-500" />
                <input
                  type="text"
                  value={project.projectSearchQuery()}
                  onInput={(e) => project.setProjectSearchQuery(e.currentTarget.value)}
                  placeholder="Search projects..."
                  class="flex-1 bg-transparent text-sm text-wool-200 placeholder-wool-500 border-none outline-none"
                  autofocus
                />
              </div>
            </div>

            {/* Project List */}
            <div class="max-h-72 overflow-y-auto">
              <Show when={project.filteredProjects().length === 0 && project.projects().length > 0}>
                <div class="px-4 py-6 text-sm text-wool-500 text-center">No projects found</div>
              </Show>
              <Show when={project.projects().length === 0}>
                <div class="px-4 py-6 text-sm text-wool-500 text-center">
                  <i data-lucide="folder-open" class="w-8 h-8 mx-auto mb-2 text-wool-600" />
                  <p>No projects yet</p>
                </div>
              </Show>
              <For each={project.filteredProjects()}>
                {(proj) => (
                  <button
                    onClick={() => selectProject(proj)}
                    class="w-full px-4 py-3 text-left hover:bg-pasture-700 flex items-center justify-between gap-3 border-b border-pasture-700 last:border-b-0"
                    classList={{
                      'bg-pasture-700/50': project.selectedProjectId() === proj.id,
                    }}
                  >
                    <span class="flex items-center gap-3 min-w-0">
                      <i data-lucide="folder" class="w-5 h-5 text-amber-500 shrink-0" />
                      <span class="truncate text-wool-200">{proj.name}</span>
                    </span>
                    <Show when={project.selectedProjectId() === proj.id}>
                      <i data-lucide="check" class="w-5 h-5 text-amber-500 shrink-0" />
                    </Show>
                  </button>
                )}
              </For>
            </div>

            {/* Footer */}
            <div class="p-4 border-t border-pasture-600">
              <button
                onClick={() => {
                  setProjectSelectorOpen(false);
                  project.openProjectSetup();
                }}
                class="w-full btn flex items-center justify-center gap-2"
              >
                <i data-lucide="plus" class="w-4 h-4" />
                <span>New Project</span>
              </button>
            </div>
          </div>
        </div>
      </Show>

      <div class="flex items-center gap-1">
        {/* Notifications */}
        <Notifications />

        {/* Help Button */}
        <button
          onClick={() => app.setShowHelp(true)}
          class="p-2 rounded hover:bg-pasture-700 text-wool-300"
        >
          <i data-lucide="help-circle" class="w-4 h-4" />
        </button>

        {/* Settings Button */}
        <button
          onClick={() => app.setShowSettings(true)}
          class="p-2 rounded hover:bg-pasture-700 text-wool-300"
        >
          <i data-lucide="settings" class="w-4 h-4" />
        </button>
      </div>
    </header>
  );
};
