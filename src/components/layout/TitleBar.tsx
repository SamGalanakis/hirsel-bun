/**
 * TitleBar - Minimal application header
 *
 * Shows:
 * - App name "Hirsel" on left
 * - Right side: Notifications, App settings
 */
import { type Component } from 'solid-js';
import { useApp, useProject } from '../../stores';
import { NotificationsDropdown } from './Notifications';
import { Icon } from '../shared';

export const TitleBar: Component = () => {
  const app = useApp();
  const project = useProject();

  return (
    <header class="flex items-center justify-between px-3 py-1.5 border-b border-pasture-600/50 bg-pasture-900/50 backdrop-blur-sm select-none relative z-50">
      {/* Left: App name */}
      <div class="flex items-center gap-3">
        <span class="text-[11px] font-medium text-white uppercase tracking-[0.2em]" style="font-family: 'Space Grotesk', sans-serif;">Hirsel</span>
        <button
          type="button"
          onClick={() => project.setProjectSelectorOpen(true)}
          class="rounded-none border border-pasture-700/60 bg-pasture-800/70 px-2.5 py-1 text-[11px] text-wool-300 transition-colors hover:bg-pasture-700"
        >
          <span class="flex items-center gap-2">
            <Icon name="folder" class="w-3.5 h-3.5 text-amber-400" />
            <span>{project.selectedProject()?.name ?? 'Projects'}</span>
          </span>
        </button>
      </div>

      {/* Right side actions */}
      <div class="flex items-center gap-1">
        <NotificationsDropdown />

        {/* App settings */}
        <button
          type="button"
          onClick={() => app.setShowSettings(true)}
          class="p-2 rounded-none text-wool-500 hover:text-wool-300 hover:bg-pasture-800 transition-colors"
          title="Settings"
        >
          <Icon name="settings" class="w-4 h-4" />
        </button>
      </div>
    </header>
  );
};
