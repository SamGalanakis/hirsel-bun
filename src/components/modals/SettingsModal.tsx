/**
 * Settings modal component - Full implementation with Basecoat UI components
 */
import { invoke } from '@tauri-apps/api/core';
import {
  type Component,
  type JSX,
  For,
  Show,
  createEffect,
  createSignal,
  onCleanup,
} from 'solid-js';
import { createStore } from 'solid-js/store';
import { useApp } from '../../stores';
import { type ThemeId, THEMES, THEME_LIST } from '../../lib/theme';
import {
  type ShortcutConfig,
  type ShortcutAction,
  getShortcuts,
  saveShortcuts,
  resetShortcuts,
  formatBinding,
  bindingFromEvent,
  findConflict,
} from '../../lib/shortcuts';
import { initLucideIcons } from '../../lib/icons';

type SettingsTab = 'appearance' | 'shortcuts' | 'profiles' | 'about';
type ProfileTab = 'connection' | 'agents' | 'runners' | 'defaults' | 'git' | 'data' | 'services';

// Dropdown option type
interface DropdownOption {
  value: string;
  label: string;
}

// Basecoat-style Dropdown component
const Dropdown: Component<{
  value: string;
  options: DropdownOption[];
  onChange: (value: string) => void;
  placeholder?: string;
  class?: string;
}> = (props) => {
  const [open, setOpen] = createSignal(false);
  let containerRef: HTMLDivElement | undefined;

  const selectedLabel = () => {
    const option = props.options.find((o) => o.value === props.value);
    return option?.label || props.placeholder || 'Select...';
  };

  // Close on click outside
  createEffect(() => {
    if (open()) {
      const handler = (e: MouseEvent) => {
        if (containerRef && !containerRef.contains(e.target as Node)) {
          setOpen(false);
        }
      };
      document.addEventListener('click', handler);
      onCleanup(() => document.removeEventListener('click', handler));
    }
  });

  // Reinit icons when dropdown opens
  createEffect(() => {
    if (open()) {
      queueMicrotask(() => initLucideIcons());
    }
  });

  return (
    <div ref={containerRef} class={`dropdown relative ${props.class || ''}`}>
      <button
        type="button"
        class="btn-outline w-full justify-between"
        onClick={() => setOpen(!open())}
        aria-haspopup="listbox"
        aria-expanded={open()}
      >
        <span class="truncate flex-1 text-left" classList={{ 'text-muted-foreground': !props.value }}>
          {selectedLabel()}
        </span>
        <i data-lucide="chevrons-up-down" class="w-4 h-4 opacity-50 shrink-0" />
      </button>
      <Show when={open()}>
        <div
          data-popover
          class="absolute z-50 mt-1 w-full bg-popover border border-border rounded-md shadow-md py-1 max-h-60 overflow-auto"
        >
          <div role="listbox" aria-orientation="vertical">
            <For each={props.options}>
              {(option) => (
                <div
                  role="option"
                  aria-selected={props.value === option.value}
                  class="px-3 py-2 text-sm cursor-pointer hover:bg-accent flex items-center justify-between"
                  classList={{ 'bg-accent/50': props.value === option.value }}
                  onClick={() => {
                    props.onChange(option.value);
                    setOpen(false);
                  }}
                >
                  <span>{option.label}</span>
                  <Show when={props.value === option.value}>
                    <i data-lucide="check" class="w-4 h-4 text-primary" />
                  </Show>
                </div>
              )}
            </For>
          </div>
        </div>
      </Show>
    </div>
  );
};

// Basecoat-style Switch component
const Switch: Component<{
  id?: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  description?: string;
}> = (props) => {
  return (
    <div class="flex items-start justify-between rounded-lg border p-4">
      <div class="flex flex-col gap-0.5">
        <label for={props.id} class="font-medium leading-normal">{props.label}</label>
        <Show when={props.description}>
          <p class="text-muted-foreground text-sm">{props.description}</p>
        </Show>
      </div>
      <input
        id={props.id}
        type="checkbox"
        role="switch"
        checked={props.checked}
        onChange={(e) => props.onChange(e.currentTarget.checked)}
      />
    </div>
  );
};

// Runner types
interface RunnerHost {
  type: 'local' | 'ssh' | 'fly' | 'client';
  address?: string;
  user?: string;
  port?: number;
  app?: string;
  region?: string;
  cpus?: number;
  memory_mb?: number;
}

interface RunnerContainer {
  image: string;
  env?: Record<string, string>;
}

interface Runner {
  host?: RunnerHost;
  container?: RunnerContainer;
}

// Profile types
interface ProfileAccess {
  type: 'direct' | 'tailscale';
  oauth_client_id?: string;
  oauth_client_secret?: string;
  tag?: string;
}

interface RemoteProfile {
  url: string;
  apiKey?: string;
  access?: ProfileAccess;
}

// Service worker config
interface ServiceWorkerConfig {
  runner?: string;
  idleTimeoutSeconds?: number;
}

interface ServiceWorkers {
  runner?: string;
  scribe?: ServiceWorkerConfig;
  gyp?: ServiceWorkerConfig;
}

// Storage config
interface StorageConfig {
  provider: 's3' | 'tigris' | 'minio';
  endpoint?: string;
  bucket: string;
  region?: string;
  accessKeyId?: string;
  secretAccessKey?: string;
}

interface Settings {
  defaultProfile: string;
  evalTimeout: number;
  humanInTheLoop: boolean;
  userMessagePause: 'sender' | 'all' | 'none';
  autoLearn: boolean;
  compactionEnabled: boolean;
  compactionThreshold: number;
  compactionKeepMessages: number;
  contextWarningThreshold: number;
  coordinatorPort: number;
  profiles: Record<string, RemoteProfile>;
  runners: Record<string, Runner>;
  defaultRunner?: string;
  serviceWorkers?: ServiceWorkers;
  storage?: {
    defaultStorage?: string;
    configs: Record<string, StorageConfig>;
  };
  git?: {
    configuredProviders?: string[];
    defaultProvider?: string;
  };
}

interface ProfileHealth {
  status: 'checking' | 'online' | 'offline';
  error?: string;
}

interface RunnerHealth {
  status: 'checking' | 'online' | 'offline';
  error?: string;
}

const defaultSettings = (): Settings => ({
  defaultProfile: 'local',
  evalTimeout: 300,
  humanInTheLoop: true,
  userMessagePause: 'sender',
  autoLearn: false,
  compactionEnabled: false,
  compactionThreshold: 10000,
  compactionKeepMessages: 20,
  contextWarningThreshold: 0.5,
  coordinatorPort: 19700,
  profiles: {},
  runners: {},
});

