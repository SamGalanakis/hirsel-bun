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
import { Icon, ProjectIcon } from '../shared';

export const TitleBar: Component = () => {
  const app = useApp();
  const project = useProject();

  return (
    <header class="title-bar flex items-center justify-between px-4 py-1.5 border-b border-pasture-700/40 select-none relative z-50">
      {/* Left: App name + project */}
      <div class="no-drag flex items-center gap-4">
        <span class="text-[10px] font-medium text-wool-300 uppercase tracking-[0.3em]">Hirsel</span>
        <div class="h-3 w-px bg-pasture-700/50" />
        <div class="flex items-center gap-1">
          <button
            type="button"
            onClick={() => project.setProjectSelectorOpen(true)}
            class="border border-pasture-700/40 bg-pasture-800/50 px-2.5 py-1 text-[11px] text-wool-300 hover:border-pasture-600 hover:text-wool-100"
          >
            <span class="flex items-center gap-2">
              {project.selectedProject() ? (
                <ProjectIcon
                  name={project.selectedProject()!.name}
                  icon={project.selectedProject()!.icon}
                  size={16}
                />
              ) : (
                <Icon name="folder" class="w-3 h-3 text-wool-500" />
              )}
              <span>{project.selectedProject()?.name ?? 'Projects'}</span>
            </span>
          </button>
          <button
            type="button"
            onClick={() => project.setShowProjectSettings(true)}
            disabled={!project.selectedProject()}
            class="border border-pasture-700/40 bg-pasture-800/50 px-1.5 py-1 text-wool-500 hover:border-pasture-600 hover:text-wool-300 disabled:cursor-not-allowed disabled:opacity-30"
            title="Project settings"
          >
            <Icon name="settings" class="w-3 h-3" />
          </button>
        </div>
      </div>

      {/* Right side actions */}
      <div class="no-drag flex items-center gap-0.5">
        <NotificationsDropdown />
        <button
          type="button"
          onClick={() => app.setShowSettings(true)}
          class="p-1.5 text-wool-600 hover:text-wool-300"
          title="Settings"
        >
          <Icon name="settings" class="w-3.5 h-3.5" />
        </button>
      </div>
    </header>
  );
};
