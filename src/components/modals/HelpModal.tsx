/**
 * Help modal showing keyboard shortcuts and documentation
 */
import { type Component, For, Show } from 'solid-js';
import { useApp } from '../../stores';
import {
  type ShortcutCategory,
  getCategoryLabel,
  getShortcutsByCategory,
} from '../../lib/shortcuts';

export const HelpModal: Component = () => {
  const app = useApp();

  const shortcutsByCategory = () => getShortcutsByCategory(app.shortcuts());
  const categories: ShortcutCategory[] = [
    'navigation',
    'run-controls',
    'communication',
    'other',
  ];

  return (
    <Show when={app.showHelp()}>
      <div
        class="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
        onClick={(e) => {
          if (e.target === e.currentTarget) app.setShowHelp(false);
        }}
        onKeyDown={(e) => {
          if (e.key === 'Escape') app.setShowHelp(false);
        }}
      >
        <div class="bg-pasture-800 border border-pasture-600 rounded-lg shadow-xl w-[600px] max-h-[80vh] overflow-hidden">
          <div class="p-4 border-b border-pasture-600 flex items-center justify-between">
            <h2 class="text-lg font-medium text-wool-100">Keyboard Shortcuts</h2>
            <button
              onClick={() => app.setShowHelp(false)}
              class="p-1 rounded hover:bg-pasture-700 text-wool-500"
            >
              <i data-lucide="x" class="w-4 h-4" />
            </button>
          </div>

          <div class="p-4 overflow-y-auto max-h-[calc(80vh-100px)]">
            <For each={categories}>
              {(category) => (
                <div class="mb-6 last:mb-0">
                  <h3 class="text-xs font-medium text-wool-500 uppercase tracking-wide mb-3">
                    {getCategoryLabel(category)}
                  </h3>
                  <div class="space-y-2">
                    <For each={shortcutsByCategory()[category]}>
                      {(shortcut) => (
                        <div class="flex items-center justify-between py-1.5">
                          <span class="text-sm text-wool-300">
                            {shortcut.label}
                          </span>
                          <kbd class="px-2 py-1 text-xs font-mono bg-pasture-700 border border-pasture-600 rounded text-wool-200">
                            {app.formatBinding(shortcut.binding)}
                          </kbd>
                        </div>
                      )}
                    </For>
                  </div>
                </div>
              )}
            </For>

            <div class="mt-6 pt-4 border-t border-pasture-600">
              <p class="text-xs text-wool-500">
                Press <kbd class="px-1 py-0.5 text-[10px] bg-pasture-700 rounded">?</kbd> to toggle this help panel.
                Customize shortcuts in Settings.
              </p>
            </div>
          </div>
        </div>
      </div>
    </Show>
  );
};
