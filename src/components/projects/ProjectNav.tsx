/**
 * Project navigation sidebar
 */
import { type Component, Show } from 'solid-js';
import { useProject, useRuns } from '../../stores';

export const ProjectNav: Component = () => {
  const project = useProject();
  const runs = useRuns();

  // Only show when a project is selected
  return (
    <Show when={project.selectedProjectId()}>
      <aside class="w-12 flex-shrink-0 border-r border-pasture-600 bg-pasture-800/50 flex flex-col items-center py-3 gap-2">
        {/* Project Info (at top) */}
        <button
          onClick={() => project.setShowProjectSettings(!project.showProjectSettings())}
          class="p-2 rounded hover:bg-pasture-700 text-wool-400 hover:text-wool-200"
          classList={{
            'bg-pasture-700 text-amber-500': project.showProjectSettings(),
          }}
        >
          <i data-lucide="info" class="w-5 h-5" />
        </button>

        {/* Board view */}
        <button
          onClick={() => {
            runs.setSelectedRun(null);
            project.setShowProjectSettings(false);
            project.setActiveProjectView('board');
          }}
          class="p-2 rounded hover:bg-pasture-700 text-wool-400 hover:text-wool-200"
          classList={{
            'bg-pasture-700 text-amber-500':
              project.activeProjectView() === 'board' && !runs.selectedRun() && !project.showProjectSettings(),
          }}
        >
          <i data-lucide="layout-dashboard" class="w-5 h-5" />
        </button>

        {/* Runs view */}
        <button
          onClick={() => {
            project.setShowProjectSettings(false);
            project.setActiveProjectView('runs');
          }}
          class="p-2 rounded hover:bg-pasture-700 text-wool-400 hover:text-wool-200"
          classList={{
            'bg-pasture-700 text-amber-500':
              project.activeProjectView() === 'runs' || !!runs.selectedRun(),
          }}
        >
          <i data-lucide="play-circle" class="w-5 h-5" />
        </button>

        <div class="flex-1" />
      </aside>
    </Show>
  );
};
