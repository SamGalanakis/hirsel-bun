/**
 * TitleBar - Minimal application header
 *
 * Shows:
 * - App name "Hirsel" on left
 * - Right side: Notifications, App settings
 */
import { type Component } from 'solid-js';
import { useApp } from '../../stores';
import { NotificationsDropdown } from './Notifications';
import { Icon } from '../shared';

export const TitleBar: Component = () => {
  const app = useApp();

  return (
    <header class="flex items-center justify-between px-3 py-1.5 border-b border-pasture-600/50 bg-pasture-900/50 backdrop-blur-sm select-none relative z-50">
      {/* Left: App name */}
      <div class="flex items-center gap-3">
        <span class="text-[13px] font-semibold text-wool-200 tracking-tight">Hirsel</span>
      </div>

      {/* Right side actions */}
      <div class="flex items-center gap-1">
        <NotificationsDropdown />

        {/* App settings */}
        <button
          type="button"
          onClick={() => app.setShowSettings(true)}
          class="p-2 rounded-md text-wool-500 hover:text-wool-300 hover:bg-pasture-800 transition-colors"
          title="Settings"
        >
          <Icon name="settings" class="w-4 h-4" />
        </button>
      </div>
    </header>
  );
};
