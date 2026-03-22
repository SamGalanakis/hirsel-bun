/**
 * Shortcuts tab - keyboard shortcut configuration
 */
import { type Component, For, Show, type Accessor } from 'solid-js';
import {
  type ShortcutConfig,
  type ShortcutAction,
  formatBinding,
} from '../../../lib/shortcuts';
import { Icon } from '../../shared';

export interface ShortcutsTabProps {
  shortcuts: Accessor<ShortcutConfig[]>;
  rebindingAction: Accessor<ShortcutAction | null>;
  shortcutConflict: Accessor<string>;
  startRebind: (action: ShortcutAction) => void;
  resetAllShortcuts: () => void;
}

export const ShortcutsTab: Component<ShortcutsTabProps> = (props) => {
  const getShortcutsByCategory = (category: string) => {
    return props.shortcuts().filter((s) => s.category === category);
  };

  const renderShortcutList = (shortcuts: ShortcutConfig[], lastBorderless?: boolean) => (
    <div class="space-y-1 text-sm">
      <For each={shortcuts}>
        {(shortcut) => (
          <div class={`flex justify-between items-center py-1.5 border-b border-pasture-700${lastBorderless ? ' last:border-b-0' : ''}`}>
            <span class="text-wool-300">{shortcut.label}</span>
            <button
              onClick={() => props.startRebind(shortcut.action)}
              class="cursor-pointer transition-all rounded-none px-1"
              classList={{
                'ring-2 ring-amber-500': props.rebindingAction() === shortcut.action,
                'hover:bg-pasture-600': props.rebindingAction() !== shortcut.action,
              }}
            >
              <Show when={props.rebindingAction() !== shortcut.action}>
                <kbd class="kbd text-wool-500">{formatBinding(shortcut.binding)}</kbd>
              </Show>
              <Show when={props.rebindingAction() === shortcut.action}>
                <span class="kbd text-amber-400 animate-pulse">Press key...</span>
              </Show>
            </button>
          </div>
        )}
      </For>
    </div>
  );

  return (
    <div class="max-w-2xl mx-auto space-y-6">
      <div class="flex items-center justify-between">
        <div>
          <h3 class="text-lg font-medium text-wool-100 mb-1">Keyboard Shortcuts</h3>
          <p class="text-sm text-wool-500">Click any shortcut to rebind. Press Escape to cancel.</p>
        </div>
        <button onClick={props.resetAllShortcuts} class="btn btn-ghost btn-sm">
          <Icon name="rotate-ccw" class="w-4 h-4 mr-1" />
          Reset
        </button>
      </div>

      <Show when={props.shortcutConflict()}>
        <div class="bg-amber-500/10 border border-amber-500/30 rounded-none p-3 text-sm text-amber-400 flex items-center gap-2">
          <Icon name="alert-triangle" class="w-4 h-4" />
          <span>{props.shortcutConflict()}</span>
        </div>
      </Show>

      {/* Navigation */}
      <div class="space-y-2">
        <h4 class="text-sm font-medium text-wool-200">Navigation</h4>
        {renderShortcutList(getShortcutsByCategory('navigation'))}
      </div>

      {/* Run Controls */}
      <div class="space-y-2">
        <h4 class="text-sm font-medium text-wool-200">Run Controls</h4>
        {renderShortcutList(getShortcutsByCategory('run-controls'))}
      </div>

      {/* Other */}
      <div class="space-y-2">
        <h4 class="text-sm font-medium text-wool-200">Other</h4>
        {renderShortcutList(getShortcutsByCategory('other'), true)}
      </div>
    </div>
  );
};
