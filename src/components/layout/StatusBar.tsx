/**
 * Status bar component
 */
import { type Component, Show } from 'solid-js';
import { useApp, useProject, useRoute, useWorkspace } from '../../stores';
import { Icon } from '../shared';

export const StatusBar: Component = () => {
  const app = useApp();
  const project = useProject();
  const route = useRoute();
  const workspace = useWorkspace();

  const versionInfo = () => app.versionInfo();

  return (
    <footer class="status-bar px-4 py-1.5 border-t border-pasture-600 flex items-center text-xs">
      {/* Version info (left) */}
      <Show when={versionInfo()}>
        <div class="text-wool-600 mr-4">
          v{versionInfo()?.version} ({versionInfo()?.gitSha})
        </div>
      </Show>

      <div class="flex-1" />
      <Show when={project.selectedProject()}>
        <div class="flex items-center gap-2 text-xs text-wool-500">
          <Icon name="git-branch" class="w-3.5 h-3.5" />
          <span>{route.currentRoute()?.name ?? 'No active route'}</span>
          <Show when={workspace.machineryOpen()}>
            <span class="text-wool-700">/</span>
            <span class="capitalize">{workspace.activeMachineryTab()}</span>
          </Show>
        </div>
      </Show>
    </footer>
  );
};
