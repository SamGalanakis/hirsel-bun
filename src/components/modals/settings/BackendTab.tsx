import { invoke } from '../../../lib/invoke';
import {
  type Accessor,
  type Component,
  For,
  Show,
  createSignal,
  type Setter,
} from 'solid-js';
import { createStore, type SetStoreFunction } from 'solid-js/store';
import { Dropdown, Icon, type DropdownOption } from '../../shared';
import { Switch } from './Switch';
import type {
  BackendHealth,
  BackendSection,
  CodexDeviceExchangeResponse,
  CodexDevicePollResponse,
  CodexDeviceStartResponse,
  Runner,
  Settings,
  StorageConfig,
} from './types';

export interface BackendTabRef {
  initCredentialState: () => Promise<void>;
  saveCredentials: () => Promise<void>;
}

export interface BackendTabProps {
  settings: Settings;
  setSettings: SetStoreFunction<Settings>;
  section: Accessor<BackendSection>;
  setSection: Setter<BackendSection>;
  backendHealth: Accessor<BackendHealth | null>;
  checkBackendHealth: () => Promise<void>;
  ref?: (ref: BackendTabRef) => void;
}

export const BackendTab: Component<BackendTabProps> = (props) => {
  const [addingRunner, setAddingRunner] = createSignal(false);
  const [editingRunner, setEditingRunner] = createSignal<string | null>(null);
  const [runnerForm, setRunnerForm] = createStore({
    name: '',
    useDocker: false,
    dockerImage: 'debian:bookworm-slim',
  });

  const [addingStorage, setAddingStorage] = createSignal(false);
  const [editingStorage, setEditingStorage] = createSignal<string | null>(null);
  const [storageForm, setStorageForm] = createStore({
    name: '',
    provider: 's3' as 's3' | 'minio',
    endpoint: '',
    bucket: '',
    region: 'us-east-1',
    accessKeyId: '',
    secretAccessKey: '',
  });

  const [gitHubToken, setGitHubToken] = createSignal('');
  const [openrouterApiKey, setOpenrouterApiKey] = createSignal('');
  const [openrouterKeyConfigured, setOpenrouterKeyConfigured] = createSignal(false);
  const [codexLoggedIn, setCodexLoggedIn] = createSignal(false);
  const [codexLoginInProgress, setCodexLoginInProgress] = createSignal(false);
  const [codexLoginStatus, setCodexLoginStatus] = createSignal('');
  const [codexUserCode, setCodexUserCode] = createSignal('');
  const [codexVerifyUrl, setCodexVerifyUrl] = createSignal('');
  const [codexExpiresAt, setCodexExpiresAt] = createSignal<number | null>(null);

  const providerOptions: DropdownOption[] = [
    { value: 'codex', label: 'Codex (OpenAI)' },
    { value: 'openrouter', label: 'OpenRouter' },
  ];

  const pauseOptions: DropdownOption[] = [
    { value: 'sender', label: 'Sender only' },
    { value: 'all', label: 'All workers' },
    { value: 'none', label: 'None' },
  ];

  const runnerSelectOptions = (): DropdownOption[] => [
    { value: '', label: 'host (default)' },
    ...Object.keys(props.settings.runners).map((name) => ({ value: name, label: name })),
  ];

  const storageProviderOptions: DropdownOption[] = [
    { value: 's3', label: 'Amazon S3' },
    { value: 'minio', label: 'MinIO' },
  ];

  const runnerNames = () => Object.keys(props.settings.runners);
  const storageNames = () => Object.keys(props.settings.storage?.configs || {});
  const getRunner = (name: string) => props.settings.runners[name];
  const getStorage = (name: string) => props.settings.storage?.configs[name];

  const getStorageProviderLabel = (provider: string) => {
    switch (provider) {
      case 's3':
        return 'Amazon S3';
      case 'minio':
        return 'MinIO';
      default:
        return provider;
    }
  };

  const setDefaultStorage = (name: string) => {
    props.setSettings('storage', 'defaultStorage', name);
  };

  const setDefaultRunner = (name: string | null) => {
    props.setSettings('defaultRunner', name || undefined);
  };

  const startAddRunner = () => {
    setRunnerForm({
      name: '',
      useDocker: false,
      dockerImage: 'debian:bookworm-slim',
    });
    setEditingRunner(null);
    setAddingRunner(true);
  };

  const startEditRunner = (name: string) => {
    const runner = props.settings.runners[name];
    if (!runner) return;

    setRunnerForm({
      name,
      useDocker: !!runner.container,
      dockerImage: runner.container?.image || 'debian:bookworm-slim',
    });
    setEditingRunner(name);
    setAddingRunner(true);
  };

  const saveRunner = () => {
    const name = runnerForm.name.trim();
    if (!name) return;

    const runner: Runner = {};
    if (runnerForm.useDocker && runnerForm.dockerImage.trim()) {
      runner.container = { image: runnerForm.dockerImage.trim() };
    }

    if (editingRunner() && editingRunner() !== name) {
      props.setSettings('runners', editingRunner()!, undefined!);
    }

    props.setSettings('runners', name, runner);
    setEditingRunner(null);
    setAddingRunner(false);
  };

  const deleteRunner = async (name: string) => {
    const confirmed = await window.confirmDialog?.show({
      title: 'Delete Runner',
      message: `Delete runner "${name}"?`,
      confirmText: 'Delete',
      danger: true,
    });
    if (!confirmed) return;

    props.setSettings('runners', name, undefined!);
    if (props.settings.defaultRunner === name) {
      props.setSettings('defaultRunner', undefined);
    }
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
    setEditingStorage(null);
    setAddingStorage(true);
  };

  const startEditStorage = (name: string) => {
    const storage = props.settings.storage?.configs[name];
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
    setEditingStorage(name);
    setAddingStorage(true);
  };

  const saveStorage = () => {
    const name = storageForm.name.trim();
    if (!name || !storageForm.bucket.trim()) return;

    const config: StorageConfig = {
      provider: storageForm.provider,
      endpoint: storageForm.endpoint || undefined,
      bucket: storageForm.bucket.trim(),
      region: storageForm.region || undefined,
      accessKeyId: storageForm.accessKeyId || undefined,
      secretAccessKey: storageForm.secretAccessKey || undefined,
    };

    if (editingStorage() && editingStorage() !== name) {
      const configs = { ...(props.settings.storage?.configs || {}) };
      delete configs[editingStorage()!];
      props.setSettings('storage', 'configs', configs);
    }

    if (!props.settings.storage) {
      props.setSettings('storage', { configs: { [name]: config } });
    } else {
      props.setSettings('storage', 'configs', name, config);
    }

    if (!props.settings.storage?.defaultStorage) {
      props.setSettings('storage', 'defaultStorage', name);
    }

    setEditingStorage(null);
    setAddingStorage(false);
  };

  const deleteStorage = async (name: string) => {
    const confirmed = await window.confirmDialog?.show({
      title: 'Delete Storage',
      message: `Delete storage configuration "${name}"?`,
      confirmText: 'Delete',
      danger: true,
    });
    if (!confirmed) return;

    const configs = { ...(props.settings.storage?.configs || {}) };
    delete configs[name];
    props.setSettings('storage', 'configs', configs);

    if (props.settings.storage?.defaultStorage === name) {
      const remaining = Object.keys(configs);
      props.setSettings('storage', 'defaultStorage', remaining[0] || undefined);
    }
  };

  const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

  const refreshCodexAuthState = async () => {
    const [hasAccessToken, hasRefreshToken, expiresAtRaw] = await Promise.all([
      invoke<boolean>('has_credential', { keyType: 'codex_access_token' }).catch(() => false),
      invoke<boolean>('has_credential', { keyType: 'codex_refresh_token' }).catch(() => false),
      invoke<string | null>('get_credential', { keyType: 'codex_expires_at' }).catch(() => null),
    ]);
    setCodexLoggedIn(hasAccessToken && hasRefreshToken);
    const parsed = expiresAtRaw ? Number.parseInt(expiresAtRaw, 10) : Number.NaN;
    setCodexExpiresAt(Number.isFinite(parsed) ? parsed : null);
  };

  const loginWithCodex = async () => {
    if (codexLoginInProgress()) return;
    setCodexLoginInProgress(true);
    setCodexLoginStatus('Starting device login...');
    setCodexUserCode('');
    setCodexVerifyUrl('');

    try {
      const start = await invoke<CodexDeviceStartResponse>('codex_device_start_gui');
      setCodexUserCode(start.userCode);
      setCodexVerifyUrl(start.verifyUrl);
      setCodexLoginStatus('Waiting for approval...');

      window.open(start.verifyUrl, '_blank', 'noopener,noreferrer');

      const pollIntervalMs = Math.max(start.interval, 1) * 1000;
      const deadline = Date.now() + 10 * 60 * 1000;
      let approvedCode: string | undefined;
      let approvedVerifier: string | undefined;

      while (Date.now() < deadline) {
        await sleep(pollIntervalMs);
        const poll = await invoke<CodexDevicePollResponse>('codex_device_poll_gui', {
          deviceAuthId: start.deviceAuthId,
          userCode: start.userCode,
        });

        if (poll.status === 'approved' && poll.authorizationCode && poll.codeVerifier) {
          approvedCode = poll.authorizationCode;
          approvedVerifier = poll.codeVerifier;
          break;
        }
      }

      if (!approvedCode || !approvedVerifier) {
        throw new Error('Timed out waiting for approval');
      }

      setCodexLoginStatus('Exchanging authorization...');
      const exchange = await invoke<CodexDeviceExchangeResponse>('codex_device_exchange_gui', {
        authorizationCode: approvedCode,
        codeVerifier: approvedVerifier,
      });

      if (exchange.status !== 'ok') {
        throw new Error('Token exchange failed');
      }

      await refreshCodexAuthState();
      setCodexLoginStatus('Connected');
      window.toast?.success('Codex connected');
    } catch (e) {
      console.error('Codex login failed:', e);
      setCodexLoginStatus('Login failed');
      window.toast?.error(`Codex login failed: ${String(e)}`);
    } finally {
      setCodexLoginInProgress(false);
    }
  };

  const initCredentialState = async () => {
    const hasOpenrouter = await invoke<boolean>('has_credential', {
      keyType: 'openrouter_api_key',
    }).catch(() => false);
    setOpenrouterKeyConfigured(hasOpenrouter);
    await refreshCodexAuthState();
  };

  const saveCredentials = async () => {
    if (gitHubToken().trim()) {
      await invoke('store_credential', {
        keyType: 'git_github_token',
        value: gitHubToken().trim(),
      });
    }
    if (props.settings.llm?.provider === 'openrouter' && openrouterApiKey().trim()) {
      await invoke('store_credential', {
        keyType: 'openrouter_api_key',
        value: openrouterApiKey().trim(),
      });
    }
  };

  props.ref?.({ initCredentialState, saveCredentials });

  return (
    <>
      <div class="flex flex-col h-full">
        <div class="flex items-center justify-between mb-4">
          <div class="flex items-center gap-3">
            <Icon name="server" class="w-5 h-5 text-sage" />
            <h3 class="text-lg font-medium text-wool-100">Backend</h3>
            <Show when={props.backendHealth()}>
              {(health) => (
                <span
                  class="flex items-center gap-1.5 text-xs"
                  classList={{
                    'text-wool-500': health().status === 'checking',
                    'text-green-400': health().status === 'online' && !health().error,
                    'text-amber-400': health().status === 'online' && !!health().error,
                    'text-terra': health().status === 'offline',
                  }}
                >
                  <span
                    class="w-2 h-2 rounded-none"
                    classList={{
                      'bg-wool-500 animate-pulse': health().status === 'checking',
                      'bg-green-400': health().status === 'online' && !health().error,
                      'bg-amber-400': health().status === 'online' && !!health().error,
                      'bg-terra': health().status === 'offline',
                    }}
                  />
                  {health().status === 'online' && health().error ? 'auth error' : health().status}
                </span>
              )}
            </Show>
          </div>
          <button type="button" class="btn btn-ghost btn-sm" onClick={() => void props.checkBackendHealth()}>
            Check connection
          </button>
        </div>

        <div class="flex gap-1 mb-6 border-b border-pasture-600 pb-2">
          <button
            onClick={() => props.setSection('connection')}
            class="px-3 py-1.5 rounded-none text-sm transition-colors"
            classList={{
              'bg-pasture-700 text-wool-100': props.section() === 'connection',
              'text-wool-400 hover:bg-pasture-700/50': props.section() !== 'connection',
            }}
          >
            Connection
          </button>
          <button
            onClick={() => props.setSection('agents')}
            class="px-3 py-1.5 rounded-none text-sm transition-colors"
            classList={{
              'bg-pasture-700 text-wool-100': props.section() === 'agents',
              'text-wool-400 hover:bg-pasture-700/50': props.section() !== 'agents',
            }}
          >
            LLM
          </button>
          <button
            onClick={() => props.setSection('runners')}
            class="px-3 py-1.5 rounded-none text-sm transition-colors"
            classList={{
              'bg-pasture-700 text-wool-100': props.section() === 'runners',
              'text-wool-400 hover:bg-pasture-700/50': props.section() !== 'runners',
            }}
          >
            Runners
          </button>
          <button
            onClick={() => props.setSection('defaults')}
            class="px-3 py-1.5 rounded-none text-sm transition-colors"
            classList={{
              'bg-pasture-700 text-wool-100': props.section() === 'defaults',
              'text-wool-400 hover:bg-pasture-700/50': props.section() !== 'defaults',
            }}
          >
            Defaults
          </button>
          <button
            onClick={() => props.setSection('git')}
            class="px-3 py-1.5 rounded-none text-sm transition-colors"
            classList={{
              'bg-pasture-700 text-wool-100': props.section() === 'git',
              'text-wool-400 hover:bg-pasture-700/50': props.section() !== 'git',
            }}
          >
            Git
          </button>
          <button
            onClick={() => props.setSection('data')}
            class="px-3 py-1.5 rounded-none text-sm transition-colors"
            classList={{
              'bg-pasture-700 text-wool-100': props.section() === 'data',
              'text-wool-400 hover:bg-pasture-700/50': props.section() !== 'data',
            }}
          >
            Data
          </button>
          <button
            onClick={() => props.setSection('services')}
            class="px-3 py-1.5 rounded-none text-sm transition-colors"
            classList={{
              'bg-pasture-700 text-wool-100': props.section() === 'services',
              'text-wool-400 hover:bg-pasture-700/50': props.section() !== 'services',
            }}
          >
            Services
          </button>
        </div>

        <div class="max-w-2xl overflow-y-auto flex-1">
          <Show when={props.section() === 'connection'}>
            <div class="space-y-6">
              <p class="text-sm text-wool-500">
                Connect this Hirsel app to a backend. Leave the URL empty to use the local embedded backend on this machine.
              </p>

              <div class="space-y-2">
                <label class="block text-sm font-medium text-wool-300">Backend URL</label>
                <input
                  type="text"
                  class="input w-full"
                  value={props.settings.backend.url || ''}
                  onInput={(e) => props.setSettings('backend', 'url', e.currentTarget.value)}
                  placeholder="http://hirsel-host.tailnet.ts.net:8080"
                />
                <p class="text-xs text-wool-500">Use the backend URL you want this client to connect to.</p>
              </div>

              <div class="space-y-2">
                <label class="block text-sm font-medium text-wool-300">Backend API Key</label>
                <input
                  type="password"
                  class="input w-full"
                  value={props.settings.backend.apiKey || ''}
                  onInput={(e) => props.setSettings('backend', 'apiKey', e.currentTarget.value)}
                  placeholder="hirsel_..."
                />
                <p class="text-xs text-wool-500">Used by this app when connecting to a remote Hirsel backend.</p>
              </div>

              <Show when={props.backendHealth()?.error}>
                <p class="text-sm text-terra">{props.backendHealth()?.error}</p>
              </Show>
            </div>
          </Show>

          <Show when={props.section() === 'agents'}>
            <div class="space-y-6">
              <div class="space-y-2">
                <label class="block text-sm font-medium text-wool-300">LLM Provider</label>
                <Dropdown
                  value={props.settings.llm?.provider || 'codex'}
                  options={providerOptions}
                  onChange={(value) => props.setSettings('llm', 'provider', value as 'codex' | 'openrouter')}
                />
                <p class="text-xs text-wool-500">Used by Lash for Shepherd chat and worker runtimes.</p>
              </div>

              <Show when={props.settings.llm?.provider === 'codex'}>
                <div class="space-y-4">
                  <div class="p-4 bg-pasture-700/30 rounded-none space-y-4">
                    <div class="flex items-center justify-between gap-3">
                      <div class="text-sm">
                        <p class="text-wool-300">
                          Status:{' '}
                          <span classList={{ 'text-green-400': codexLoggedIn(), 'text-amber-400': !codexLoggedIn() }}>
                            {codexLoggedIn() ? 'Connected' : 'Not connected'}
                          </span>
                        </p>
                        <Show when={codexExpiresAt()}>
                          <p class="text-xs text-wool-500 mt-1">
                            Token expiry: {new Date((codexExpiresAt() || 0) * 1000).toLocaleString()}
                          </p>
                        </Show>
                      </div>
                      <button
                        type="button"
                        class="btn btn-sm"
                        disabled={codexLoginInProgress()}
                        onClick={() => void loginWithCodex()}
                      >
                        {codexLoginInProgress() ? 'Connecting...' : codexLoggedIn() ? 'Reconnect' : 'Connect Codex'}
                      </button>
                    </div>

                    <Show when={codexUserCode()}>
                      <div class="space-y-2">
                        <p class="text-xs text-wool-500">Finish approval in your browser with this code:</p>
                        <div class="flex items-center gap-3">
                          <code class="bg-pasture-900 px-3 py-2 text-lg tracking-[0.25em] text-wool-100">{codexUserCode()}</code>
                          <a href={codexVerifyUrl()} target="_blank" rel="noreferrer" class="text-sm text-amber-500 hover:underline">
                            Open verification page
                          </a>
                        </div>
                      </div>
                    </Show>

                    <Show when={codexLoginStatus()}>
                      <p class="text-xs text-wool-500">{codexLoginStatus()}</p>
                    </Show>
                  </div>
                </div>
              </Show>

              <Show when={props.settings.llm?.provider === 'openrouter'}>
                <div class="space-y-4">
                  <div class="space-y-2">
                    <label class="block text-sm font-medium text-wool-300">OpenRouter API Key</label>
                    <input
                      type="password"
                      class="input w-full"
                      value={openrouterApiKey()}
                      onInput={(e) => setOpenrouterApiKey(e.currentTarget.value)}
                      placeholder="sk-or-..."
                    />
                    <div class="text-xs text-wool-500">
                      <Show when={openrouterKeyConfigured()}>
                        <span class="text-green-400">A key is already configured.</span>
                      </Show>
                      <Show when={!openrouterKeyConfigured()}>
                        <span>No key configured yet.</span>
                      </Show>
                    </div>
                  </div>

                  <div class="space-y-2">
                    <label class="block text-sm font-medium text-wool-300">Base URL Override</label>
                    <input
                      type="text"
                      class="input w-full"
                      value={props.settings.llm?.openrouterBaseUrl || ''}
                      onInput={(e) => props.setSettings('llm', 'openrouterBaseUrl', e.currentTarget.value)}
                      placeholder="https://openrouter.ai/api/v1"
                    />
                  </div>
                </div>
              </Show>
            </div>
          </Show>

          <Show when={props.section() === 'runners'}>
            <div class="space-y-6">
              <p class="text-sm text-wool-500">
                Named runners execute on the backend host. Add a container image only when you want sandboxed worker execution.
              </p>

              <div class="space-y-3">
                <div class="flex items-center justify-between">
                  <h4 class="text-sm font-medium text-wool-200">Configured Runners</h4>
                  <button type="button" onClick={startAddRunner} class="btn btn-ghost btn-sm">
                    <Icon name="plus" class="w-4 h-4 mr-1" />
                    Add Runner
                  </button>
                </div>

                <div class="p-3 bg-pasture-700/30 rounded-none border border-pasture-600/50">
                  <div class="flex items-center justify-between">
                    <div class="flex items-center gap-2">
                      <Icon name="server" class="w-4 h-4 text-sage" />
                      <span class="font-medium text-wool-200">host</span>
                      <span class="badge-secondary">Built-in</span>
                      <Show when={!props.settings.defaultRunner}>
                        <span class="badge">default</span>
                      </Show>
                    </div>
                    <Show when={props.settings.defaultRunner}>
                      <button type="button" onClick={() => setDefaultRunner(null)} class="btn btn-ghost btn-sm text-wool-400">
                        Set Default
                      </button>
                    </Show>
                  </div>
                  <div class="mt-2 text-xs text-wool-500">Workers run directly on the backend host.</div>
                </div>

                <For each={runnerNames()}>
                  {(name) => (
                    <div class="p-3 bg-pasture-700/30 rounded-none border border-pasture-600/50">
                      <div class="flex items-center justify-between">
                        <div class="flex items-center gap-2">
                          <Icon name={getRunner(name)?.container ? 'container' : 'server'} class="w-4 h-4 text-sage" />
                          <span class="font-medium text-wool-200">{name}</span>
                          <span class="badge-secondary">Host</span>
                          <Show when={getRunner(name)?.container}>
                            <span class="badge-outline">Container</span>
                          </Show>
                          <Show when={props.settings.defaultRunner === name}>
                            <span class="badge">default</span>
                          </Show>
                        </div>
                        <div class="flex items-center gap-2">
                          <Show when={props.settings.defaultRunner !== name}>
                            <button type="button" onClick={() => setDefaultRunner(name)} class="btn btn-ghost btn-sm text-wool-400">
                              Set Default
                            </button>
                          </Show>
                          <button type="button" onClick={() => startEditRunner(name)} class="btn btn-ghost btn-sm">
                            <Icon name="pencil" class="w-4 h-4 mr-1" />
                            Edit
                          </button>
                          <button type="button" onClick={() => void deleteRunner(name)} class="btn btn-ghost btn-sm text-wool-400 hover:text-terra">
                            <Icon name="trash-2" class="w-4 h-4" />
                          </button>
                        </div>
                      </div>
                      <div class="mt-2 text-xs text-wool-500 flex flex-wrap gap-x-4 gap-y-1">
                        <span class="flex items-center gap-1">
                          <span class="text-wool-600">Execution:</span>
                          <code class="text-wool-400">{getRunner(name)?.container ? 'Containerized on host' : 'Bare host'}</code>
                        </span>
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

          <Show when={props.section() === 'defaults'}>
            <div class="space-y-6">
              <div class="space-y-2">
                <label class="block text-sm text-wool-300">Default Runner</label>
                <Dropdown
                  value={props.settings.defaultRunner || ''}
                  options={runnerSelectOptions()}
                  onChange={(value) => setDefaultRunner(value || null)}
                />
                <p class="text-xs text-wool-500">Used when a project or task does not pick a specific runner.</p>
              </div>

              <div class="space-y-2">
                <label class="block text-sm text-wool-300">Eval Timeout (seconds)</label>
                <input
                  type="number"
                  class="input w-full"
                  value={props.settings.evalTimeout}
                  onInput={(e) => props.setSettings('evalTimeout', Number.parseInt(e.currentTarget.value, 10) || 300)}
                  min={60}
                  step={60}
                />
                <p class="text-xs text-wool-500">Time limit for eval agents.</p>
              </div>

              <Switch
                id="hitl-switch"
                checked={props.settings.humanInTheLoop}
                onChange={(checked) => props.setSettings('humanInTheLoop', checked)}
                label="Human in the Loop"
                description="Allow agents to send messages to the user and wait for a response."
              />

              <div class="space-y-2">
                <label class="block text-sm text-wool-300">Pause on User Message</label>
                <Dropdown
                  value={props.settings.userMessagePause}
                  options={pauseOptions}
                  onChange={(value) => props.setSettings('userMessagePause', value as 'sender' | 'all' | 'none')}
                />
                <p class="text-xs text-wool-500">Which workers pause when a user message is sent.</p>
              </div>

              <Switch
                id="autolearn-switch"
                checked={props.settings.autoLearn}
                onChange={(checked) => props.setSettings('autoLearn', checked)}
                label="Auto Learn"
                description="Enable Scribe to update retained context based on worker discoveries."
              />
            </div>
          </Show>

          <Show when={props.section() === 'git'}>
            <div class="space-y-6">
              <p class="text-sm text-wool-500">Configure git provider tokens for repository access.</p>

              <div class="p-4 bg-pasture-700/30 rounded-none border border-pasture-600/50">
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
                  <Show when={props.settings.git?.configuredProviders?.includes('github')}>
                    <span class="text-xs px-2 py-0.5 bg-green-500/20 text-green-400 rounded-none flex items-center gap-1">
                      <Icon name="check" class="w-3 h-3" />
                      Configured
                    </span>
                  </Show>
                  <Show when={!props.settings.git?.configuredProviders?.includes('github')}>
                    <span class="text-xs px-2 py-0.5 bg-amber-500/20 text-amber-400 rounded">Not configured</span>
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
                </div>
              </div>
            </div>
          </Show>

          <Show when={props.section() === 'data'}>
            <div class="space-y-6">
              <div class="space-y-2">
                <label class="block text-sm font-medium text-wool-300">Client Data</label>
                <div class="space-y-2 text-sm">
                  <div class="flex justify-between items-center">
                    <span class="text-wool-500">Config</span>
                    <code class="text-wool-400 text-xs bg-pasture-700 px-1.5 py-0.5 rounded">~/.hirsel/config.toml</code>
                  </div>
                  <div class="flex justify-between items-center">
                    <span class="text-wool-500">Credentials</span>
                    <code class="text-wool-400 text-xs bg-pasture-700 px-1.5 py-0.5 rounded">~/.hirsel/hirsel.db</code>
                  </div>
                </div>
              </div>

              <div class="rounded-none border border-pasture-600/50 bg-pasture-700/20 px-3 py-2 text-sm text-wool-500">
                Backend route state, runs, artifacts, and workspaces live on the selected backend host.
              </div>

              <div class="space-y-4">
                <div class="flex items-center justify-between">
                  <div>
                    <label class="block text-sm font-medium text-wool-300">Cloud Storage</label>
                    <p class="text-xs text-wool-500">S3-compatible storage for snapshots and exported files.</p>
                  </div>
                  <button type="button" onClick={startAddStorage} class="btn btn-sm">
                    <Icon name="plus" class="w-4 h-4 mr-1" />
                    Add Storage
                  </button>
                </div>

                <Show when={storageNames().length > 0}>
                  <div class="space-y-2">
                    <For each={storageNames()}>
                      {(name) => (
                        <div class="flex items-center justify-between p-3 bg-pasture-700/30 rounded-none">
                          <div class="flex items-center gap-3">
                            <Icon name="database" class="w-4 h-4 text-sage" />
                            <div>
                              <div class="flex items-center gap-2">
                                <span class="font-medium text-wool-200">{name}</span>
                                <Show when={props.settings.storage?.defaultStorage === name}>
                                  <span class="text-xs px-1.5 py-0.5 bg-sage/20 text-sage rounded">Default</span>
                                </Show>
                              </div>
                              <p class="text-xs text-wool-500">
                                {getStorageProviderLabel(getStorage(name)?.provider || 's3')} • {getStorage(name)?.bucket || 'No bucket'}
                              </p>
                            </div>
                          </div>
                          <div class="flex items-center gap-1">
                            <Show when={props.settings.storage?.defaultStorage !== name}>
                              <button type="button" onClick={() => setDefaultStorage(name)} class="btn btn-ghost btn-sm" title="Set as default">
                                <Icon name="star" class="w-4 h-4" />
                              </button>
                            </Show>
                            <button type="button" onClick={() => startEditStorage(name)} class="btn btn-ghost btn-sm" title="Edit">
                              <Icon name="pencil" class="w-4 h-4" />
                            </button>
                            <button type="button" onClick={() => void deleteStorage(name)} class="btn btn-ghost btn-sm text-wool-400 hover:text-terra" title="Delete">
                              <Icon name="trash-2" class="w-4 h-4" />
                            </button>
                          </div>
                        </div>
                      )}
                    </For>
                  </div>
                </Show>
              </div>
            </div>
          </Show>

          <Show when={props.section() === 'services'}>
            <div class="space-y-6">
              <p class="text-sm text-wool-500">
                Hirsel now assumes a single backend control plane. Workers, Scribe, delivery, and orchestration run there; clients connect over your private network.
              </p>
              <div class="rounded-none border border-pasture-600 p-4 space-y-3">
                <div class="flex items-center gap-3">
                  <span class="flex items-center justify-center w-8 h-8 rounded-none bg-sage/20">
                    <Icon name="server" class="w-4 h-4 text-sage" />
                  </span>
                  <div>
                    <h3 class="font-medium text-wool-200">Single Backend Runtime</h3>
                    <p class="text-xs text-wool-500">Run one Hirsel backend on your host or server, then connect from desktop or mobile clients.</p>
                  </div>
                </div>
                <p class="text-sm text-wool-400">
                  Expose the backend on whatever private or public network you manage, then point clients at that URL.
                </p>
              </div>
            </div>
          </Show>
        </div>
      </div>

      <Show when={addingRunner()}>
        <div class="fixed inset-0 z-60 flex items-center justify-center bg-black/50">
          <div class="bg-pasture-800 border border-pasture-600 rounded-none shadow-xl w-[500px] max-h-[80vh] overflow-y-auto p-6">
            <h3 class="text-lg font-medium text-wool-100 mb-4">{editingRunner() ? 'Edit Runner' : 'Add Runner'}</h3>
            <div class="space-y-4">
              <div class="space-y-2">
                <label class="block text-sm text-wool-300">Runner Name</label>
                <input
                  type="text"
                  class="input w-full"
                  value={runnerForm.name}
                  onInput={(e) => setRunnerForm('name', e.currentTarget.value)}
                  placeholder="sandboxed"
                  disabled={!!editingRunner()}
                />
              </div>

              <Switch
                id="docker-switch"
                checked={runnerForm.useDocker}
                onChange={(checked) => setRunnerForm('useDocker', checked)}
                label="Use Container"
                description="Run workers inside a container on the backend host."
              />

              <Show when={runnerForm.useDocker}>
                <div class="space-y-2 pl-4 border-l-2 border-pasture-600">
                  <label class="block text-sm text-wool-300">Container Image</label>
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
                  setEditingRunner(null);
                  setAddingRunner(false);
                }}
                class="btn btn-ghost"
              >
                Cancel
              </button>
              <button onClick={saveRunner} disabled={!runnerForm.name.trim()} class="btn">
                {editingRunner() ? 'Save Runner' : 'Add Runner'}
              </button>
            </div>
          </div>
        </div>
      </Show>

      <Show when={addingStorage()}>
        <div class="fixed inset-0 z-60 flex items-center justify-center bg-black/50">
          <div class="bg-pasture-800 border border-pasture-600 rounded-none shadow-xl w-[500px] max-h-[80vh] overflow-y-auto p-6">
            <h3 class="text-lg font-medium text-wool-100 mb-4">{editingStorage() ? 'Edit Storage' : 'Add Storage'}</h3>
            <div class="space-y-4">
              <div class="space-y-2">
                <label class="block text-sm text-wool-300">Name</label>
                <input
                  type="text"
                  class="input w-full"
                  value={storageForm.name}
                  onInput={(e) => setStorageForm('name', e.currentTarget.value)}
                  placeholder="snapshots"
                />
              </div>

              <div class="space-y-2">
                  <label class="block text-sm text-wool-300">Provider</label>
                  <Dropdown
                    value={storageForm.provider}
                    options={storageProviderOptions}
                    onChange={(value) => setStorageForm('provider', value as 's3' | 'minio')}
                  />
                </div>

              <div class="space-y-2">
                <label class="block text-sm text-wool-300">Bucket</label>
                <input
                  type="text"
                  class="input w-full"
                  value={storageForm.bucket}
                  onInput={(e) => setStorageForm('bucket', e.currentTarget.value)}
                />
              </div>

              <div class="space-y-2">
                <label class="block text-sm text-wool-300">Endpoint</label>
                <input
                  type="text"
                  class="input w-full"
                  value={storageForm.endpoint}
                  onInput={(e) => setStorageForm('endpoint', e.currentTarget.value)}
                  placeholder="https://s3.amazonaws.com"
                />
              </div>

              <div class="space-y-2">
                <label class="block text-sm text-wool-300">Region</label>
                <input
                  type="text"
                  class="input w-full"
                  value={storageForm.region}
                  onInput={(e) => setStorageForm('region', e.currentTarget.value)}
                />
              </div>

              <div class="space-y-2">
                <label class="block text-sm text-wool-300">Access Key ID</label>
                <input
                  type="text"
                  class="input w-full"
                  value={storageForm.accessKeyId}
                  onInput={(e) => setStorageForm('accessKeyId', e.currentTarget.value)}
                />
              </div>

              <div class="space-y-2">
                <label class="block text-sm text-wool-300">Secret Access Key</label>
                <input
                  type="password"
                  class="input w-full"
                  value={storageForm.secretAccessKey}
                  onInput={(e) => setStorageForm('secretAccessKey', e.currentTarget.value)}
                />
              </div>
            </div>

            <div class="flex justify-end gap-3 mt-6">
              <button
                onClick={() => {
                  setEditingStorage(null);
                  setAddingStorage(false);
                }}
                class="btn btn-ghost"
              >
                Cancel
              </button>
              <button onClick={saveStorage} disabled={!storageForm.name.trim() || !storageForm.bucket.trim()} class="btn">
                {editingStorage() ? 'Save Storage' : 'Add Storage'}
              </button>
            </div>
          </div>
        </div>
      </Show>
    </>
  );
};
