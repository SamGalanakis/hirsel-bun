/**
 * Status bar component
 */
import { type Component, Show } from 'solid-js';
import { useApp, useRuns } from '../../stores';
import { Icon } from '../shared';
import { ShepherdConsoleBar } from '../chat/ShepherdConsole';

export const StatusBar: Component = () => {
  const app = useApp();
  const runs = useRuns();

  const detail = () => runs.runDetail();
  const versionInfo = () => app.versionInfo();

  // Extract display strings from remote URL
  const remoteDisplay = () => {
    const url = detail()?.remoteUrl;
    if (!url) return '';
    return url
      .replace(/^https?:\/\//, '')
      .replace(/\.git$/, '')
      .split('/')
      .slice(-2)
      .join('/');
  };

  // Extract display path
  const pathDisplay = () => {
    const path = detail()?.projectPath;
    if (!path) return '';
    return path.split('/').slice(-2).join('/');
  };

  return (
    <footer class="status-bar px-4 py-1.5 border-t border-pasture-600 flex items-center text-xs">
      {/* Version info (left) */}
      <Show when={versionInfo()}>
        <div class="text-wool-600 mr-4">
          v{versionInfo()?.version} ({versionInfo()?.gitSha})
        </div>
      </Show>

      <div class="flex items-center gap-3 text-wool-600">
        {/* Remote indicator */}
        <Show when={detail()?.remoteUrl}>
          <span
            class="flex items-center gap-1.5 text-amber-400"
            data-tooltip={detail()?.remoteUrl}
            data-side="top"
          >
            <Icon name="cloud" class="w-3 h-3 flex-shrink-0" />
            <span class="truncate max-w-[200px]">{remoteDisplay()}</span>
          </span>
        </Show>

        {/* Project path (only show for local repos) */}
        <Show when={detail()?.projectPath && !detail()?.remoteUrl}>
          <span
            class="flex items-center gap-1.5 max-w-md truncate"
            data-tooltip={detail()?.projectPath}
            data-side="top"
          >
            <Icon name="folder" class="w-3 h-3 flex-shrink-0" />
            <span class="truncate">{pathDisplay()}</span>
          </span>
        </Show>

        {/* Git branch hint */}
        <Show when={detail()?.name}>
          <span class="flex items-center gap-1.5">
            <Icon name="git-branch" class="w-3 h-3 flex-shrink-0" />
            <span>hirsel/{detail()?.name}</span>
          </span>
        </Show>
      </div>

      {/* Spacer */}
      <div class="flex-1" />

      {/* Shepherd Console Bar */}
      <ShepherdConsoleBar />
    </footer>
  );
};
