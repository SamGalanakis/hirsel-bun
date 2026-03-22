import { invoke } from '../../lib/invoke';
import {
  type Component,
  Show,
  createEffect,
  createSignal,
  onCleanup,
} from 'solid-js';
import { createStore } from 'solid-js/store';
import { useApp } from '../../stores';
import {
  bindingFromEvent,
  findConflict,
  getShortcuts,
  resetShortcuts,
  saveShortcuts,
  type ShortcutAction,
  type ShortcutConfig,
} from '../../lib/shortcuts';
import { Icon } from '../shared';
import {
  AboutTab,
  AppearanceTab,
  BackendTab,
  type BackendHealth,
  type BackendSection,
  type BackendTabRef,
  defaultSettings,
  type Settings,
  type SettingsTab,
  ShortcutsTab,
} from './settings';

export const SettingsModal: Component = () => {
  const app = useApp();
  const [activeTab, setActiveTab] = createSignal<SettingsTab>('appearance');
  const [backendSection, setBackendSection] = createSignal<BackendSection>('connection');
  const [themeSelectorOpen, setThemeSelectorOpen] = createSignal(false);

  const [settings, setSettings] = createStore<Settings>(defaultSettings());
  const [loading, setLoading] = createSignal(true);
  const [saving, setSaving] = createSignal(false);
  const [backendHealth, setBackendHealth] = createSignal<BackendHealth | null>(null);

  const [shortcuts, setShortcuts] = createSignal<ShortcutConfig[]>(getShortcuts());
  const [rebindingAction, setRebindingAction] = createSignal<ShortcutAction | null>(null);
  const [shortcutConflict, setShortcutConflict] = createSignal('');

  let backendRef: BackendTabRef | undefined;

  createEffect(() => {
    if (app.showSettings()) {
      void loadSettings();
    }
  });

  const loadSettings = async () => {
    setLoading(true);
    try {
      const config = await invoke<Partial<Settings>>('get_config');
      setSettings({ ...defaultSettings(), ...config });
      await backendRef?.initCredentialState();
      setShortcuts(getShortcuts());

      if (config.backend?.url) {
        await checkBackendHealth(config.backend.url, config.backend.apiKey);
      } else {
        setBackendHealth(null);
      }
    } catch (e) {
      console.error('Failed to load settings:', e);
    } finally {
      setLoading(false);
    }
  };

  const checkBackendHealth = async (url = settings.backend.url, apiKey = settings.backend.apiKey) => {
    const trimmedUrl = (url || '').trim();
    if (!trimmedUrl) {
      setBackendHealth(null);
      return;
    }

    setBackendHealth({ status: 'checking' });
    try {
      await invoke('check_backend_health', { url: trimmedUrl, apiKey: apiKey?.trim() || null });
      setBackendHealth({ status: 'online' });
    } catch (e) {
      const error = String(e);
      if (error.includes('auth') || error.includes('401') || error.includes('403')) {
        setBackendHealth({ status: 'online', error: 'auth error' });
      } else {
        setBackendHealth({ status: 'offline', error });
      }
    }
  };

  const startRebind = (action: ShortcutAction) => {
    setRebindingAction(action);
    setShortcutConflict('');
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    const action = rebindingAction();
    if (!action) return;

    e.preventDefault();
    e.stopPropagation();

    if (e.key === 'Escape') {
      setRebindingAction(null);
      return;
    }

    const binding = bindingFromEvent(e);
    if (!binding) return;

    const conflict = findConflict(action, binding, shortcuts());
    if (conflict) {
      setShortcutConflict(`Conflicts with "${conflict}"`);
      return;
    }

    const updated = shortcuts().map((shortcut) =>
      shortcut.action === action ? { ...shortcut, binding } : shortcut,
    );
    setShortcuts(updated);
    saveShortcuts(updated);
    setRebindingAction(null);
    setShortcutConflict('');
  };

  createEffect(() => {
    if (rebindingAction()) {
      document.addEventListener('keydown', handleKeyDown, true);
      onCleanup(() => document.removeEventListener('keydown', handleKeyDown, true));
    }
  });

  const resetAllShortcuts = () => {
    resetShortcuts();
    setShortcuts(getShortcuts());
  };

  const saveSettings = async () => {
    setSaving(true);
    try {
      await invoke('save_config', { updates: settings });
      await backendRef?.saveCredentials();
      window.toast?.success('Settings saved');
      app.setShowSettings(false);
    } catch (e) {
      console.error('Failed to save settings:', e);
      window.toast?.error('Failed to save settings');
    } finally {
      setSaving(false);
    }
  };

  return (
    <Show when={app.showSettings()}>
      <div
        class="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
        onClick={(e) => {
          if (e.target === e.currentTarget) app.setShowSettings(false);
        }}
        onKeyDown={(e) => {
          if (e.key === 'Escape' && !rebindingAction()) app.setShowSettings(false);
        }}
      >
        <div class="bg-pasture-800 border border-pasture-600 rounded-none shadow-xl w-[90vw] max-w-5xl h-[85vh] flex">
          <div class="w-56 border-r border-pasture-600 p-3 flex flex-col">
            <div class="mb-2">
              <div class="text-xs font-semibold text-wool-500 uppercase tracking-wider px-3 py-1.5">
                App
              </div>
              <nav class="space-y-1">
                <button
                  onClick={() => setActiveTab('appearance')}
                  class="w-full px-3 py-2 text-left text-sm rounded-none flex items-center gap-2"
                  classList={{
                    'bg-pasture-700 text-wool-100': activeTab() === 'appearance',
                    'text-wool-400 hover:bg-pasture-700/50': activeTab() !== 'appearance',
                  }}
                >
                  <Icon name="palette" class="w-4 h-4" />
                  Theme
                </button>
                <button
                  onClick={() => setActiveTab('shortcuts')}
                  class="w-full px-3 py-2 text-left text-sm rounded-none flex items-center gap-2"
                  classList={{
                    'bg-pasture-700 text-wool-100': activeTab() === 'shortcuts',
                    'text-wool-400 hover:bg-pasture-700/50': activeTab() !== 'shortcuts',
                  }}
                >
                  <Icon name="keyboard" class="w-4 h-4" />
                  Shortcuts
                </button>
              </nav>
            </div>

            <div class="border-t border-pasture-600 my-2" />

            <div class="flex-1 overflow-y-auto">
              <div class="text-xs font-semibold text-wool-500 uppercase tracking-wider px-3 py-1.5">
                Backend
              </div>
              <nav class="space-y-1">
                <button
                  onClick={() => {
                    setActiveTab('backend');
                    setBackendSection('connection');
                  }}
                  class="w-full px-3 py-2 text-left text-sm rounded-none flex items-center gap-2"
                  classList={{
                    'bg-pasture-700 text-wool-100': activeTab() === 'backend',
                    'text-wool-400 hover:bg-pasture-700/50': activeTab() !== 'backend',
                  }}
                >
                  <Icon name="server" class="w-4 h-4 text-sage" />
                  <span class="flex-1">Connection</span>
                  <Show when={backendHealth()}>
                    {(health) => (
                      <span
                        class="w-2 h-2 rounded-none shrink-0"
                        classList={{
                          'bg-wool-500 animate-pulse': health().status === 'checking',
                          'bg-green-400': health().status === 'online' && !health().error,
                          'bg-amber-400': health().status === 'online' && !!health().error,
                          'bg-terra': health().status === 'offline',
                        }}
                      />
                    )}
                  </Show>
                </button>
              </nav>
            </div>

            <div class="border-t border-pasture-600 my-2" />

            <button
              onClick={() => setActiveTab('about')}
              class="w-full px-3 py-2 text-left text-sm rounded-none flex items-center gap-2"
              classList={{
                'bg-pasture-700 text-wool-100': activeTab() === 'about',
                'text-wool-400 hover:bg-pasture-700/50': activeTab() !== 'about',
              }}
            >
              <Icon name="info" class="w-4 h-4" />
              About
            </button>
          </div>

          <div class="flex-1 flex flex-col overflow-hidden">
            <div class="p-4 border-b border-pasture-600 flex items-center justify-between shrink-0">
              <h2 class="text-lg font-medium text-wool-100">Settings</h2>
              <button
                onClick={() => app.setShowSettings(false)}
                class="p-1 rounded-none hover:bg-pasture-700 text-wool-500"
              >
                <Icon name="x" class="w-5 h-5" />
              </button>
            </div>

            <div class="flex-1 overflow-y-auto p-6">
              <Show when={loading()}>
                <div class="flex items-center justify-center py-12 text-wool-500">
                  Loading settings...
                </div>
              </Show>

              <Show when={!loading() && activeTab() === 'appearance'}>
                <AppearanceTab
                  currentTheme={app.currentTheme}
                  setTheme={app.setTheme}
                  themeSelectorOpen={themeSelectorOpen}
                  setThemeSelectorOpen={setThemeSelectorOpen}
                />
              </Show>

              <Show when={!loading() && activeTab() === 'shortcuts'}>
                <ShortcutsTab
                  shortcuts={shortcuts}
                  rebindingAction={rebindingAction}
                  shortcutConflict={shortcutConflict}
                  startRebind={startRebind}
                  resetAllShortcuts={resetAllShortcuts}
                />
              </Show>

              <Show when={!loading() && activeTab() === 'backend'}>
                <BackendTab
                  settings={settings}
                  setSettings={setSettings}
                  section={backendSection}
                  setSection={setBackendSection}
                  backendHealth={backendHealth}
                  checkBackendHealth={() => checkBackendHealth()}
                  ref={(value) => {
                    backendRef = value;
                  }}
                />
              </Show>

              <Show when={!loading() && activeTab() === 'about'}>
                <AboutTab versionInfo={app.versionInfo} />
              </Show>
            </div>

            <div class="px-6 py-4 border-t border-pasture-600 flex justify-end shrink-0">
              <div class="flex gap-3">
                <button onClick={() => app.setShowSettings(false)} class="btn btn-ghost">
                  Cancel
                </button>
                <button onClick={saveSettings} disabled={saving()} class="btn">
                  {saving() ? 'Saving...' : 'Save'}
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>
    </Show>
  );
};