export const SettingsModal: Component = () => {
  const app = useApp();
  const [activeTab, setActiveTab] = createSignal<SettingsTab>('appearance');
  const [profileTab, setProfileTab] = createSignal<ProfileTab>('defaults');
  const [themeSelectorOpen, setThemeSelectorOpen] = createSignal(false);

  // Settings state
  const [settings, setSettings] = createStore<Settings>(defaultSettings());
  const [loading, setLoading] = createSignal(true);
  const [saving, setSaving] = createSignal(false);

  // Profile state
  const [selectedProfile, setSelectedProfile] = createSignal<string>('local');
  const [profileHealth, setProfileHealth] = createStore<Record<string, ProfileHealth>>({});
  const [runnerHealth, setRunnerHealth] = createStore<Record<string, RunnerHealth>>({});
  const [addingProfile, setAddingProfile] = createSignal(false);
  const [newProfileName, setNewProfileName] = createSignal('');
  const [newProfileUrl, setNewProfileUrl] = createSignal('');

  // Runner state
  const [addingRunner, setAddingRunner] = createSignal(false);
  const [editingRunner, setEditingRunner] = createSignal<string | null>(null);
  const [runnerForm, setRunnerForm] = createStore({
    name: '',
    hostType: 'ssh' as 'ssh' | 'fly' | 'local' | 'client',
    address: '',
    user: '',
    port: 22,
    flyApp: '',
    flyRegion: 'ams',
    flyCpus: 2,
    flyMemory: 2048,
    useDocker: false,
    dockerImage: 'debian:bookworm-slim',
  });

  // Storage state
  const [addingStorage, setAddingStorage] = createSignal(false);
  const [editingStorage, setEditingStorage] = createSignal<string | null>(null);
  const [storageForm, setStorageForm] = createStore({
    name: '',
    provider: 's3' as 's3' | 'tigris' | 'minio',
    endpoint: '',
    bucket: '',
    region: 'us-east-1',
    accessKeyId: '',
    secretAccessKey: '',
  });

  // Shortcuts state
  const [shortcuts, setShortcuts] = createSignal<ShortcutConfig[]>(getShortcuts());
  const [rebindingAction, setRebindingAction] = createSignal<ShortcutAction | null>(null);
  const [shortcutConflict, setShortcutConflict] = createSignal('');

  // Git state
  const [gitHubToken, setGitHubToken] = createSignal('');

  // Auth state
  const [selectedAuthProvider, setSelectedAuthProvider] = createSignal<'' | 'claude' | 'gemini' | 'codex' | 'goose'>('');
  const [selectedAuthMethod, setSelectedAuthMethod] = createSignal<'env' | 'apiKey' | 'oauth'>('env');
  const [authEnvVar, setAuthEnvVar] = createSignal('');
  const [authApiKey, setAuthApiKey] = createSignal('');
  const [testing, setTesting] = createSignal(false);

  // Load settings
  createEffect(() => {
    if (app.showSettings()) {
      loadSettings();
    }
  });

  // Reinit icons when tab changes
  createEffect(() => {
    void activeTab();
    void profileTab();
    queueMicrotask(() => initLucideIcons());
  });

  // Check profile health when settings load or profile changes
  createEffect(() => {
    const profiles = Object.keys(settings.profiles);
    for (const name of profiles) {
      checkProfileHealth(name);
    }
  });

  // Check runner health for SSH runners
  createEffect(() => {
    const runners = Object.entries(settings.runners);
    for (const [name, runner] of runners) {
      if (runner.host?.type === 'ssh') {
        checkRunnerHealth(name);
      }
    }
  });

  const loadSettings = async () => {
    setLoading(true);
    try {
      const config = await invoke<{ settings?: Partial<Settings> }>('get_config');
      if (config.settings) {
        setSettings({ ...defaultSettings(), ...config.settings });
      }
      setShortcuts(getShortcuts());
    } catch (e) {
      console.error('Failed to load settings:', e);
    } finally {
      setLoading(false);
    }
  };

  const checkProfileHealth = async (name: string) => {
    const profile = settings.profiles[name];
    if (!profile?.url) return;

    setProfileHealth(name, { status: 'checking' });
    try {
      await invoke('check_profile_health', { url: profile.url, apiKey: profile.apiKey });
      setProfileHealth(name, { status: 'online' });
    } catch (e) {
      const error = String(e);
      if (error.includes('auth') || error.includes('401') || error.includes('403')) {
        setProfileHealth(name, { status: 'online', error: 'auth error' });
      } else {
        setProfileHealth(name, { status: 'offline', error });
      }
    }
  };

  const checkRunnerHealth = async (name: string) => {
    setRunnerHealth(name, { status: 'checking' });
    try {
      await invoke('check_runner_health', { runnerName: name });
      setRunnerHealth(name, { status: 'online' });
    } catch (e) {
      setRunnerHealth(name, { status: 'offline', error: String(e) });
    }
  };

  const handleThemeSelect = (themeId: ThemeId) => {
    app.setTheme(themeId);
    setThemeSelectorOpen(false);
  };

  const currentThemeDisplay = () => {
    return THEMES[app.currentTheme()].name;
  };

  const currentSwatches = () => {
    return THEMES[app.currentTheme()].swatches;
  };

  const isLocalProfile = () => selectedProfile() === 'local';

  const remoteProfileNames = () => Object.keys(settings.profiles);
  const runnerNames = () => Object.keys(settings.runners);
  const storageNames = () => Object.keys(settings.storage?.configs || {});

  // Navigate to profile tab
  const navigateToProfile = (name: string) => {
    setSelectedProfile(name);
    setActiveTab('profiles');
    setProfileTab(name === 'local' ? 'defaults' : 'connection');
  };

  // Shortcuts
  const getShortcutsByCategory = (category: string) => {
    return shortcuts().filter((s) => s.category === category);
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

    const updated = shortcuts().map((s) =>
      s.action === action ? { ...s, binding } : s
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

  // Auth helpers
  const getDefaultEnvVar = (provider: string) => {
    switch (provider) {
      case 'claude': return 'ANTHROPIC_API_KEY';
      case 'gemini': return 'GOOGLE_API_KEY';
      case 'codex': return 'OPENAI_API_KEY';
      case 'goose': return 'GOOSE_API_KEY';
      default: return '';
    }
  };

  const testConnection = async () => {
    setTesting(true);
    try {
      await invoke('test_agent_connection', {
        provider: selectedAuthProvider(),
        method: selectedAuthMethod(),
        envVar: authEnvVar(),
        apiKey: authApiKey(),
      });
      window.toast?.success('Connection successful');
    } catch (e) {
      window.toast?.error(`Connection failed: ${e}`);
    } finally {
      setTesting(false);
    }
  };

  // Profile management
  const addProfile = () => {
    const name = newProfileName().trim();
    const url = newProfileUrl().trim();
    if (!name || !url) return;

    setSettings('profiles', name, { url, apiKey: '', access: { type: 'direct' } });
    setAddingProfile(false);
    setNewProfileName('');
    setNewProfileUrl('');
    navigateToProfile(name);
    checkProfileHealth(name);
  };

  const deleteProfile = async (name: string) => {
    const confirmed = await window.confirmDialog?.show({
      title: 'Delete Profile',
      message: `Delete profile "${name}"? This cannot be undone.`,
      confirmText: 'Delete',
      danger: true,
    });
    if (!confirmed) return;

    setSettings('profiles', name, undefined!);
    if (settings.defaultProfile === name) {
      setSettings('defaultProfile', 'local');
    }
    setSelectedProfile('local');
  };

  // Runner management
  const getRunner = (name: string) => settings.runners[name];

  const getHostTypeLabel = (type?: string) => {
    switch (type) {
      case 'ssh': return 'SSH';
      case 'fly': return 'Fly.io';
      case 'local': return 'Local';
      case 'client': return 'Client';
      default: return 'Local';
    }
  };

  const getLocalRunnerName = () => isLocalProfile() ? 'local' : 'orchestrator';
  const getLocalRunnerBadge = () => isLocalProfile() ? 'Built-in' : 'Remote Server';
  const getLocalRunnerDescription = () => isLocalProfile()
    ? 'Workers run on this machine'
    : 'Workers run on the remote orchestrator';

  const getCurrentDefaultRunner = () => {
    if (isLocalProfile()) return settings.defaultRunner;
    return settings.profiles[selectedProfile()]?.apiKey ? settings.defaultRunner : undefined;
  };

  const setCurrentDefaultRunner = (name: string | null) => {
    setSettings('defaultRunner', name || undefined);
  };

  const startAddRunner = () => {
    setRunnerForm({
      name: '',
      hostType: 'ssh',
      address: '',
      user: '',
      port: 22,
      flyApp: '',
      flyRegion: 'ams',
      flyCpus: 2,
      flyMemory: 2048,
      useDocker: false,
      dockerImage: 'debian:bookworm-slim',
    });
    setAddingRunner(true);
    setEditingRunner(null);
  };

  const startEditRunner = (name: string) => {
    const runner = settings.runners[name];
    if (!runner) return;

    setRunnerForm({
      name,
      hostType: runner.host?.type || 'ssh',
      address: runner.host?.address || '',
      user: runner.host?.user || '',
      port: runner.host?.port || 22,
      flyApp: runner.host?.app || '',
      flyRegion: runner.host?.region || 'ams',
      flyCpus: runner.host?.cpus || 2,
      flyMemory: runner.host?.memory_mb || 2048,
      useDocker: !!runner.container,
      dockerImage: runner.container?.image || 'debian:bookworm-slim',
    });
    setAddingRunner(true);
    setEditingRunner(name);
  };

  const saveRunner = () => {
    const name = runnerForm.name.trim();
    if (!name) return;

    const runner: Runner = {};

    if (runnerForm.hostType === 'ssh') {
      runner.host = {
        type: 'ssh',
        address: runnerForm.address,
        user: runnerForm.user || undefined,
        port: runnerForm.port !== 22 ? runnerForm.port : undefined,
      };
    } else if (runnerForm.hostType === 'fly') {
      runner.host = {
        type: 'fly',
        app: runnerForm.flyApp,
        region: runnerForm.flyRegion,
        cpus: runnerForm.flyCpus,
        memory_mb: runnerForm.flyMemory,
      };
    } else if (runnerForm.hostType === 'local') {
      runner.host = { type: 'local' };
    } else if (runnerForm.hostType === 'client') {
      runner.host = { type: 'client' };
    }

    if (runnerForm.useDocker && runnerForm.dockerImage) {
      runner.container = { image: runnerForm.dockerImage };
    }

    if (editingRunner() && editingRunner() !== name) {
      setSettings('runners', editingRunner()!, undefined!);
    }

    setSettings('runners', name, runner);
    setAddingRunner(false);
    setEditingRunner(null);

    if (runner.host?.type === 'ssh') {
      checkRunnerHealth(name);
    }
  };

  const deleteRunner = async (name: string) => {
    const confirmed = await window.confirmDialog?.show({
      title: 'Delete Runner',
      message: `Delete runner "${name}"?`,
      confirmText: 'Delete',
      danger: true,
    });
    if (!confirmed) return;

    setSettings('runners', name, undefined!);
    if (settings.defaultRunner === name) {
      setSettings('defaultRunner', undefined);
    }
  };

  // Storage management
  const getStorage = (name: string) => settings.storage?.configs[name];

  const getStorageProviderLabel = (provider: string) => {
    switch (provider) {
      case 's3': return 'Amazon S3';
      case 'tigris': return 'Tigris';
      case 'minio': return 'MinIO';
      default: return provider;
    }
  };

  const setDefaultStorage = (name: string) => {
    setSettings('storage', 'defaultStorage', name);
  };

  const startAddStorage = () => {
    setStorageForm({
      name: '',
      provider: 's3',
      endpoint: '',
      bucket: '',
      region: 'us-east-1',
      accessKeyId: '',
      secretAccessKey: '',
    });
    setAddingStorage(true);
    setEditingStorage(null);
  };

  const startEditStorage = (name: string) => {
    const storage = settings.storage?.configs[name];
    if (!storage) return;

    setStorageForm({
      name,
      provider: storage.provider,
      endpoint: storage.endpoint || '',
      bucket: storage.bucket,
      region: storage.region || 'us-east-1',
      accessKeyId: storage.accessKeyId || '',
      secretAccessKey: storage.secretAccessKey || '',
    });
    setAddingStorage(true);
    setEditingStorage(name);
  };

  const saveStorage = () => {
    const name = storageForm.name.trim();
    if (!name || !storageForm.bucket) return;

    const config: StorageConfig = {
      provider: storageForm.provider,
      bucket: storageForm.bucket,
      endpoint: storageForm.endpoint || undefined,
      region: storageForm.region || undefined,
      accessKeyId: storageForm.accessKeyId || undefined,
      secretAccessKey: storageForm.secretAccessKey || undefined,
    };

    if (editingStorage() && editingStorage() !== name) {
      const configs = { ...settings.storage?.configs };
      delete configs[editingStorage()!];
      setSettings('storage', 'configs', configs);
    }

    if (!settings.storage) {
      setSettings('storage', { configs: { [name]: config } });
    } else {
      setSettings('storage', 'configs', name, config);
    }

    if (!settings.storage?.defaultStorage) {
      setSettings('storage', 'defaultStorage', name);
    }

    setAddingStorage(false);
    setEditingStorage(null);
  };

  const deleteStorage = async (name: string) => {
    const confirmed = await window.confirmDialog?.show({
      title: 'Delete Storage',
      message: `Delete storage configuration "${name}"?`,
      confirmText: 'Delete',
      danger: true,
    });
    if (!confirmed) return;

    const configs = { ...settings.storage?.configs };
    delete configs[name];
    setSettings('storage', 'configs', configs);

    if (settings.storage?.defaultStorage === name) {
      const remaining = Object.keys(configs);
      setSettings('storage', 'defaultStorage', remaining[0] || undefined);
    }
  };

  // Save settings
  const saveSettings = async () => {
    setSaving(true);
    try {
      await invoke('save_config', { updates: settings });

      if (gitHubToken()) {
        await invoke('store_credential', {
          key: 'github_token',
          value: gitHubToken(),
        });
      }

      window.toast?.success('Settings saved');
      app.setShowSettings(false);
    } catch (e) {
      console.error('Failed to save settings:', e);
      window.toast?.error('Failed to save settings');
    } finally {
      setSaving(false);
    }
  };

  const deleteAllRuns = async () => {
    const confirmed = await window.confirmDialog?.show({
      title: 'Delete All Runs',
      message: 'This will permanently delete all runs. This cannot be undone.',
      confirmText: 'Delete All',
      danger: true,
    });
    if (!confirmed) return;

    try {
      await invoke('delete_all_runs');
      window.toast?.success('All runs deleted');
    } catch (e) {
      window.toast?.error('Failed to delete runs');
    }
  };

  // Dropdown options
  const providerOptions: DropdownOption[] = [
    { value: '', label: 'Select a provider...' },
    { value: 'claude', label: 'Claude (Anthropic)' },
    { value: 'gemini', label: 'Gemini (Google)' },
    { value: 'codex', label: 'Codex (OpenAI)' },
    { value: 'goose', label: 'Goose' },
  ];

  const authMethodOptions = (): DropdownOption[] => {
    const options: DropdownOption[] = [
      { value: 'env', label: 'Environment Variable' },
      { value: 'apiKey', label: 'API Key' },
    ];
    if (selectedAuthProvider() === 'claude') {
      options.push({ value: 'oauth', label: 'OAuth' });
    }
    return options;
  };

  const pauseOptions: DropdownOption[] = [
    { value: 'sender', label: 'Sender only' },
    { value: 'all', label: 'All workers' },
    { value: 'none', label: 'None' },
  ];

  const accessMethodOptions: DropdownOption[] = [
    { value: 'direct', label: 'Direct' },
    { value: 'tailscale', label: 'Tailscale' },
  ];

  const hostTypeOptions: DropdownOption[] = [
    { value: 'ssh', label: 'SSH Server' },
    { value: 'fly', label: 'Fly.io' },
    { value: 'local', label: 'Local' },
    { value: 'client', label: 'Client' },
  ];

  const storageProviderOptions: DropdownOption[] = [
    { value: 's3', label: 'Amazon S3' },
    { value: 'tigris', label: 'Tigris' },
    { value: 'minio', label: 'MinIO' },
  ];

  const runnerSelectOptions = (): DropdownOption[] => {
    return [
      { value: '', label: 'local (default)' },
      ...runnerNames().map((name) => ({ value: name, label: name })),
    ];
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
        <div class="bg-pasture-800 border border-pasture-600 rounded-lg shadow-xl w-[90vw] max-w-5xl h-[85vh] flex">
          {/* Sidebar */}
          <div class="w-56 border-r border-pasture-600 p-3 flex flex-col">
            {/* App Section */}
            <div class="mb-2">
              <div class="text-xs font-semibold text-wool-500 uppercase tracking-wider px-3 py-1.5">
                App
              </div>
              <nav class="space-y-1">
                <button
                  onClick={() => setActiveTab('appearance')}
                  class="w-full px-3 py-2 text-left text-sm rounded flex items-center gap-2"
                  classList={{
                    'bg-pasture-700 text-wool-100': activeTab() === 'appearance',
                    'text-wool-400 hover:bg-pasture-700/50': activeTab() !== 'appearance',
                  }}
                >
                  <i data-lucide="palette" class="w-4 h-4" />
                  Theme
                </button>
                <button
                  onClick={() => setActiveTab('shortcuts')}
                  class="w-full px-3 py-2 text-left text-sm rounded flex items-center gap-2"
                  classList={{
                    'bg-pasture-700 text-wool-100': activeTab() === 'shortcuts',
                    'text-wool-400 hover:bg-pasture-700/50': activeTab() !== 'shortcuts',
                  }}
                >
                  <i data-lucide="keyboard" class="w-4 h-4" />
                  Shortcuts
                </button>
              </nav>
            </div>

            <div class="border-t border-pasture-600 my-2" />

            {/* Profiles Section */}
            <div class="flex-1 overflow-y-auto">
              <div class="text-xs font-semibold text-wool-500 uppercase tracking-wider px-3 py-1.5">
                Profiles
              </div>
              <nav class="space-y-1">
                {/* Local profile */}
                <button
                  onClick={() => navigateToProfile('local')}
                  class="w-full px-3 py-2 text-left text-sm rounded flex items-center gap-2"
                  classList={{
                    'bg-pasture-700 text-wool-100': activeTab() === 'profiles' && selectedProfile() === 'local',
                    'text-wool-400 hover:bg-pasture-700/50': !(activeTab() === 'profiles' && selectedProfile() === 'local'),
                  }}
                >
                  <i data-lucide="laptop" class="w-4 h-4 text-sage" />
                  <span class="flex-1">local</span>
                  <Show when={settings.defaultProfile === 'local'}>
                    <span class="text-amber-400 text-xs">●</span>
                  </Show>
                </button>

                {/* Remote profiles */}
                <For each={remoteProfileNames()}>
                  {(name) => (
                    <button
                      onClick={() => navigateToProfile(name)}
                      class="w-full px-3 py-2 text-left text-sm rounded flex items-center gap-2"
                      classList={{
                        'bg-pasture-700 text-wool-100': activeTab() === 'profiles' && selectedProfile() === name,
                        'text-wool-400 hover:bg-pasture-700/50': !(activeTab() === 'profiles' && selectedProfile() === name),
                      }}
                    >
                      <i data-lucide="server" class="w-4 h-4 text-sage" />
                      <span class="flex-1 truncate">{name}</span>
                      <Show when={settings.defaultProfile === name}>
                        <span class="text-amber-400 text-xs">●</span>
                      </Show>
                      <Show when={profileHealth[name]}>
                        <span
                          class="w-2 h-2 rounded-full shrink-0"
                          classList={{
                            'bg-wool-500 animate-pulse': profileHealth[name]?.status === 'checking',
                            'bg-green-400': profileHealth[name]?.status === 'online' && !profileHealth[name]?.error,
                            'bg-amber-400': profileHealth[name]?.status === 'online' && !!profileHealth[name]?.error,
                            'bg-terra': profileHealth[name]?.status === 'offline',
                          }}
                        />
                      </Show>
                    </button>
                  )}
                </For>

                {/* Add profile button */}
                <button
                  onClick={() => setAddingProfile(true)}
                  class="w-full px-3 py-2 text-left text-sm rounded flex items-center gap-2 text-wool-500 hover:text-wool-300 hover:bg-pasture-700/50"
                >
                  <i data-lucide="plus" class="w-4 h-4" />
                  Add Profile
                </button>
              </nav>
            </div>

            <div class="border-t border-pasture-600 my-2" />

            {/* About */}
            <button
              onClick={() => setActiveTab('about')}
              class="w-full px-3 py-2 text-left text-sm rounded flex items-center gap-2"
              classList={{
                'bg-pasture-700 text-wool-100': activeTab() === 'about',
                'text-wool-400 hover:bg-pasture-700/50': activeTab() !== 'about',
              }}
            >
              <i data-lucide="info" class="w-4 h-4" />
              About
            </button>
          </div>

          {/* Content */}
          <div class="flex-1 flex flex-col overflow-hidden">
            <div class="p-4 border-b border-pasture-600 flex items-center justify-between shrink-0">
              <h2 class="text-lg font-medium text-wool-100">Settings</h2>
              <button
                onClick={() => app.setShowSettings(false)}
                class="p-1 rounded hover:bg-pasture-700 text-wool-500"
              >
                <i data-lucide="x" class="w-5 h-5" />
              </button>
            </div>

            <div class="flex-1 overflow-y-auto p-6">
              {/* Loading */}
              <Show when={loading()}>
                <div class="flex items-center justify-center py-12 text-wool-500">
                  Loading settings...
                </div>
              </Show>

              {/* Appearance Tab */}
              <Show when={!loading() && activeTab() === 'appearance'}>
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
                        onClick={() => setThemeSelectorOpen(!themeSelectorOpen())}
                        class="btn-outline w-full justify-between"
                        classList={{ 'ring-2 ring-primary': themeSelectorOpen() }}
                        aria-haspopup="listbox"
                        aria-expanded={themeSelectorOpen()}
                      >
                        <span class="flex items-center gap-3">
                          <span class="flex gap-1">
                            <For each={currentSwatches()}>
                              {(color) => (
                                <span
                                  class="w-3.5 h-3.5 rounded-full border border-white/20"
                                  style={{ background: color }}
                                />
                              )}
                            </For>
                          </span>
                          <span class="text-wool-200">{currentThemeDisplay()}</span>
                        </span>
                        <i data-lucide="chevrons-up-down" class="w-4 h-4 opacity-50" />
                      </button>

                      <Show when={themeSelectorOpen()}>
                        <div
                          data-popover
                          class="absolute z-50 mt-1 w-full bg-popover border border-border rounded-md shadow-xl max-h-80 overflow-y-auto"
                        >
                          <div role="listbox">
                            <For each={THEME_LIST}>
                              {(theme) => (
                                <div
                                  role="option"
                                  aria-selected={app.currentTheme() === theme.id}
                                  class="px-3 py-2.5 cursor-pointer hover:bg-accent flex items-center justify-between"
                                  classList={{ 'bg-accent/50': app.currentTheme() === theme.id }}
                                  onClick={() => handleThemeSelect(theme.id)}
                                >
                                  <span class="flex items-center gap-3">
                                    <span class="flex gap-1">
                                      <For each={theme.swatches}>
                                        {(color) => (
                                          <span
                                            class="w-3.5 h-3.5 rounded-full border border-white/20"
                                            style={{ background: color }}
                                          />
                                        )}
                                      </For>
                                    </span>
                                    <span class="text-wool-200">{theme.name}</span>
                                  </span>
                                  <Show when={app.currentTheme() === theme.id}>
                                    <i data-lucide="check" class="w-4 h-4 text-primary" />
                                  </Show>
                                </div>
                              )}
                            </For>
                          </div>
                        </div>
                      </Show>
                    </div>
                    <p class="text-xs text-wool-500 mt-2">
                      {THEMES[app.currentTheme()].description}
                    </p>
                  </div>
                </div>
              </Show>

              {/* Shortcuts Tab */}
              <Show when={!loading() && activeTab() === 'shortcuts'}>
                <div class="max-w-2xl mx-auto space-y-6">
                  <div class="flex items-center justify-between">
                    <div>
                      <h3 class="text-lg font-medium text-wool-100 mb-1">Keyboard Shortcuts</h3>
                      <p class="text-sm text-wool-500">Click any shortcut to rebind. Press Escape to cancel.</p>
                    </div>
                    <button onClick={resetAllShortcuts} class="btn btn-ghost btn-sm">
                      <i data-lucide="rotate-ccw" class="w-4 h-4 mr-1" />
                      Reset
                    </button>
                  </div>

                  <Show when={shortcutConflict()}>
                    <div class="bg-amber-500/10 border border-amber-500/30 rounded-lg p-3 text-sm text-amber-400 flex items-center gap-2">
                      <i data-lucide="alert-triangle" class="w-4 h-4" />
                      <span>{shortcutConflict()}</span>
                    </div>
                  </Show>

                  {/* Navigation */}
                  <div class="space-y-2">
                    <h4 class="text-sm font-medium text-wool-200">Navigation</h4>
                    <div class="space-y-1 text-sm">
                      <For each={getShortcutsByCategory('navigation')}>
                        {(shortcut) => (
                          <div class="flex justify-between items-center py-1.5 border-b border-pasture-700">
                            <span class="text-wool-300">{shortcut.label}</span>
                            <button
                              onClick={() => startRebind(shortcut.action)}
                              class="cursor-pointer transition-all rounded px-1"
                              classList={{
                                'ring-2 ring-amber-500': rebindingAction() === shortcut.action,
                                'hover:bg-pasture-600': rebindingAction() !== shortcut.action,
                              }}
                            >
                              <Show when={rebindingAction() !== shortcut.action}>
                                <kbd class="kbd text-wool-500">{formatBinding(shortcut.binding)}</kbd>
                              </Show>
                              <Show when={rebindingAction() === shortcut.action}>
                                <span class="kbd text-amber-400 animate-pulse">Press key...</span>
                              </Show>
                            </button>
                          </div>
                        )}
                      </For>
                    </div>
                  </div>

                  {/* Run Controls */}
                  <div class="space-y-2">
                    <h4 class="text-sm font-medium text-wool-200">Run Controls</h4>
                    <div class="space-y-1 text-sm">
                      <For each={getShortcutsByCategory('run-controls')}>
                        {(shortcut) => (
                          <div class="flex justify-between items-center py-1.5 border-b border-pasture-700">
                            <span class="text-wool-300">{shortcut.label}</span>
                            <button
                              onClick={() => startRebind(shortcut.action)}
                              class="cursor-pointer transition-all rounded px-1"
                              classList={{
                                'ring-2 ring-amber-500': rebindingAction() === shortcut.action,
                                'hover:bg-pasture-600': rebindingAction() !== shortcut.action,
                              }}
                            >
                              <Show when={rebindingAction() !== shortcut.action}>
                                <kbd class="kbd text-wool-500">{formatBinding(shortcut.binding)}</kbd>
                              </Show>
                              <Show when={rebindingAction() === shortcut.action}>
                                <span class="kbd text-amber-400 animate-pulse">Press key...</span>
                              </Show>
                            </button>
                          </div>
                        )}
                      </For>
                    </div>
                  </div>

                  {/* Other */}
                  <div class="space-y-2">
                    <h4 class="text-sm font-medium text-wool-200">Other</h4>
                    <div class="space-y-1 text-sm">
                      <For each={getShortcutsByCategory('other')}>
                        {(shortcut) => (
                          <div class="flex justify-between items-center py-1.5 border-b border-pasture-700 last:border-b-0">
                            <span class="text-wool-300">{shortcut.label}</span>
                            <button
                              onClick={() => startRebind(shortcut.action)}
                              class="cursor-pointer transition-all rounded px-1"
                              classList={{
                                'ring-2 ring-amber-500': rebindingAction() === shortcut.action,
                                'hover:bg-pasture-600': rebindingAction() !== shortcut.action,
                              }}
                            >
                              <Show when={rebindingAction() !== shortcut.action}>
                                <kbd class="kbd text-wool-500">{formatBinding(shortcut.binding)}</kbd>
                              </Show>
                              <Show when={rebindingAction() === shortcut.action}>
                                <span class="kbd text-amber-400 animate-pulse">Press key...</span>
                              </Show>
                            </button>
                          </div>
                        )}
                      </For>
                    </div>
                  </div>
                </div>
              </Show>

              {/* Profiles Tab */}
              <Show when={!loading() && activeTab() === 'profiles'}>
                <div class="flex flex-col h-full">
                  {/* Profile Header */}
                  <div class="flex items-center justify-between mb-4">
                    <div class="flex items-center gap-3">
                      <Show when={isLocalProfile()}>
                        <i data-lucide="laptop" class="w-5 h-5 text-sage" />
                      </Show>
                      <Show when={!isLocalProfile()}>
                        <i data-lucide="server" class="w-5 h-5 text-sage" />
                      </Show>
                      <h3 class="text-lg font-medium text-wool-100">{selectedProfile()}</h3>
                      <span class="badge-secondary">{isLocalProfile() ? 'Built-in' : 'Remote'}</span>
                      <Show when={profileHealth[selectedProfile()]}>
                        <span
                          class="flex items-center gap-1.5 text-xs"
                          classList={{
                            'text-wool-500': profileHealth[selectedProfile()]?.status === 'checking',
                            'text-green-400': profileHealth[selectedProfile()]?.status === 'online' && !profileHealth[selectedProfile()]?.error,
                            'text-amber-400': profileHealth[selectedProfile()]?.status === 'online' && !!profileHealth[selectedProfile()]?.error,
                            'text-terra': profileHealth[selectedProfile()]?.status === 'offline',
                          }}
                        >
                          <span
                            class="w-2 h-2 rounded-full"
                            classList={{
                              'bg-wool-500 animate-pulse': profileHealth[selectedProfile()]?.status === 'checking',
                              'bg-green-400': profileHealth[selectedProfile()]?.status === 'online' && !profileHealth[selectedProfile()]?.error,
                              'bg-amber-400': profileHealth[selectedProfile()]?.status === 'online' && !!profileHealth[selectedProfile()]?.error,
                              'bg-terra': profileHealth[selectedProfile()]?.status === 'offline',
                            }}
                          />
                          {profileHealth[selectedProfile()]?.status === 'online' && profileHealth[selectedProfile()]?.error
                            ? 'auth error'
                            : profileHealth[selectedProfile()]?.status}
                        </span>
                      </Show>
                    </div>
                    <div class="flex items-center gap-2">
                      <Show when={settings.defaultProfile !== selectedProfile()}>
                        <button
                          onClick={() => setSettings('defaultProfile', selectedProfile())}
                          class="btn btn-ghost btn-sm"
                        >
                          Set as Default
                        </button>
                      </Show>
                      <Show when={settings.defaultProfile === selectedProfile()}>
                        <span class="text-xs px-2 py-1 bg-amber-500/20 text-amber-400 rounded">Default Profile</span>
                      </Show>
                      <Show when={!isLocalProfile()}>
                        <button
                          onClick={() => deleteProfile(selectedProfile())}
                          class="btn btn-ghost btn-sm text-terra hover:text-terra"
                        >
                          <i data-lucide="trash-2" class="w-4 h-4" />
                        </button>
                      </Show>
                    </div>
                  </div>

                  {/* Profile Tabs */}
                  <div class="flex gap-1 mb-6 border-b border-pasture-600 pb-2">
                    <Show when={!isLocalProfile()}>
                      <button
                        onClick={() => setProfileTab('connection')}
                        class="px-3 py-1.5 rounded text-sm transition-colors"
                        classList={{
                          'bg-pasture-700 text-wool-100': profileTab() === 'connection',
                          'text-wool-400 hover:bg-pasture-700/50': profileTab() !== 'connection',
                        }}
                      >
                        Connection
                      </button>
                    </Show>
                    <button
                      onClick={() => setProfileTab('agents')}
                      class="px-3 py-1.5 rounded text-sm transition-colors"
                      classList={{
                        'bg-pasture-700 text-wool-100': profileTab() === 'agents',
                        'text-wool-400 hover:bg-pasture-700/50': profileTab() !== 'agents',
                      }}
                    >
                      Agents
                    </button>
                    <button
                      onClick={() => setProfileTab('runners')}
                      class="px-3 py-1.5 rounded text-sm transition-colors"
                      classList={{
                        'bg-pasture-700 text-wool-100': profileTab() === 'runners',
                        'text-wool-400 hover:bg-pasture-700/50': profileTab() !== 'runners',
                      }}
                    >
                      Runners
                    </button>
                    <button
                      onClick={() => setProfileTab('defaults')}
                      class="px-3 py-1.5 rounded text-sm transition-colors"
                      classList={{
                        'bg-pasture-700 text-wool-100': profileTab() === 'defaults',
                        'text-wool-400 hover:bg-pasture-700/50': profileTab() !== 'defaults',
                      }}
                    >
                      Defaults
                    </button>
                    <button
                      onClick={() => setProfileTab('git')}
                      class="px-3 py-1.5 rounded text-sm transition-colors"
                      classList={{
                        'bg-pasture-700 text-wool-100': profileTab() === 'git',
                        'text-wool-400 hover:bg-pasture-700/50': profileTab() !== 'git',
                      }}
                    >
                      Git
                    </button>
                    <button
                      onClick={() => setProfileTab('data')}
                      class="px-3 py-1.5 rounded text-sm transition-colors"
                      classList={{
                        'bg-pasture-700 text-wool-100': profileTab() === 'data',
                        'text-wool-400 hover:bg-pasture-700/50': profileTab() !== 'data',
                      }}
                    >
                      Data
                    </button>
                    <button
                      onClick={() => setProfileTab('services')}
                      class="px-3 py-1.5 rounded text-sm transition-colors"
                      classList={{
                        'bg-pasture-700 text-wool-100': profileTab() === 'services',
                        'text-wool-400 hover:bg-pasture-700/50': profileTab() !== 'services',
                      }}
                    >
                      Services
                    </button>
                  </div>

                  <div class="max-w-2xl overflow-y-auto flex-1">
                    {/* Connection Tab (remote only) */}
                    <Show when={profileTab() === 'connection' && !isLocalProfile()}>
                      <div class="space-y-6">
                        <p class="text-sm text-wool-500">Configure the connection to this remote orchestrator.</p>

                        <div class="space-y-2">
                          <label class="block text-sm font-medium text-wool-300">Server URL</label>
                          <input
                            type="text"
                            class="input w-full"
                            value={settings.profiles[selectedProfile()]?.url || ''}
                            onInput={(e) => setSettings('profiles', selectedProfile(), 'url', e.currentTarget.value)}
                            placeholder="http://100.x.x.x:8080"
                          />
                          <p class="text-xs text-wool-500">Tailscale IP recommended for secure access.</p>
                        </div>

                        <div class="space-y-2">
                          <label class="block text-sm font-medium text-wool-300">Hirsel API Key</label>
                          <input
                            type="password"
                            class="input w-full"
                            value={settings.profiles[selectedProfile()]?.apiKey || ''}
                            onInput={(e) => setSettings('profiles', selectedProfile(), 'apiKey', e.currentTarget.value)}
                            placeholder="••••••••"
                          />
                          <p class="text-xs text-wool-500">Authentication key for the remote Hirsel server (set via HIRSEL_API_KEY on server).</p>
                        </div>

                        <div class="space-y-2">
                          <label class="block text-sm font-medium text-wool-300">Worker Access Method</label>
                          <Dropdown
                            value={settings.profiles[selectedProfile()]?.access?.type || 'direct'}
                            options={accessMethodOptions}
                            onChange={(value) => setSettings('profiles', selectedProfile(), 'access', 'type', value as 'direct' | 'tailscale')}
                          />
                          <p class="text-xs text-wool-500">How workers connect to the orchestrator.</p>
                        </div>

                        <Show when={settings.profiles[selectedProfile()]?.access?.type === 'tailscale'}>
                          <div class="flex flex-col gap-4 pl-4 border-l-2 border-sage/30">
                            <p class="text-sm text-wool-400">
                              <a href="https://login.tailscale.com/admin/settings/oauth" target="_blank" class="text-amber-500 hover:underline">
                                Create OAuth client
                              </a>{' '}
                              for ephemeral auth keys.
                            </p>
                            <div class="space-y-2">
                              <label class="block text-sm text-wool-300">OAuth Client ID</label>
                              <input
                                type="text"
                                class="input w-full"
                                value={settings.profiles[selectedProfile()]?.access?.oauth_client_id || ''}
                                onInput={(e) => setSettings('profiles', selectedProfile(), 'access', 'oauth_client_id', e.currentTarget.value)}
                                placeholder="k..."
                              />
                            </div>
                            <div class="space-y-2">
                              <label class="block text-sm text-wool-300">OAuth Client Secret</label>
                              <input
                                type="password"
                                class="input w-full"
                                value={settings.profiles[selectedProfile()]?.access?.oauth_client_secret || ''}
                                onInput={(e) => setSettings('profiles', selectedProfile(), 'access', 'oauth_client_secret', e.currentTarget.value)}
                                placeholder="tskey-client-..."
                              />
                            </div>
                            <div class="space-y-2">
                              <label class="block text-sm text-wool-300">Device Tag (optional)</label>
                              <input
                                type="text"
                                class="input w-full"
                                value={settings.profiles[selectedProfile()]?.access?.tag || ''}
                                onInput={(e) => setSettings('profiles', selectedProfile(), 'access', 'tag', e.currentTarget.value)}
                                placeholder="tag:hirsel-worker"
                              />
                            </div>
                          </div>
                        </Show>
                      </div>
                    </Show>

                    {/* Agents Tab */}
                    <Show when={profileTab() === 'agents'}>
                      <div class="space-y-6">
                        <div class="space-y-2">
                          <label class="block text-sm font-medium text-wool-300">Provider</label>
                          <Dropdown
                            value={selectedAuthProvider()}
                            options={providerOptions}
                            onChange={(value) => setSelectedAuthProvider(value as typeof selectedAuthProvider extends () => infer T ? T : never)}
                            placeholder="Select a provider..."
                          />
                          <p class="text-xs text-wool-500">Select your AI provider to configure authentication.</p>
                        </div>

                        <Show when={selectedAuthProvider()}>
                          <div class="space-y-4">
                            <div class="space-y-2">
                              <label class="block text-sm font-medium text-wool-300">Authentication Method</label>
                              <Dropdown
                                value={selectedAuthMethod()}
                                options={authMethodOptions()}
                                onChange={(value) => setSelectedAuthMethod(value as 'env' | 'apiKey' | 'oauth')}
                              />
                            </div>

                            <div class="p-4 bg-pasture-700/30 rounded-lg space-y-4">
                              <Show when={selectedAuthMethod() === 'env'}>
                                <div class="space-y-2">
                                  <label class="block text-sm text-wool-300">Environment Variable</label>
                                  <input
                                    type="text"
                                    class="input w-full"
                                    value={authEnvVar()}
                                    onInput={(e) => setAuthEnvVar(e.currentTarget.value)}
                                    placeholder={getDefaultEnvVar(selectedAuthProvider())}
                                  />
                                  <p class="text-xs text-wool-500">Leave empty to use default.</p>
                                </div>
                              </Show>

                              <Show when={selectedAuthMethod() === 'apiKey'}>
                                <div class="space-y-2">
                                  <label class="block text-sm text-wool-300">API Key</label>
                                  <input
                                    type="password"
                                    class="input w-full"
                                    value={authApiKey()}
                                    onInput={(e) => setAuthApiKey(e.currentTarget.value)}
                                    placeholder="••••••••"
                                  />
                                </div>
                              </Show>

                              <Show when={selectedAuthMethod() === 'oauth'}>
                                <div class="text-sm text-wool-500">
                                  <p>Uses OAuth credentials from <code class="text-xs bg-pasture-600 px-1 py-0.5 rounded">~/.claude/.credentials.json</code></p>
                                  <p class="mt-2">Run <code class="bg-pasture-600 px-1 py-0.5 rounded">claude login</code> to authenticate.</p>
                                </div>
                              </Show>
                            </div>

                            <button
                              type="button"
                              onClick={testConnection}
                              disabled={testing()}
                              class="btn"
                            >
                              {testing() ? 'Testing...' : 'Test Connection'}
                            </button>
                          </div>
                        </Show>
                      </div>
                    </Show>

                    {/* Runners Tab */}
                    <Show when={profileTab() === 'runners'}>
                      <div class="space-y-6">
                        <p class="text-sm text-wool-500">
                          Configure where workers run: local machine, SSH servers, or Fly.io cloud VMs.
                        </p>

                        <div class="space-y-3">
                          <div class="flex items-center justify-between">
                            <h4 class="text-sm font-medium text-wool-200">Configured Runners</h4>
                            <button type="button" onClick={startAddRunner} class="btn btn-ghost btn-sm">
                              <i data-lucide="plus" class="w-4 h-4 mr-1" />
                              Add Runner
                            </button>
                          </div>

                          {/* Local/Orchestrator runner (always shown) */}
                          <div class="p-3 bg-pasture-700/30 rounded-lg border border-pasture-600/50">
                            <div class="flex items-center justify-between">
                              <div class="flex items-center gap-2">
                                <Show when={isLocalProfile()}>
                                  <i data-lucide="laptop" class="w-4 h-4 text-sage" />
                                </Show>
                                <Show when={!isLocalProfile()}>
                                  <i data-lucide="server" class="w-4 h-4 text-sage" />
                                </Show>
                                <span class="font-medium text-wool-200">{getLocalRunnerName()}</span>
                                <span class="badge-secondary">{getLocalRunnerBadge()}</span>
                                <Show when={!getCurrentDefaultRunner()}>
                                  <span class="badge">default</span>
                                </Show>
                              </div>
                              <Show when={getCurrentDefaultRunner()}>
                                <button
                                  type="button"
                                  onClick={() => setCurrentDefaultRunner(null)}
                                  class="btn btn-ghost btn-sm text-wool-400"
                                >
                                  Set Default
                                </button>
                              </Show>
                            </div>
                            <div class="mt-2 text-xs text-wool-500">
                              {getLocalRunnerDescription()}
                            </div>
                          </div>

                          {/* Remote runner cards */}
                          <For each={runnerNames()}>
                            {(name) => (
                              <div class="p-3 bg-pasture-700/30 rounded-lg border border-pasture-600/50">
                                <div class="flex items-center justify-between">
                                  <div class="flex items-center gap-2">
                                    <Show when={getRunner(name)?.container}>
                                      <i data-lucide="container" class="w-4 h-4 text-sage" />
                                    </Show>
                                    <Show when={getRunner(name)?.host?.type === 'ssh' && !getRunner(name)?.container}>
                                      <i data-lucide="server" class="w-4 h-4 text-sage" />
                                    </Show>
                                    <Show when={getRunner(name)?.host?.type === 'fly' && !getRunner(name)?.container}>
                                      <i data-lucide="cloud" class="w-4 h-4 text-sage" />
                                    </Show>
                                    <Show when={getRunner(name)?.host?.type === 'local' && !getRunner(name)?.container}>
                                      <i data-lucide="laptop" class="w-4 h-4 text-sage" />
                                    </Show>
                                    <Show when={getRunner(name)?.host?.type === 'client' && !getRunner(name)?.container}>
                                      <i data-lucide="monitor" class="w-4 h-4 text-sage" />
                                    </Show>
                                    <span class="font-medium text-wool-200">{name}</span>
                                    <span class="badge-secondary">{getHostTypeLabel(getRunner(name)?.host?.type)}</span>
                                    <Show when={getRunner(name)?.container}>
                                      <span class="badge-outline">Docker</span>
                                    </Show>
                                    <Show when={getCurrentDefaultRunner() === name}>
                                      <span class="badge">default</span>
                                    </Show>
                                    <Show when={getRunner(name)?.host?.type === 'ssh' && runnerHealth[name]}>
                                      <span
                                        class="w-2 h-2 rounded-full shrink-0"
                                        classList={{
                                          'bg-wool-500 animate-pulse': runnerHealth[name]?.status === 'checking',
                                          'bg-green-400': runnerHealth[name]?.status === 'online',
                                          'bg-terra': runnerHealth[name]?.status === 'offline',
                                        }}
                                      />
                                    </Show>
                                  </div>
                                  <div class="flex items-center gap-2">
                                    <Show when={getCurrentDefaultRunner() !== name}>
                                      <button
                                        type="button"
                                        onClick={() => setCurrentDefaultRunner(name)}
                                        class="btn btn-ghost btn-sm text-wool-400"
                                      >
                                        Set Default
                                      </button>
                                    </Show>
                                    <button
                                      type="button"
                                      onClick={() => deleteRunner(name)}
                                      class="btn btn-ghost btn-sm text-wool-400 hover:text-terra"
                                    >
                                      <i data-lucide="trash-2" class="w-4 h-4" />
                                    </button>
                                    <button
                                      type="button"
                                      onClick={() => startEditRunner(name)}
                                      class="btn btn-ghost btn-sm"
                                    >
                                      <i data-lucide="pencil" class="w-4 h-4 mr-1" />
                                      Edit
                                    </button>
                                  </div>
                                </div>
                                <div class="mt-2 text-xs text-wool-500 flex flex-wrap gap-x-4 gap-y-1">
                                  <Show when={getRunner(name)?.host?.type === 'ssh'}>
                                    <span class="flex items-center gap-1">
                                      <span class="text-wool-600">Host:</span>
                                      <code class="text-wool-400">{getRunner(name)?.host?.address}</code>
                                    </span>
                                  </Show>
                                  <Show when={getRunner(name)?.host?.type === 'fly'}>
                                    <span class="flex items-center gap-1">
                                      <span class="text-wool-600">App:</span>
                                      <code class="text-wool-400">{getRunner(name)?.host?.app}</code>
                                    </span>
                                    <span class="flex items-center gap-1">
                                      <span class="text-wool-600">Region:</span>
                                      <code class="text-wool-400">{getRunner(name)?.host?.region}</code>
                                    </span>
                                  </Show>
                                  <Show when={getRunner(name)?.container}>
                                    <span class="flex items-center gap-1">
                                      <span class="text-wool-600">Image:</span>
                                      <code class="text-wool-400">{getRunner(name)?.container?.image}</code>
                                    </span>
                                  </Show>
                                </div>
                              </div>
                            )}
                          </For>
                        </div>
                      </div>
                    </Show>

                    {/* Defaults Tab */}
                    <Show when={profileTab() === 'defaults'}>
                      <div class="space-y-6">
                        {/* Eval Timeout */}
                        <div class="space-y-2">
                          <label class="block text-sm text-wool-300">Eval Timeout (seconds)</label>
                          <input
                            type="number"
                            class="input w-full"
                            value={settings.evalTimeout}
                            onInput={(e) => setSettings('evalTimeout', Number.parseInt(e.currentTarget.value) || 300)}
                            min={60}
                            step={60}
                          />
                          <p class="text-xs text-wool-500">Time limit for eval agent.</p>
                        </div>

                        {/* Human in the Loop */}
                        <Switch
                          id="hitl-switch"
                          checked={settings.humanInTheLoop}
                          onChange={(checked) => setSettings('humanInTheLoop', checked)}
                          label="Human in the Loop"
                          description="Allow agents to send messages to the user and wait for response."
                        />

                        {/* Pause on User Message */}
                        <div class="space-y-2">
                          <label class="block text-sm text-wool-300">Pause on User Message</label>
                          <Dropdown
                            value={settings.userMessagePause}
                            options={pauseOptions}
                            onChange={(value) => setSettings('userMessagePause', value as 'sender' | 'all' | 'none')}
                          />
                          <p class="text-xs text-wool-500">Which workers pause when a user message is sent.</p>
                        </div>

                        <hr class="border-pasture-600" />

                        {/* Auto Learn */}
                        <Switch
                          id="autolearn-switch"
                          checked={settings.autoLearn}
                          onChange={(checked) => setSettings('autoLearn', checked)}
                          label="Auto Learn"
                          description="Enable the Scribe system to update docs based on worker discoveries."
                        />

                        {/* Auto-compact */}
                        <Switch
                          id="compaction-switch"
                          checked={settings.compactionEnabled}
                          onChange={(checked) => setSettings('compactionEnabled', checked)}
                          label="Auto-compact Messages"
                          description="Periodically summarize old messages to save context space."
                        />

                        <Show when={settings.compactionEnabled}>
                          <div class="grid grid-cols-2 gap-4 pl-4 border-l-2 border-pasture-600">
                            <div class="space-y-2">
                              <label class="block text-sm text-wool-300">Threshold (chars)</label>
                              <input
                                type="number"
                                class="input w-full"
                                value={settings.compactionThreshold}
                                onInput={(e) => setSettings('compactionThreshold', Number.parseInt(e.currentTarget.value) || 10000)}
                                min={1000}
                                max={100000}
                                step={1000}
                              />
                            </div>
                            <div class="space-y-2">
                              <label class="block text-sm text-wool-300">Keep messages</label>
                              <input
                                type="number"
                                class="input w-full"
                                value={settings.compactionKeepMessages}
                                onInput={(e) => setSettings('compactionKeepMessages', Number.parseInt(e.currentTarget.value) || 20)}
                                min={5}
                                max={200}
                                step={5}
                              />
                            </div>
                          </div>
                        </Show>
                      </div>
                    </Show>

                    {/* Git Tab */}
                    <Show when={profileTab() === 'git'}>
                      <div class="space-y-6">
                        <p class="text-sm text-wool-500">Configure git provider tokens for repository access.</p>

                        <div class="p-4 bg-pasture-700/30 rounded-lg border border-pasture-600/50">
                          <div class="flex items-center justify-between mb-4">
                            <div class="flex items-center gap-3">
                              <svg class="w-6 h-6" viewBox="0 0 24 24" fill="currentColor">
                                <path d="M12 0c-6.626 0-12 5.373-12 12 0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23.957-.266 1.983-.399 3.003-.404 1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576 4.765-1.589 8.199-6.086 8.199-11.386 0-6.627-5.373-12-12-12z" />
                              </svg>
                              <div>
                                <h4 class="font-medium text-wool-200">GitHub</h4>
                                <p class="text-xs text-wool-500">Personal Access Token for GitHub repositories</p>
                              </div>
                            </div>
                            <Show when={settings.git?.configuredProviders?.includes('github')}>
                              <span class="text-xs px-2 py-0.5 bg-green-500/20 text-green-400 rounded flex items-center gap-1">
                                <i data-lucide="check" class="w-3 h-3" />
                                Configured
                              </span>
                            </Show>
                            <Show when={!settings.git?.configuredProviders?.includes('github')}>
                              <span class="text-xs px-2 py-0.5 bg-amber-500/20 text-amber-400 rounded">
                                Not configured
                              </span>
                            </Show>
                          </div>

                          <div class="space-y-2">
                            <label class="block text-sm text-wool-300">Personal Access Token</label>
                            <input
                              type="password"
                              class="input w-full"
                              value={gitHubToken()}
                              onInput={(e) => setGitHubToken(e.currentTarget.value)}
                              placeholder="ghp_xxxxxxxxxxxx"
                            />
                            <p class="text-xs text-wool-500">
                              Create a token at{' '}
                              <a href="https://github.com/settings/tokens" target="_blank" class="text-amber-500 hover:underline">
                                github.com/settings/tokens
                              </a>{' '}
                              with <code class="text-xs bg-pasture-600 px-1 py-0.5 rounded">repo</code> scope.
                            </p>
                          </div>
                        </div>
                      </div>
                    </Show>

                    {/* Data Tab */}
                    <Show when={profileTab() === 'data'}>
                      <div class="space-y-6">
                        <Show when={isLocalProfile()}>
                          <div class="space-y-2">
                            <label class="block text-sm font-medium text-wool-300">Data Locations</label>
                            <div class="space-y-2 text-sm">
                              <div class="flex justify-between items-center">
                                <span class="text-wool-500">Config</span>
                                <code class="text-wool-400 text-xs bg-pasture-700 px-1.5 py-0.5 rounded">~/.hirsel/config.toml</code>
                              </div>
                              <div class="flex justify-between items-center">
                                <span class="text-wool-500">Runs</span>
                                <code class="text-wool-400 text-xs bg-pasture-700 px-1.5 py-0.5 rounded">~/.hirsel/runs/</code>
                              </div>
                              <div class="flex justify-between items-center">
                                <span class="text-wool-500">Credentials</span>
                                <code class="text-wool-400 text-xs bg-pasture-700 px-1.5 py-0.5 rounded">~/.hirsel/hirsel.db</code>
                              </div>
                              <div class="flex justify-between items-center">
                                <span class="text-wool-500">Logs</span>
                                <code class="text-wool-400 text-xs bg-pasture-700 px-1.5 py-0.5 rounded">~/.local/share/app.hirsel/logs/</code>
                              </div>
                            </div>
                          </div>

                          <hr class="border-pasture-600" />

                          {/* Storage Configuration */}
                          <div class="space-y-4">
                            <div class="flex items-center justify-between">
                              <div>
                                <label class="block text-sm font-medium text-wool-300">Cloud Storage</label>
                                <p class="text-xs text-wool-500">S3-compatible storage for snapshots and files.</p>
                              </div>
                              <button type="button" onClick={startAddStorage} class="btn btn-sm">
                                <i data-lucide="plus" class="w-4 h-4 mr-1" />
                                Add Storage
                              </button>
                            </div>

                            <Show when={storageNames().length > 0}>
                              <div class="space-y-2">
                                <For each={storageNames()}>
                                  {(name) => (
                                    <div class="flex items-center justify-between p-3 bg-pasture-700/30 rounded-lg">
                                      <div class="flex items-center gap-3">
                                        <i data-lucide="database" class="w-4 h-4 text-sage" />
                                        <div>
                                          <div class="flex items-center gap-2">
                                            <span class="font-medium text-wool-200">{name}</span>
                                            <Show when={settings.storage?.defaultStorage === name}>
                                              <span class="text-xs px-1.5 py-0.5 bg-sage/20 text-sage rounded">Default</span>
                                            </Show>
                                          </div>
                                          <p class="text-xs text-wool-500">
                                            {getStorageProviderLabel(getStorage(name)?.provider || 's3')} • {getStorage(name)?.bucket || 'No bucket'}
                                          </p>
                                        </div>
                                      </div>
                                      <div class="flex items-center gap-1">
                                        <Show when={settings.storage?.defaultStorage !== name}>
                                          <button
                                            type="button"
                                            onClick={() => setDefaultStorage(name)}
                                            class="btn btn-ghost btn-sm"
                                            title="Set as default"
                                          >
                                            <i data-lucide="star" class="w-4 h-4" />
                                          </button>
                                        </Show>
                                        <button
                                          type="button"
                                          onClick={() => startEditStorage(name)}
                                          class="btn btn-ghost btn-sm"
                                          title="Edit"
                                        >
                                          <i data-lucide="pencil" class="w-4 h-4" />
                                        </button>
                                        <button
                                          type="button"
                                          onClick={() => deleteStorage(name)}
                                          class="btn btn-ghost btn-sm text-wool-400 hover:text-terra"
                                          title="Delete"
                                        >
                                          <i data-lucide="trash-2" class="w-4 h-4" />
                                        </button>
                                      </div>
                                    </div>
                                  )}
                                </For>
                              </div>
                            </Show>
                          </div>

                          <hr class="border-pasture-600" />
                        </Show>

                        <Show when={!isLocalProfile()}>
                          <div class="space-y-2">
                            <label class="block text-sm font-medium text-wool-300">Remote Server</label>
                            <div class="space-y-2 text-sm">
                              <div class="flex justify-between items-center">
                                <span class="text-wool-500">Server URL</span>
                                <code class="text-wool-400 text-xs bg-pasture-700 px-1.5 py-0.5 rounded">
                                  {settings.profiles[selectedProfile()]?.url || 'Not configured'}
                                </code>
                              </div>
                            </div>
                          </div>

                          <hr class="border-pasture-600" />
                        </Show>

                        {/* Danger Zone */}
                        <div class="space-y-4">
                          <label class="block text-sm font-medium text-terra">Danger Zone</label>
                          <div class="flex items-start justify-between rounded-lg border border-terra/30 p-4">
                            <div>
                              <label class="font-medium text-wool-200">Delete All Runs</label>
                              <p class="text-sm text-wool-500 mt-0.5">
                                Permanently delete all runs{isLocalProfile() ? ' on this machine' : ' from this profile'}. This cannot be undone.
                              </p>
                            </div>
                            <button
                              type="button"
                              onClick={deleteAllRuns}
                              class="btn btn-destructive shrink-0"
                            >
                              Delete All
                            </button>
                          </div>
                        </div>
                      </div>
                    </Show>

                    {/* Services Tab */}
                    <Show when={profileTab() === 'services'}>
                      <div class="space-y-6">
                        <p class="text-sm text-wool-500">
                          Configure where service workers run. Service workers handle background tasks like documentation (Scribe) and chat assistance (Gyp).
                        </p>

                        {/* Scribe Service Worker */}
                        <div class="rounded-lg border border-pasture-600 p-4">
                          <div class="flex items-center gap-3 mb-4">
                            <span class="flex items-center justify-center w-8 h-8 rounded-lg bg-sage/20">
                              <i data-lucide="book-open" class="w-4 h-4 text-sage" />
                            </span>
                            <div>
                              <h3 class="font-medium text-wool-200">Scribe</h3>
                              <p class="text-xs text-wool-500">Documentation agent that updates docs/ based on scribe calls</p>
                            </div>
                          </div>

                          <div class="space-y-4">
                            <div class="space-y-2">
                              <label class="block text-sm text-wool-300">Runner</label>
                              <Dropdown
                                value={settings.serviceWorkers?.scribe?.runner || ''}
                                options={runnerSelectOptions()}
                                onChange={(value) => {
                                  if (!settings.serviceWorkers) {
                                    setSettings('serviceWorkers', { scribe: {}, gyp: {} });
                                  }
                                  setSettings('serviceWorkers', 'scribe', 'runner', value || undefined);
                                }}
                              />
                              <p class="text-xs text-wool-500">Where to run the Scribe service worker.</p>
                            </div>

                            <div class="space-y-2">
                              <label class="block text-sm text-wool-300">Idle Timeout (seconds)</label>
                              <input
                                type="number"
                                class="input w-full"
                                value={settings.serviceWorkers?.scribe?.idleTimeoutSeconds || 300}
                                onInput={(e) => {
                                  if (!settings.serviceWorkers) {
                                    setSettings('serviceWorkers', { scribe: {}, gyp: {} });
                                  }
                                  setSettings('serviceWorkers', 'scribe', 'idleTimeoutSeconds', Number.parseInt(e.currentTarget.value) || undefined);
                                }}
                                min={30}
                                step={30}
                                placeholder="300"
                              />
                              <p class="text-xs text-wool-500">Keep worker alive after last activity (default: 5 minutes).</p>
                            </div>
                          </div>
                        </div>

                        {/* Gyp Service Worker */}
                        <div class="rounded-lg border border-pasture-600 p-4">
                          <div class="flex items-center gap-3 mb-4">
                            <span class="flex items-center justify-center w-8 h-8 rounded-lg bg-amber-500/20">
                              <i data-lucide="message-circle" class="w-4 h-4 text-amber-400" />
                            </span>
                            <div>
                              <h3 class="font-medium text-wool-200">Gyp</h3>
                              <p class="text-xs text-wool-500">Chat assistant for interactive help</p>
                            </div>
                          </div>

                          <div class="space-y-4">
                            <div class="space-y-2">
                              <label class="block text-sm text-wool-300">Runner</label>
                              <Dropdown
                                value={settings.serviceWorkers?.gyp?.runner || ''}
                                options={runnerSelectOptions()}
                                onChange={(value) => {
                                  if (!settings.serviceWorkers) {
                                    setSettings('serviceWorkers', { scribe: {}, gyp: {} });
                                  }
                                  setSettings('serviceWorkers', 'gyp', 'runner', value || undefined);
                                }}
                              />
                              <p class="text-xs text-wool-500">Where to run the Gyp service worker.</p>
                            </div>

                            <div class="space-y-2">
                              <label class="block text-sm text-wool-300">Idle Timeout (seconds)</label>
                              <input
                                type="number"
                                class="input w-full"
                                value={settings.serviceWorkers?.gyp?.idleTimeoutSeconds || 300}
                                onInput={(e) => {
                                  if (!settings.serviceWorkers) {
                                    setSettings('serviceWorkers', { scribe: {}, gyp: {} });
                                  }
                                  setSettings('serviceWorkers', 'gyp', 'idleTimeoutSeconds', Number.parseInt(e.currentTarget.value) || undefined);
                                }}
                                min={30}
                                step={30}
                                placeholder="300"
                              />
                              <p class="text-xs text-wool-500">Keep worker alive after last activity (default: 5 minutes).</p>
                            </div>
                          </div>
                        </div>
                      </div>
                    </Show>
                  </div>
                </div>
              </Show>

              {/* About Tab */}
              <Show when={!loading() && activeTab() === 'about'}>
                <div class="max-w-2xl mx-auto space-y-4">
                  <div>
                    <h3 class="font-medium text-wool-200">Hirsel</h3>
                    <p class="text-sm text-wool-500 mt-1">
                      Herd your AI coding agents
                    </p>
                  </div>
                  <Show when={app.versionInfo()}>
                    <div class="space-y-2 text-sm">
                      <div class="flex justify-between">
                        <span class="text-wool-500">Version</span>
                        <span class="text-wool-300">{app.versionInfo()?.version}</span>
                      </div>
                      <div class="flex justify-between">
                        <span class="text-wool-500">Build</span>
                        <span class="text-wool-300 font-mono text-xs">{app.versionInfo()?.gitSha}</span>
                      </div>
                      <div class="flex justify-between">
                        <span class="text-wool-500">Build Date</span>
                        <span class="text-wool-300">{app.versionInfo()?.buildDate}</span>
                      </div>
                    </div>
                  </Show>
                </div>
              </Show>
            </div>

            {/* Footer */}
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

        {/* Add Profile Dialog */}
        <Show when={addingProfile()}>
          <div class="fixed inset-0 z-60 flex items-center justify-center bg-black/50">
            <div class="bg-pasture-800 border border-pasture-600 rounded-lg shadow-xl w-[400px] p-6">
              <h3 class="text-lg font-medium text-wool-100 mb-4">Add Remote Profile</h3>
              <div class="space-y-4">
                <div class="space-y-2">
                  <label class="block text-sm text-wool-300">Profile Name</label>
                  <input
                    type="text"
                    class="input w-full"
                    value={newProfileName()}
                    onInput={(e) => setNewProfileName(e.currentTarget.value)}
                    placeholder="my-server"
                  />
                </div>
                <div class="space-y-2">
                  <label class="block text-sm text-wool-300">Server URL</label>
                  <input
                    type="text"
                    class="input w-full"
                    value={newProfileUrl()}
                    onInput={(e) => setNewProfileUrl(e.currentTarget.value)}
                    placeholder="http://100.x.x.x:8080"
                  />
                </div>
              </div>
              <div class="flex justify-end gap-3 mt-6">
                <button onClick={() => setAddingProfile(false)} class="btn btn-ghost">
                  Cancel
                </button>
                <button
                  onClick={addProfile}
                  disabled={!newProfileName().trim() || !newProfileUrl().trim()}
                  class="btn"
                >
                  Add Profile
                </button>
              </div>
            </div>
          </div>
        </Show>

        {/* Add/Edit Runner Dialog */}
        <Show when={addingRunner()}>
          <div class="fixed inset-0 z-60 flex items-center justify-center bg-black/50">
            <div class="bg-pasture-800 border border-pasture-600 rounded-lg shadow-xl w-[500px] max-h-[80vh] overflow-y-auto p-6">
              <h3 class="text-lg font-medium text-wool-100 mb-4">
                {editingRunner() ? 'Edit Runner' : 'Add Runner'}
              </h3>
              <div class="space-y-4">
                <div class="space-y-2">
                  <label class="block text-sm text-wool-300">Runner Name</label>
                  <input
                    type="text"
                    class="input w-full"
                    value={runnerForm.name}
                    onInput={(e) => setRunnerForm('name', e.currentTarget.value)}
                    placeholder="my-runner"
                    disabled={!!editingRunner()}
                  />
                </div>

                <div class="space-y-2">
                  <label class="block text-sm text-wool-300">Host Type</label>
                  <Dropdown
                    value={runnerForm.hostType}
                    options={hostTypeOptions}
                    onChange={(value) => setRunnerForm('hostType', value as typeof runnerForm.hostType)}
                  />
                </div>

                {/* SSH fields */}
                <Show when={runnerForm.hostType === 'ssh'}>
                  <div class="space-y-4 pl-4 border-l-2 border-pasture-600">
                    <div class="space-y-2">
                      <label class="block text-sm text-wool-300">Host Address</label>
                      <input
                        type="text"
                        class="input w-full"
                        value={runnerForm.address}
                        onInput={(e) => setRunnerForm('address', e.currentTarget.value)}
                        placeholder="192.168.1.100"
                      />
                    </div>
                    <div class="grid grid-cols-2 gap-4">
                      <div class="space-y-2">
                        <label class="block text-sm text-wool-300">User (optional)</label>
                        <input
                          type="text"
                          class="input w-full"
                          value={runnerForm.user}
                          onInput={(e) => setRunnerForm('user', e.currentTarget.value)}
                          placeholder="root"
                        />
                      </div>
                      <div class="space-y-2">
                        <label class="block text-sm text-wool-300">Port</label>
                        <input
                          type="number"
                          class="input w-full"
                          value={runnerForm.port}
                          onInput={(e) => setRunnerForm('port', Number.parseInt(e.currentTarget.value) || 22)}
                          min={1}
                          max={65535}
                        />
                      </div>
                    </div>
                  </div>
                </Show>

                {/* Fly.io fields */}
                <Show when={runnerForm.hostType === 'fly'}>
                  <div class="space-y-4 pl-4 border-l-2 border-pasture-600">
                    <div class="space-y-2">
                      <label class="block text-sm text-wool-300">App Name</label>
                      <input
                        type="text"
                        class="input w-full"
                        value={runnerForm.flyApp}
                        onInput={(e) => setRunnerForm('flyApp', e.currentTarget.value)}
                        placeholder="my-workers"
                      />
                    </div>
                    <div class="grid grid-cols-3 gap-4">
                      <div class="space-y-2">
                        <label class="block text-sm text-wool-300">Region</label>
                        <input
                          type="text"
                          class="input w-full"
                          value={runnerForm.flyRegion}
                          onInput={(e) => setRunnerForm('flyRegion', e.currentTarget.value)}
                          placeholder="ams"
                        />
                      </div>
                      <div class="space-y-2">
                        <label class="block text-sm text-wool-300">CPUs</label>
                        <input
                          type="number"
                          class="input w-full"
                          value={runnerForm.flyCpus}
                          onInput={(e) => setRunnerForm('flyCpus', Number.parseInt(e.currentTarget.value) || 2)}
                          min={1}
                          max={16}
                        />
                      </div>
                      <div class="space-y-2">
                        <label class="block text-sm text-wool-300">Memory (MB)</label>
                        <input
                          type="number"
                          class="input w-full"
                          value={runnerForm.flyMemory}
                          onInput={(e) => setRunnerForm('flyMemory', Number.parseInt(e.currentTarget.value) || 2048)}
                          min={256}
                          max={16384}
                          step={256}
                        />
                      </div>
                    </div>
                  </div>
                </Show>

                {/* Docker container toggle */}
                <Switch
                  id="docker-switch"
                  checked={runnerForm.useDocker}
                  onChange={(checked) => setRunnerForm('useDocker', checked)}
                  label="Use Docker Container"
                  description="Run workers inside a Docker container on the host."
                />

                <Show when={runnerForm.useDocker}>
                  <div class="space-y-2 pl-4 border-l-2 border-pasture-600">
                    <label class="block text-sm text-wool-300">Docker Image</label>
                    <input
                      type="text"
                      class="input w-full"
                      value={runnerForm.dockerImage}
                      onInput={(e) => setRunnerForm('dockerImage', e.currentTarget.value)}
                      placeholder="debian:bookworm-slim"
                    />
                  </div>
                </Show>
              </div>

              <div class="flex justify-end gap-3 mt-6">
                <button
                  onClick={() => {
                    setAddingRunner(false);
                    setEditingRunner(null);
                  }}
                  class="btn btn-ghost"
                >
                  Cancel
                </button>
                <button
                  onClick={saveRunner}
                  disabled={!runnerForm.name.trim()}
                  class="btn"
                >
                  {editingRunner() ? 'Save Runner' : 'Add Runner'}
                </button>
              </div>
            </div>
          </div>
        </Show>

        {/* Add/Edit Storage Dialog */}
        <Show when={addingStorage()}>
          <div class="fixed inset-0 z-60 flex items-center justify-center bg-black/50">
            <div class="bg-pasture-800 border border-pasture-600 rounded-lg shadow-xl w-[500px] max-h-[80vh] overflow-y-auto p-6">
              <h3 class="text-lg font-medium text-wool-100 mb-4">
                {editingStorage() ? 'Edit Storage' : 'Add Storage'}
              </h3>
              <div class="space-y-4">
                <div class="space-y-2">
                  <label class="block text-sm text-wool-300">Storage Name</label>
                  <input
                    type="text"
                    class="input w-full"
                    value={storageForm.name}
                    onInput={(e) => setStorageForm('name', e.currentTarget.value)}
                    placeholder="my-s3"
                    disabled={!!editingStorage()}
                  />
                </div>

                <div class="space-y-2">
                  <label class="block text-sm text-wool-300">Provider</label>
                  <Dropdown
                    value={storageForm.provider}
                    options={storageProviderOptions}
                    onChange={(value) => setStorageForm('provider', value as typeof storageForm.provider)}
                  />
                </div>

                <div class="space-y-2">
                  <label class="block text-sm text-wool-300">Endpoint (optional for S3)</label>
                  <input
                    type="text"
                    class="input w-full"
                    value={storageForm.endpoint}
                    onInput={(e) => setStorageForm('endpoint', e.currentTarget.value)}
                    placeholder="http://localhost:9000"
                  />
                </div>

                <div class="grid grid-cols-2 gap-4">
                  <div class="space-y-2">
                    <label class="block text-sm text-wool-300">Bucket</label>
                    <input
                      type="text"
                      class="input w-full"
                      value={storageForm.bucket}
                      onInput={(e) => setStorageForm('bucket', e.currentTarget.value)}
                      placeholder="hirsel"
                    />
                  </div>
                  <div class="space-y-2">
                    <label class="block text-sm text-wool-300">Region</label>
                    <input
                      type="text"
                      class="input w-full"
                      value={storageForm.region}
                      onInput={(e) => setStorageForm('region', e.currentTarget.value)}
                      placeholder="us-east-1"
                    />
                  </div>
                </div>

                <div class="space-y-2">
                  <label class="block text-sm text-wool-300">Access Key ID</label>
                  <input
                    type="text"
                    class="input w-full"
                    value={storageForm.accessKeyId}
                    onInput={(e) => setStorageForm('accessKeyId', e.currentTarget.value)}
                    placeholder="AKIA..."
                  />
                </div>

                <div class="space-y-2">
                  <label class="block text-sm text-wool-300">Secret Access Key</label>
                  <input
                    type="password"
                    class="input w-full"
                    value={storageForm.secretAccessKey}
                    onInput={(e) => setStorageForm('secretAccessKey', e.currentTarget.value)}
                    placeholder="••••••••"
                  />
                </div>
              </div>

              <div class="flex justify-end gap-3 mt-6">
                <button
                  onClick={() => {
                    setAddingStorage(false);
                    setEditingStorage(null);
                  }}
                  class="btn btn-ghost"
                >
                  Cancel
                </button>
                <button
                  onClick={saveStorage}
                  disabled={!storageForm.name.trim() || !storageForm.bucket.trim()}
                  class="btn"
                >
                  {editingStorage() ? 'Save Storage' : 'Add Storage'}
                </button>
              </div>
            </div>
          </div>
        </Show>
      </div>
    </Show>
  );
};
