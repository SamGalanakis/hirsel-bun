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

type AuthMethod = 'env' | 'apiKey' | 'oauth';

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

interface RemoteConfig {
  host: string;
  sshKey: string | null;
  sshPort: number;
  workBase: string;
  pythonPath: string;
  location: string | null;
}

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
}

// Default agent auth
const defaultAgentAuth = (): AgentAuth => ({
  method: 'env',
  apiKey: null,
  envVar: null,
});

// Default remote config
const defaultRemoteConfig = (): RemoteConfig => ({
  host: '',
  sshKey: null,
  sshPort: 22,
  workBase: '/tmp/hirsel-remote',
  pythonPath: 'python3',
  location: null,
});

/**
 * Settings modal component
 */
export function settingsModal() {
  return {
    loading: false,
    saving: false,
    error: null as string | null,
    activeTab: 'general' as 'general' | 'auth' | 'remotes',
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
    } as Settings,

    // Editing state for remotes
    editingRemote: null as string | null,
    newRemoteName: '',
    editRemoteData: defaultRemoteConfig(),

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
  };
}
