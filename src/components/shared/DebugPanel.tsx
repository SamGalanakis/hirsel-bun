/**
 * Debug panel component - only shown in development via Ctrl+D
 */
import { type Component, Show, createEffect, createSignal, onCleanup, onMount } from 'solid-js';
import { Icon } from './Icon';

export const DebugPanel: Component = () => {
  const [visible, setVisible] = createSignal(false);
  const [isDev, setIsDev] = createSignal(false);

  onMount(() => {
    // Check if in dev mode
    setIsDev(import.meta.env.DEV);
  });

  // Ctrl+D keyboard shortcut (only in dev mode)
  createEffect(() => {
    if (!isDev()) return;

    const handleKeydown = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key === 'd') {
        e.preventDefault();
        setVisible((v) => !v);
      }
      // Also close on Escape
      if (e.key === 'Escape' && visible()) {
        setVisible(false);
      }
    };

    window.addEventListener('keydown', handleKeydown);
    onCleanup(() => window.removeEventListener('keydown', handleKeydown));
  });

  // Only render in development
  return (
    <Show when={isDev() && visible()}>
      <div
        class="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
        onClick={(e) => {
          if (e.target === e.currentTarget) setVisible(false);
        }}
      >
        <div class="bg-pasture-800 border border-pasture-600 rounded-lg shadow-xl p-4 w-96">
          <div class="flex items-center justify-between mb-3">
            <h3 class="text-sm font-medium text-wool-300 flex items-center gap-2">
              <Icon name="bug" class="w-4 h-4" />
              Debug Panel
            </h3>
            <button
              onClick={() => setVisible(false)}
              class="p-1 rounded hover:bg-pasture-700 text-wool-500"
            >
              <Icon name="x" class="w-4 h-4" />
            </button>
          </div>
          <div class="text-xs text-wool-500 space-y-2">
            <p>Development mode enabled</p>
            <p class="text-wool-600">Press Ctrl+D to toggle this panel</p>
            <hr class="border-pasture-600 my-3" />
            <p>Check console for logs</p>
          </div>
        </div>
      </div>
    </Show>
  );
};
