import { invoke } from '../../../lib/invoke';
import {
  type Accessor,
  type Component,
  Show,
  createSignal,
  type Setter,
} from 'solid-js';
import type { SetStoreFunction } from 'solid-js/store';
import { Dropdown, Icon, type DropdownOption } from '../../shared';
import type {
  BackendHealth,
  BackendSection,
  CodexDeviceExchangeResponse,
  CodexDevicePollResponse,
  CodexDeviceStartResponse,
  Settings,
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
  const [gitHubToken, setGitHubToken] = createSignal('');
  const [gitHubConfigured, setGitHubConfigured] = createSignal(false);
  const [openrouterApiKey, setOpenrouterApiKey] = createSignal('');
  const [openrouterKeyConfigured, setOpenrouterKeyConfigured] = createSignal(false);
  const [tavilyApiKey, setTavilyApiKey] = createSignal('');
  const [tavilyConfigured, setTavilyConfigured] = createSignal(false);
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

      void invoke('open_external_url', { url: start.verifyUrl });

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
    const [hasGitHub, hasOpenrouter, hasTavily] = await Promise.all([
      invoke<boolean>('has_credential', { keyType: 'git_github_token' }).catch(() => false),
      invoke<boolean>('has_credential', { keyType: 'openrouter_api_key' }).catch(() => false),
      invoke<boolean>('has_credential', { keyType: 'tavily_api_key' }).catch(() => false),
    ]);
    setGitHubConfigured(hasGitHub);
    setOpenrouterKeyConfigured(hasOpenrouter);
    setTavilyConfigured(hasTavily);
    await refreshCodexAuthState();
  };

  const saveCredentials = async () => {
    if (gitHubToken().trim()) {
      await invoke('store_credential', {
        keyType: 'git_github_token',
        value: gitHubToken().trim(),
      });
      setGitHubConfigured(true);
    }
    if (props.settings.llm?.provider === 'openrouter' && openrouterApiKey().trim()) {
      await invoke('store_credential', {
        keyType: 'openrouter_api_key',
        value: openrouterApiKey().trim(),
      });
      setOpenrouterKeyConfigured(true);
    }
    if (tavilyApiKey().trim()) {
      await invoke('store_credential', {
        keyType: 'tavily_api_key',
        value: tavilyApiKey().trim(),
      });
      setTavilyConfigured(true);
    }
  };

  props.ref?.({ initCredentialState, saveCredentials });

  return (
    <div class="flex h-full flex-col">
      <div class="mb-4 flex items-center justify-between">
        <div class="flex items-center gap-3">
          <Icon name="server" class="h-5 w-5 text-sage" />
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
                  class="h-2 w-2 rounded-none"
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

      <div class="mb-6 flex gap-1 border-b border-pasture-600 pb-2">
        <button
          onClick={() => props.setSection('connection')}
          class="rounded-none px-3 py-1.5 text-sm transition-colors"
          classList={{
            'bg-pasture-700 text-wool-100': props.section() === 'connection',
            'text-wool-400 hover:bg-pasture-700/50': props.section() !== 'connection',
          }}
        >
          Connection
        </button>
        <button
          onClick={() => props.setSection('llm')}
          class="rounded-none px-3 py-1.5 text-sm transition-colors"
          classList={{
            'bg-pasture-700 text-wool-100': props.section() === 'llm',
            'text-wool-400 hover:bg-pasture-700/50': props.section() !== 'llm',
          }}
        >
          LLM
        </button>
        <button
          onClick={() => props.setSection('services')}
          class="rounded-none px-3 py-1.5 text-sm transition-colors"
          classList={{
            'bg-pasture-700 text-wool-100': props.section() === 'services',
            'text-wool-400 hover:bg-pasture-700/50': props.section() !== 'services',
          }}
        >
          Services
        </button>
      </div>

      <div class="max-w-2xl flex-1 overflow-y-auto">
        <Show when={props.section() === 'connection'}>
          <div class="space-y-6">
            <p class="text-sm text-wool-500">
              Point this client at the Hirsel backend you want to use.
            </p>

            <div class="space-y-2">
              <label class="block text-sm font-medium text-wool-300">Backend URL</label>
              <input
                type="text"
                class="input w-full"
                value={props.settings.backend.url || ''}
                onInput={(e) => props.setSettings('backend', 'url', e.currentTarget.value)}
                placeholder="http://backend.example.internal:8080"
              />
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
            </div>

            <Show when={props.backendHealth()?.error}>
              <p class="text-sm text-terra">{props.backendHealth()?.error}</p>
            </Show>
          </div>
        </Show>

        <Show when={props.section() === 'llm'}>
          <div class="space-y-6">
            <div class="space-y-2">
              <label class="block text-sm font-medium text-wool-300">Provider</label>
              <Dropdown
                value={props.settings.llm?.provider || 'codex'}
                options={providerOptions}
                onChange={(value) =>
                  props.setSettings('llm', 'provider', value as 'codex' | 'openrouter')
                }
              />
            </div>

            <Show when={props.settings.llm?.provider === 'codex'}>
              <div class="space-y-4 rounded-none border border-pasture-600/50 bg-pasture-700/30 p-4">
                <div class="flex items-center justify-between gap-3">
                  <div class="text-sm">
                    <p class="text-wool-300">
                      Status:{' '}
                      <span classList={{ 'text-green-400': codexLoggedIn(), 'text-amber-400': !codexLoggedIn() }}>
                        {codexLoggedIn() ? 'Connected' : 'Not connected'}
                      </span>
                    </p>
                    <Show when={codexExpiresAt()}>
                      <p class="mt-1 text-xs text-wool-500">
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
                      <button
                        type="button"
                        class="p-1.5 text-wool-500 hover:text-wool-200 hover:bg-pasture-700 border border-pasture-700/40"
                        title="Copy code"
                        onClick={() => {
                          void navigator.clipboard.writeText(codexUserCode()).then(() => {
                            window.toast?.success('Code copied');
                          });
                        }}
                      >
                        <Icon name="clipboard" class="w-4 h-4" />
                      </button>
                      <button
                        type="button"
                        class="text-sm text-amber-500 hover:underline"
                        onClick={() => void invoke('open_external_url', { url: codexVerifyUrl() })}
                      >
                        Open verification page
                      </button>
                    </div>
                  </div>
                </Show>

                <Show when={codexLoginStatus()}>
                  <p class="text-xs text-wool-500">{codexLoginStatus()}</p>
                </Show>
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
                  <p class="text-xs text-wool-500">
                    {openrouterKeyConfigured() ? 'A key is already configured.' : 'No key configured yet.'}
                  </p>
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

        <Show when={props.section() === 'services'}>
          <div class="space-y-6">
            <div class="rounded-none border border-pasture-600 p-4 space-y-4">
              <div class="flex items-center justify-between gap-3">
                <div>
                  <h3 class="font-medium text-wool-200">GitHub</h3>
                  <p class="text-xs text-wool-500">Used for repository access and GitHub actions.</p>
                </div>
                <span
                  class="text-xs px-2 py-0.5 rounded-none"
                  classList={{
                    'bg-green-500/20 text-green-400': gitHubConfigured(),
                    'bg-amber-500/20 text-amber-400': !gitHubConfigured(),
                  }}
                >
                  {gitHubConfigured() ? 'Configured' : 'Not configured'}
                </span>
              </div>
              <div class="space-y-2">
                <label class="block text-sm text-wool-300">GitHub Token</label>
                <input
                  type="password"
                  class="input w-full"
                  value={gitHubToken()}
                  onInput={(e) => setGitHubToken(e.currentTarget.value)}
                  placeholder="ghp_xxxxxxxxxxxx"
                />
              </div>
            </div>

            <div class="rounded-none border border-pasture-600 p-4 space-y-4">
              <div class="flex items-center justify-between gap-3">
                <div>
                  <h3 class="font-medium text-wool-200">Tavily</h3>
                  <p class="text-xs text-wool-500">Used for web search and URL fetch tools.</p>
                </div>
                <span
                  class="text-xs px-2 py-0.5 rounded-none"
                  classList={{
                    'bg-green-500/20 text-green-400': tavilyConfigured(),
                    'bg-amber-500/20 text-amber-400': !tavilyConfigured(),
                  }}
                >
                  {tavilyConfigured() ? 'Configured' : 'Not configured'}
                </span>
              </div>
              <div class="space-y-2">
                <label class="block text-sm text-wool-300">Tavily API Key</label>
                <input
                  type="password"
                  class="input w-full"
                  value={tavilyApiKey()}
                  onInput={(e) => setTavilyApiKey(e.currentTarget.value)}
                  placeholder="tvly-..."
                />
              </div>
            </div>

            <div class="rounded-none border border-pasture-600 p-4 space-y-4">
              <div>
                <h3 class="font-medium text-wool-200">MCP Servers</h3>
                <p class="text-xs text-wool-500">
                  Advanced JSON config for external MCP servers imported into Shepherd and worker sessions.
                  Leave blank to disable MCP imports.
                </p>
              </div>
              <div class="space-y-2">
                <label class="block text-sm text-wool-300">Server Map</label>
                <textarea
                  class="input min-h-48 w-full font-mono text-xs leading-6"
                  value={props.settings.mcpServersText}
                  onInput={(e) => props.setSettings('mcpServersText', e.currentTarget.value)}
                  placeholder={`{\n  "filesystem": {\n    "transport": "stdio",\n    "command": "npx",\n    "args": ["-y", "@modelcontextprotocol/server-filesystem", "."]\n  }\n}`}
                />
                <p class="text-xs text-wool-500">
                  Server names become tool prefixes like <code>mcp__filesystem__read_file</code>.
                </p>
              </div>
            </div>
          </div>
        </Show>
      </div>
    </div>
  );
};
