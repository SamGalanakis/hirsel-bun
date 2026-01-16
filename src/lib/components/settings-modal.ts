/**
 * Settings modal Alpine component
 */

import {
  THEME_LIST,
  THEME_FAMILIES,
  THEME_FAMILY_LIST,
  THEMES,
  getTheme,
  setTheme,
  getPreferredDarkTheme,
  type ThemeId,
  type ThemeInfo,
  type ThemeFamily,
  type ThemeFamilyInfo,
} from '../theme';
import {
  startChatSession,
  sendChatMessage,
  stopChatSession,
  listenChatEvents,
} from '../api';
import type { ChatEvent } from '../types';

type AuthMethod = 'env' | 'apiKey' | 'oauth';
type RunnerType = 'ssh' | 'sprite';

interface AgentAuth {
  method: AuthMethod;
  apiKey: string | null;
  envVar: string | null;
}

interface AuthConfig {
  defaultMethod: AuthMethod;
  claude: AgentAuth | null;
  gemini: AgentAuth | null;
  codex: AgentAuth | null;
  goose: AgentAuth | null;
}

// Legacy remote config (SSH)
interface RemoteConfig {
  host: string;
  sshKey: string | null;
  sshPort: number;
  workBase: string;
  pythonPath: string;
  location: string | null;
}

// New runner configs
interface SshRunnerConfig {
  type: 'ssh';
  host: string;
  sshKey: string | null;
  sshPort: number;
  workBase: string;
  location: string | null;
}

interface SpriteRunnerConfig {
  type: 'sprite';
  apiToken: string | null;
  baseCheckpoint: string | null;
  autoDestroy: boolean;
  idleTimeoutSecs: number;
  apiUrl: string;
}

type RunnerConfig = { type: 'local' } | SshRunnerConfig | SpriteRunnerConfig;

interface Settings {
  agentCommand: string;
  evalTimeout: number;
  autoLearn: boolean;
  maxIterations: number | null;
  userMessagePause: string;
  humanInTheLoop: boolean;
  compactionEnabled: boolean;
  compactionThreshold: number | null;
  compactionKeepMessages: number;
  autoImprove: boolean;
  contextWarningThreshold: number;
  coordinatorPort: number;
  auth: AuthConfig;
  remotes: Record<string, RemoteConfig>;
  defaultRemote: string | null;
  // New runner config
  runners: Record<string, RunnerConfig>;
  defaultRunner: string | null;
  workerRunners: Record<string, string>;
}

// Default agent auth
const defaultAgentAuth = (): AgentAuth => ({
  method: 'env',
  apiKey: null,
  envVar: null,
});

// Default remote config (legacy)
const defaultRemoteConfig = (): RemoteConfig => ({
  host: '',
  sshKey: null,
  sshPort: 22,
  workBase: '/tmp/hirsel-remote',
  pythonPath: 'python3',
  location: null,
});

// Default SSH runner config
const defaultSshRunnerConfig = (): SshRunnerConfig => ({
  type: 'ssh',
  host: '',
  sshKey: null,
  sshPort: 22,
  workBase: '/tmp/hirsel-remote',
  location: null,
});

// Default Sprite runner config
const defaultSpriteRunnerConfig = (): SpriteRunnerConfig => ({
  type: 'sprite',
  apiToken: null,
  baseCheckpoint: null,
  autoDestroy: true,
  idleTimeoutSecs: 30,
  apiUrl: 'https://api.sprites.dev',
});

/**
 * Settings modal component
 */
