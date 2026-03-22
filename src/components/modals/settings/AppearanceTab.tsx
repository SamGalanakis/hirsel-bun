/**
 * Appearance tab - theme selection
 */
import { type Component, For, Show, type Accessor } from 'solid-js';
import { type ThemeId, THEMES, THEME_LIST } from '../../../lib/theme';
import { Icon } from '../../shared';

export interface AppearanceTabProps {
  currentTheme: Accessor<ThemeId>;
  setTheme: (id: ThemeId) => void;
  themeSelectorOpen: Accessor<boolean>;
  setThemeSelectorOpen: (open: boolean) => void;
}

export const AppearanceTab: Component<AppearanceTabProps> = (props) => {
  const currentThemeDisplay = () => THEMES[props.currentTheme()].name;
  const currentSwatches = () => THEMES[props.currentTheme()].swatches;

  const handleThemeSelect = (themeId: ThemeId) => {
    props.setTheme(themeId);
    props.setThemeSelectorOpen(false);
  };

  return (
    <div class="max-w-2xl mx-auto space-y-6">
      <div>
        <h3 class="text-lg font-medium text-wool-100 mb-1">Theme</h3>
        <p class="text-sm text-wool-500">Customize the appearance of the application.</p>
      </div>

      {/* Theme Selector */}
      <div>
        <label class="block text-sm font-medium text-wool-300 mb-2">
          Color Theme
        </label>
        <div class="dropdown relative">
          <button
            onClick={() => props.setThemeSelectorOpen(!props.themeSelectorOpen())}
            class="btn-outline w-full justify-between"
            classList={{ 'ring-2 ring-primary': props.themeSelectorOpen() }}
            aria-haspopup="listbox"
            aria-expanded={props.themeSelectorOpen()}
          >
            <span class="flex items-center gap-3">
              <span class="flex gap-1">
                <For each={currentSwatches()}>
                  {(color) => (
                    <span
                      class="w-3.5 h-3.5 rounded-none border border-white/20"
                      style={{ background: color }}
                    />
                  )}
                </For>
              </span>
              <span class="text-wool-200">{currentThemeDisplay()}</span>
            </span>
            <Icon name="chevrons-up-down" class="w-4 h-4 opacity-50" />
          </button>

          <Show when={props.themeSelectorOpen()}>
            <div
              data-popover
              class="absolute z-50 mt-1 w-full bg-popover border border-border rounded-none shadow-xl max-h-80 overflow-y-auto"
            >
              <div role="listbox">
                <For each={THEME_LIST}>
                  {(theme) => (
                    <div
                      role="option"
                      aria-selected={props.currentTheme() === theme.id}
                      class="px-3 py-2.5 cursor-pointer hover:bg-accent flex items-center justify-between"
                      classList={{ 'bg-accent/50': props.currentTheme() === theme.id }}
                      onClick={() => handleThemeSelect(theme.id)}
                    >
                      <span class="flex items-center gap-3">
                        <span class="flex gap-1">
                          <For each={theme.swatches}>
                            {(color) => (
                              <span
                                class="w-3.5 h-3.5 rounded-none border border-white/20"
                                style={{ background: color }}
                              />
                            )}
                          </For>
                        </span>
                        <span class="text-wool-200">{theme.name}</span>
                      </span>
                      <Show when={props.currentTheme() === theme.id}>
                        <Icon name="check" class="w-4 h-4 text-primary" />
                      </Show>
                    </div>
                  )}
                </For>
              </div>
            </div>
          </Show>
        </div>
        <p class="text-xs text-wool-500 mt-2">
          {THEMES[props.currentTheme()].description}
        </p>
      </div>
    </div>
  );
};
