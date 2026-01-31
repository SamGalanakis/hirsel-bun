/**
 * Application title bar with project selector breadcrumbs
 */
import { type Component, Show, createEffect, createSignal, onMount } from 'solid-js';
import { useApp, useProject, useRuns } from '../../stores';
import { NotificationsDropdown } from './Notifications';
import { ProjectSelector } from './ProjectSelector';
import { initLucideIcons } from '../../lib/icons';

export const TitleBar: Component = () => {
  const app = useApp();
  const project = useProject();
  const runs = useRuns();

  onMount(() => initLucideIcons());
  createEffect(() => {
    project.selectedProjectId();
    runs.selectedRun();
    queueMicrotask(initLucideIcons);
  });

  return (
    <header class="flex items-center justify-between px-4 py-2 border-b border-pasture-600/50 bg-pasture-900/50 backdrop-blur-sm select-none relative z-50">
      {/* Breadcrumbs */}
      <nav aria-label="Breadcrumb">
        <ol class="flex items-center gap-1 text-sm">
          {/* Project selector (root) */}
          <li>
            <ProjectSelector />
          </li>

          {/* Board/Runs crumb - only show when project is selected */}
          <Show when={project.selectedProject()}>
            <li class="text-wool-700" aria-hidden="true">
              <i data-lucide="chevron-right" class="w-4 h-4" />
            </li>
            <li>
              <button
                type="button"
                onClick={() => {
                  runs.setSelectedRun(null);
                  project.setActiveProjectView('board');
                }}
                class="flex items-center gap-1.5 px-2 py-1 rounded-md transition-colors"
                classList={{
                  'text-wool-100 bg-pasture-700/50': project.activeProjectView() === 'board' && !runs.selectedRun(),
                  'text-wool-500 hover:text-wool-300 hover:bg-pasture-800': project.activeProjectView() !== 'board' || !!runs.selectedRun(),
                }}
              >
                <i data-lucide="layout-grid" class="w-3.5 h-3.5" />
                <span>Board</span>
              </button>
            </li>
          </Show>

          {/* Runs crumb */}
          <Show when={project.selectedProject() && (project.activeProjectView() === 'runs' || runs.selectedRun())}>
            <li class="text-wool-700" aria-hidden="true">
              <i data-lucide="chevron-right" class="w-4 h-4" />
            </li>
            <li>
              <Show
                when={runs.selectedRun()}
                fallback={
                  <span class="flex items-center gap-1.5 px-2 py-1 text-wool-100 bg-pasture-700/50 rounded-md">
                    <i data-lucide="play" class="w-3.5 h-3.5" />
                    <span>Runs</span>
                  </span>
                }
              >
                <button
                  type="button"
                  onClick={() => runs.setSelectedRun(null)}
                  class="flex items-center gap-1.5 px-2 py-1 rounded-md text-wool-500 hover:text-wool-300 hover:bg-pasture-800 transition-colors"
                >
                  <i data-lucide="play" class="w-3.5 h-3.5" />
                  <span>Runs</span>
                </button>
              </Show>
            </li>
          </Show>

          {/* Selected run crumb */}
          <Show when={runs.selectedRun()}>
            <li class="text-wool-700" aria-hidden="true">
              <i data-lucide="chevron-right" class="w-4 h-4" />
            </li>
            <li>
              <span class="flex items-center gap-1.5 px-2 py-1 text-wool-100 bg-pasture-700/50 rounded-md">
                <span class="max-w-[200px] truncate">{runs.selectedRun()}</span>
              </span>
            </li>
          </Show>
        </ol>
      </nav>

      {/* Right side actions */}
      <div class="flex items-center gap-1">
        <NotificationsDropdown />

        {/* Project settings - only show when project selected */}
        <Show when={project.selectedProject()}>
          <button
            type="button"
            onClick={() => project.setShowProjectSettings(true)}
            class="p-2 rounded-md text-wool-500 hover:text-wool-300 hover:bg-pasture-800 transition-colors"
            title={`${project.selectedProject()?.name} settings`}
          >
            <i data-lucide="folder-cog" class="w-4 h-4" />
          </button>
        </Show>

        <button
          type="button"
          onClick={() => app.setShowHelp(true)}
          class="p-2 rounded-md text-wool-500 hover:text-wool-300 hover:bg-pasture-800 transition-colors"
          title="Help"
        >
          <i data-lucide="help-circle" class="w-4 h-4" />
        </button>

        <button
          type="button"
          onClick={() => app.setShowSettings(true)}
          class="p-2 rounded-md text-wool-500 hover:text-wool-300 hover:bg-pasture-800 transition-colors"
          title="Settings"
        >
          <i data-lucide="settings" class="w-4 h-4" />
        </button>
      </div>
    </header>
  );
};