export function settingsModal() {
  return {
    loading: false,
    saving: false,
    error: null as string | null,
    activeTab: 'general' as 'general' | 'auth' | 'runners',

    // Test connection state
    testing: false,
    testResult: '' as string,
    showTestDialog: false,
    _testSessionId: null as string | null,
    _testUnlisten: null as (() => void) | null,
    settings: {
      agentCommand: 'claude-code-acp',
      evalTimeout: 1800,
      autoLearn: true,
      maxIterations: null,
      userMessagePause: 'sender',
      humanInTheLoop: true,
      compactionEnabled: true,
      compactionThreshold: 10000,
      compactionKeepMessages: 40,
      autoImprove: true,
      contextWarningThreshold: 0.5,
      coordinatorPort: 19700,
      auth: {
        defaultMethod: 'env' as AuthMethod,
        claude: null,
        gemini: null,
        codex: null,
        goose: null,
      },
      remotes: {} as Record<string, RemoteConfig>,
      defaultRemote: null,
      runners: {} as Record<string, RunnerConfig>,
      defaultRunner: null,
      workerRunners: {} as Record<string, string>,
    } as Settings,

    // Editing state for remotes (legacy)
    editingRemote: null as string | null,
    newRemoteName: '',
    editRemoteData: defaultRemoteConfig(),

    // Editing state for runners
    editingRunner: null as string | null,
    newRunnerName: '',
    newRunnerType: 'ssh' as RunnerType,
    editRunnerData: defaultSshRunnerConfig() as SshRunnerConfig | SpriteRunnerConfig,

    // Cascading auth editor state
    selectedAuthProvider: '' as '' | 'claude' | 'gemini' | 'codex' | 'goose',
    selectedAuthMethod: 'env' as AuthMethod,
    authEnvVar: '',
    authApiKey: '',

    // Theme settings - stored as reactive properties for proper Alpine binding
    selectedTheme: getTheme() as ThemeId,
    themes: THEME_LIST as ThemeInfo[],
    themeFamilies: THEME_FAMILY_LIST as ThemeFamilyInfo[],

    // Stored reactive values for select bindings (initialized from current theme)
    familyValue: THEMES[getTheme()].family as ThemeFamily,
    darkVariantValue: (THEMES[getTheme()].isDark ? getTheme() : getPreferredDarkTheme(THEMES[getTheme()].family)) as ThemeId,
    isDarkMode: THEMES[getTheme()].isDark,

    // Initialize theme values from current state
    initThemeValues() {
      const theme = THEMES[this.selectedTheme];
      this.familyValue = theme.family;
      this.isDarkMode = theme.isDark;
      this.darkVariantValue = theme.isDark ? this.selectedTheme : getPreferredDarkTheme(theme.family);
    },

    // Get current family info
    get currentFamilyInfo(): ThemeFamilyInfo {
      return THEME_FAMILIES[this.familyValue as ThemeFamily] || THEME_FAMILIES.hirsel;
    },

    // Check if family has multiple dark variants
    get hasMultipleDarkVariants(): boolean {
      return this.currentFamilyInfo.darkThemes.length > 1;
    },

    // Get remote names as sorted array
    get remoteNames(): string[] {
      return Object.keys(this.settings.remotes).sort();
    },

    // Get runner names as sorted array
    get runnerNames(): string[] {
      return Object.keys(this.settings.runners).sort();
    },

    // Get runner type icon
    getRunnerIcon(type: string): string {
      switch (type) {
        case 'ssh': return 'server';
        case 'sprite': return 'cloud';
        default: return 'cpu';
      }
    },

    // Get runner type label
    getRunnerTypeLabel(type: string): string {
      switch (type) {
        case 'ssh': return 'SSH';
        case 'sprite': return 'Sprites';
        default: return type;
      }
    },

    // Switch theme family (preserves light/dark mode)
    switchFamily(familyId: ThemeFamily) {
      const familyInfo = THEME_FAMILIES[familyId];
      let newTheme: ThemeId;

      if (this.isDarkMode) {
        // Keep dark mode, use preferred dark variant
        newTheme = getPreferredDarkTheme(familyId);
      } else {
        // Keep light mode
        newTheme = familyInfo.lightTheme;
      }

      this.familyValue = familyId;
      this.applyTheme(newTheme);
    },

    // Toggle between light and dark mode
    toggleDarkMode() {
      const familyInfo = this.currentFamilyInfo;

      if (this.isDarkMode) {
        // Switch to light
        this.isDarkMode = false;
        this.applyTheme(familyInfo.lightTheme);
      } else {
        // Switch to dark (preferred variant)
        this.isDarkMode = true;
        const darkTheme = getPreferredDarkTheme(this.familyValue as ThemeFamily);
        this.darkVariantValue = darkTheme;
        this.applyTheme(darkTheme);
      }
    },

    // Select a specific dark variant (for Catppuccin)
    selectDarkVariant(themeId: ThemeId) {
      this.darkVariantValue = themeId;
      this.applyTheme(themeId);
    },

    // Apply theme immediately when selected (preview)
    applyTheme(themeId: ThemeId) {
      this.selectedTheme = themeId;
      setTheme(themeId);
    },

    // When provider changes, load existing config if any
    onAuthProviderChange() {
      if (!this.selectedAuthProvider) {
        this.selectedAuthMethod = 'env';
        this.authEnvVar = '';
        this.authApiKey = '';
        return;
      }

      const existing = this.settings.auth[this.selectedAuthProvider];
      if (existing) {
        this.selectedAuthMethod = existing.method;
        this.authEnvVar = existing.envVar || '';
        this.authApiKey = ''; // Don't show existing key
      } else {
        // Default to OAuth for Claude, env for others
        this.selectedAuthMethod = this.selectedAuthProvider === 'claude' ? 'oauth' : 'env';
        this.authEnvVar = '';
        this.authApiKey = '';
      }
    },

    // Find and select the first configured provider (called after settings load)
    initAuthProvider() {
      for (const provider of ['claude', 'gemini', 'codex', 'goose'] as const) {
        if (this.settings.auth[provider]) {
          this.selectedAuthProvider = provider;
          this.onAuthProviderChange();
          return;
        }
      }
      // No provider configured, leave empty
      this.selectedAuthProvider = '';
    },

    // Get existing masked API key for display
    getExistingAuthKey(provider: string): string | null {
      const auth = this.settings.auth[provider as keyof AuthConfig['claude']];
      if (auth && typeof auth === 'object' && 'apiKey' in auth && auth.apiKey) {
        return auth.apiKey; // Already masked from server
      }
      return null;
    },

    // Clear agent auth config
    clearAgentAuth(agent: string) {
      (this.settings.auth as Record<string, AgentAuth | null>)[agent] = null;
    },

    // Start adding a new remote
    startAddRemote() {
      this.editingRemote = '__new__';
      this.newRemoteName = '';
      this.editRemoteData = defaultRemoteConfig();
    },

    // Start editing an existing remote
    startEditRemote(name: string) {
      this.editingRemote = name;
      this.newRemoteName = name;
      this.editRemoteData = { ...this.settings.remotes[name] };
    },

    // Save remote config
    saveRemote() {
      const name = this.editingRemote === '__new__' ? this.newRemoteName.trim() : this.editingRemote;
      if (!name) {
        window.toast?.error('Remote name is required');
        return;
      }

      // Validate host
      if (!this.editRemoteData.host.trim()) {
        window.toast?.error('Host is required');
        return;
      }

      // If renaming, delete old entry
      if (this.editingRemote !== '__new__' && this.editingRemote !== name) {
        delete this.settings.remotes[this.editingRemote!];
      }

      this.settings.remotes[name] = { ...this.editRemoteData };
      this.editingRemote = null;
    },

    // Cancel editing remote
    cancelEditRemote() {
      this.editingRemote = null;
    },

    // Delete a remote
    deleteRemote(name: string) {
      delete this.settings.remotes[name];
      if (this.settings.defaultRemote === name) {
        this.settings.defaultRemote = null;
      }
    },

    // ==================== RUNNER METHODS ====================

    // Start adding a new runner
    startAddRunner() {
      this.editingRunner = '__new__';
      this.newRunnerName = '';
      this.newRunnerType = 'ssh';
      this.editRunnerData = defaultSshRunnerConfig();
    },

    // Change runner type when adding new
    onRunnerTypeChange() {
      if (this.newRunnerType === 'ssh') {
        this.editRunnerData = defaultSshRunnerConfig();
      } else if (this.newRunnerType === 'sprite') {
        this.editRunnerData = defaultSpriteRunnerConfig();
      }
    },

    // Start editing an existing runner
    startEditRunner(name: string) {
      const runner = this.settings.runners[name];
      if (!runner) return;

      this.editingRunner = name;
      this.newRunnerName = name;
      this.newRunnerType = runner.type as RunnerType;
      // Merge with defaults to ensure all required fields are present
      if (runner.type === 'sprite') {
        this.editRunnerData = { ...defaultSpriteRunnerConfig(), ...runner };
      } else if (runner.type === 'ssh') {
        this.editRunnerData = { ...defaultSshRunnerConfig(), ...runner };
      } else {
        this.editRunnerData = { ...runner } as SshRunnerConfig | SpriteRunnerConfig;
      }
    },

    // Save runner config
    saveRunner() {
      const name = this.editingRunner === '__new__' ? this.newRunnerName.trim() : this.editingRunner;
      if (!name) {
        window.toast?.error('Runner name is required');
        return;
      }

      // Validate based on type
      if (this.editRunnerData.type === 'ssh') {
        const ssh = this.editRunnerData as SshRunnerConfig;
        if (!ssh.host?.trim()) {
          window.toast?.error('SSH host is required');
          return;
        }
      } else if (this.editRunnerData.type === 'sprite') {
        const sprite = this.editRunnerData as SpriteRunnerConfig;
        if (!sprite.apiToken?.trim()) {
          window.toast?.error('API token is required');
          return;
        }
      }

      // If renaming, delete old entry
      if (this.editingRunner !== '__new__' && this.editingRunner !== name) {
        delete this.settings.runners[this.editingRunner!];
        // Update any worker assignments
        for (const [worker, runner] of Object.entries(this.settings.workerRunners)) {
          if (runner === this.editingRunner) {
            this.settings.workerRunners[worker] = name;
          }
        }
      }

      this.settings.runners[name] = { ...this.editRunnerData };
      this.editingRunner = null;
    },

    // Cancel editing runner
    cancelEditRunner() {
      this.editingRunner = null;
    },

    // Delete a runner
    deleteRunner(name: string) {
      delete this.settings.runners[name];
      if (this.settings.defaultRunner === name) {
        this.settings.defaultRunner = null;
      }
      // Remove any worker assignments using this runner
      for (const [worker, runner] of Object.entries(this.settings.workerRunners)) {
        if (runner === name) {
          delete this.settings.workerRunners[worker];
        }
      }
    },

    async loadSettings() {
      this.loading = true;
      this.error = null;

      // Load current theme and initialize reactive values
      this.selectedTheme = getTheme();
      this.initThemeValues();

      try {
        if (window.tauriInvoke) {
          const config = await window.tauriInvoke<{
            runsDir: string;
            agentCommand: string[];
            evalTimeout: number;
            autoLearn: boolean;
            maxIterations: number | null;
            userMessagePause: string;
            humanInTheLoop: boolean;
            compactionEnabled: boolean;
            compactionThreshold: number | null;
            compactionKeepMessages: number;
            compactionCooldownMinutes: number;
            autoImprove: boolean;
            contextWarningThreshold: number;
            coordinatorPort: number;
            auth: AuthConfig;
            remotes: Record<string, RemoteConfig>;
            defaultRemote: string | null;
            runners: Record<string, RunnerConfig>;
            defaultRunner: string | null;
            workerRunners: Record<string, string>;
          }>('get_config');

          this.settings = {
            agentCommand: config.agentCommand.join(' '),
            evalTimeout: config.evalTimeout,
            autoLearn: config.autoLearn,
            maxIterations: config.maxIterations,
            userMessagePause: config.userMessagePause,
            humanInTheLoop: config.humanInTheLoop,
            compactionEnabled: config.compactionEnabled,
            compactionThreshold: config.compactionThreshold,
            compactionKeepMessages: config.compactionKeepMessages,
            autoImprove: config.autoImprove,
            contextWarningThreshold: config.contextWarningThreshold,
            coordinatorPort: config.coordinatorPort,
            auth: config.auth,
            remotes: config.remotes,
            defaultRemote: config.defaultRemote,
            runners: config.runners || {},
            defaultRunner: config.defaultRunner || null,
            workerRunners: config.workerRunners || {},
          };

          // Auto-select configured provider if any
          this.initAuthProvider();
        }
      } catch (err) {
        const error = err as Error;
        console.error('[settingsModal] Error loading settings:', error);
        this.error = error.message || String(error);
      } finally {
        this.loading = false;
      }
    },

    async saveSettings() {
      this.saving = true;
      this.error = null;

      try {
        // Save theme (already applied, just ensure it's persisted)
        setTheme(this.selectedTheme);

        if (window.tauriInvoke) {
          // Parse agent command string into array
          const agentCommand = this.settings.agentCommand
            .split(/\s+/)
            .filter((s: string) => s.length > 0);

          // Build auth update - save the selected provider's config
          const authUpdate: Record<string, unknown> = {
            defaultMethod: 'env', // Not used anymore but keep for compatibility
          };

          // Only save the currently selected provider
          if (this.selectedAuthProvider) {
            authUpdate[this.selectedAuthProvider] = {
              method: this.selectedAuthMethod,
              apiKey: this.authApiKey || null,
              envVar: this.authEnvVar || null,
            };
          }

          await window.tauriInvoke('save_config', {
            updates: {
              agentCommand,
              evalTimeout: this.settings.evalTimeout,
              autoLearn: this.settings.autoLearn,
              maxIterations: this.settings.maxIterations || null,
              userMessagePause: this.settings.userMessagePause,
              humanInTheLoop: this.settings.humanInTheLoop,
              compactionEnabled: this.settings.compactionEnabled,
              compactionThreshold: this.settings.compactionThreshold || null,
              compactionKeepMessages: this.settings.compactionKeepMessages,
              autoImprove: this.settings.autoImprove,
              contextWarningThreshold: this.settings.contextWarningThreshold,
              coordinatorPort: this.settings.coordinatorPort,
              auth: authUpdate,
              remotes: this.settings.remotes,
              defaultRemote: this.settings.defaultRemote,
              runners: this.settings.runners,
              defaultRunner: this.settings.defaultRunner,
              workerRunners: this.settings.workerRunners,
            },
          });

          // Close modal on success - dispatch event to parent scope
          window.dispatchEvent(new CustomEvent('close-settings'));

          // Show success toast
          if (window.toast) {
            window.toast.success('Settings saved successfully');
          }
        }
      } catch (err) {
        const error = err as Error;
        console.error('[settingsModal] Error saving settings:', error);
        this.error = error.message || String(error);
      } finally {
        this.saving = false;
      }
    },

    // Get display text for auth method
    getAuthMethodLabel(method: AuthMethod): string {
      switch (method) {
        case 'env': return 'Environment Variable';
        case 'apiKey': return 'API Key';
        case 'oauth': return 'OAuth';
        default: return method;
      }
    },

    // Get default env var for an agent
    getDefaultEnvVar(agent: string): string {
      switch (agent) {
        case 'claude': return 'ANTHROPIC_API_KEY';
        case 'gemini': return 'GOOGLE_API_KEY';
        case 'codex': return 'OPENAI_API_KEY';
        case 'goose': return 'ANTHROPIC_API_KEY';
        default: return '';
      }
    },

    // Test agent connection
    async testConnection() {
      this.testing = true;
      this.testResult = '';
      this.showTestDialog = true;
      this._testSessionId = null;

      try {
        // Parse agent command
        const agentCommand = this.settings.agentCommand
          .split(/\s+/)
          .filter((s: string) => s.length > 0);

        if (agentCommand.length === 0) {
          throw new Error('No agent command configured');
        }

        // Set up event listener
        const self = this;
        this._testUnlisten = await listenChatEvents((event: ChatEvent) => {
          if (event.sessionId !== self._testSessionId) return;

          if (event.type === 'textDelta') {
            self.testResult += event.text;
          } else if (event.type === 'messageComplete') {
            self.testing = false;
            self.stopTestSession();
          } else if (event.type === 'error') {
            self.testResult = `Error: ${event.message}`;
            self.testing = false;
            self.stopTestSession();
          }
        });

        // Start session
        this._testSessionId = await startChatSession(agentCommand, {
          systemPrompt: 'You are a friendly assistant. Keep responses very brief.',
        });

        // Send test message
        await sendChatMessage(this._testSessionId, 'Hey there, all good?!');

        // Set a timeout in case no response
        setTimeout(() => {
          if (this.testing && !this.testResult) {
            this.testResult = 'Timeout - no response received';
            this.testing = false;
            this.stopTestSession();
          }
        }, 30000);

      } catch (err) {
        const error = err as Error;
        this.testResult = `Failed: ${error.message || String(error)}`;
        this.testing = false;
      }
    },

    stopTestSession() {
      if (this._testUnlisten) {
        this._testUnlisten();
        this._testUnlisten = null;
      }
      if (this._testSessionId) {
        stopChatSession(this._testSessionId).catch(() => {});
        this._testSessionId = null;
      }
    },

    closeTestDialog() {
      this.showTestDialog = false;
      this.stopTestSession();
    },
  };
}
