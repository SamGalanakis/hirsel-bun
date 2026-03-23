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
    <footer class="status-bar px-4 py-1 border-t border-pasture-700/30 flex items-center">
      <Show when={versionInfo()}>
        <span class="text-[9px] uppercase tracking-[0.2em] text-wool-700">
          v{versionInfo()?.version} ({versionInfo()?.gitSha})
        </span>
      </Show>

      <div class="flex-1" />
      <Show when={project.selectedProject()}>
        <div class="flex items-center gap-2 text-[10px] uppercase tracking-[0.15em] text-wool-600">
          <Icon name="git-branch" class="w-3 h-3" />
          <span>{route.currentRoute()?.name ?? '—'}</span>
          <Show when={workspace.machineryOpen()}>
            <span class="text-wool-700">·</span>
            <span>{workspace.activeMachineryTab()}</span>
          </Show>
        </div>
      </Show>
    </footer>
  );
};
