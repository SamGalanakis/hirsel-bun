/**
 * Settings modal Alpine component
 */

import {
  THEME_LIST,
  getTheme,
  setTheme,
  type ThemeId,
  type ThemeInfo,
} from '../../theme';
import {
  startChatSession,
  sendChatMessage,
  stopChatSession,
  listenChatEvents,
} from '../../api';
import {
  getShortcuts,
  saveShortcuts,
  resetShortcuts as resetShortcutsToDefaults,
  findConflict,
  formatBinding,
  bindingFromEvent,
  getCategoryLabel,
  type ShortcutConfig,
  type ShortcutAction,
  type ShortcutCategory,
} from '../../shortcuts';
import type { ChatEvent } from '../../types';

// Import from local modules
import type {
  AuthMethod,
  HostType,
  AgentAuth,
  AuthConfig,
  SshHostConfig,
  SpriteHostConfig,
  ContainerConfig,
  HostConfig,
  RunnerConfig,
  Settings,
  OrchestratorProfile,
  OrchestratorAccessType,
  TailscaleAccess,
  AppSection,
  ProfileTab,
  ActiveSection,
  ProfileSettingsCache,
  RemoteConfig,
  TailscaleInfo,
  RunnerHealthStatus,
} from './types';
import {
  defaultAgentAuth,
  defaultSshHostConfig,
  defaultSpriteHostConfig,
  defaultContainerConfig,
  defaultLocalRunnerConfig,
  defaultSshRunnerConfig,
  defaultSpriteRunnerConfig,
  defaultRemoteProfile,
  defaultTailscaleAccess,
  defaultGitConfig,
  defaultSettings,
  defaultNavigationState,
  getAuthMethodLabel,
  getDefaultEnvVar,
  getHostIcon,
  getHostTypeLabel,
  getAccessTypeLabel,
} from './defaults';

// Re-export types and utilities
export * from './types';
export * from './defaults';

declare const window: Window & {
  tauriInvoke?: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  toast?: {
    success: (message: string, title?: string) => void;
    error: (message: string, title?: string) => void;
  };
  lucide?: {
    createIcons: (options?: { inTemplates?: boolean; nodes?: Element[] }) => void;
  };
  confirmDialog?: {
    show: (options: { title: string; message: string; confirmText?: string; cancelText?: string; danger?: boolean }) => Promise<boolean>;
    delete: (itemName: string, itemType?: string) => Promise<boolean>;
  };
};

/**
 * Settings modal component
 */
export function settingsModal() {
  return {
    loading: false,
    saving: false,
    error: null as string | null,

    // New navigation state
    activeSection: 'profile' as ActiveSection,
    appSection: 'theme' as AppSection,
    selectedProfile: 'local' as string | null,
    profileTab: 'defaults' as ProfileTab,
    loadingProfile: false,

    // Cache for profile-specific settings (for remote profiles)
    profileSettingsCache: {} as Record<string, ProfileSettingsCache>,

    // Test connection state
    testing: false,
    testResult: '' as string,
    showTestDialog: false,
    _testSessionId: null as string | null,
    _testUnlisten: null as (() => void) | null,
    settings: defaultSettings(),

    // Editing state for runners (Host + Container model)
    editingRunner: null as string | null,
    newRunnerName: '',
    newHostType: 'ssh' as HostType,
    editHostData: defaultSshHostConfig() as HostConfig,
    editContainerEnabled: false,
    editContainerImage: '',

    // Editing state for orchestrator profiles
    editingProfile: null as string | null,
    newProfileName: '',
    editProfileData: defaultRemoteProfile() as OrchestratorProfile,
    profileHealth: {} as Record<string, { status: 'checking' | 'online' | 'offline'; version?: string; lastChecked?: number; error?: string }>,
    _healthPollInterval: null as ReturnType<typeof setInterval> | null,

    // Tailscale info for "This Machine" feature
    tailscaleInfo: null as TailscaleInfo | null,

    // Runner health status polling
    runnerHealth: {} as Record<string, RunnerHealthStatus>,
    _runnerHealthPollInterval: null as ReturnType<typeof setInterval> | null,

    // Git provider state
    gitHubToken: '',

    // Cascading auth editor state
    selectedAuthProvider: '' as '' | 'claude' | 'gemini' | 'codex' | 'goose',
    selectedAuthMethod: 'env' as AuthMethod,
    authEnvVar: '',
    authApiKey: '',

    // Theme settings
    selectedTheme: getTheme() as ThemeId,
    themes: THEME_LIST as ThemeInfo[],

    // Keyboard shortcuts state
    shortcuts: [] as ShortcutConfig[],
    rebindingAction: null as ShortcutAction | null,
    shortcutConflict: '' as string,
    _rebindHandler: null as ((e: KeyboardEvent) => void) | null,

    // Get runner names as sorted array (profile-scoped)
    get runnerNames(): string[] {
      return Object.keys(this.getCurrentProfileRunners()).sort();
    },

    // ==================== PROFILE-SCOPED RUNNER ACCESS ====================

    // Get runners for current profile
    getCurrentProfileRunners(): Record<string, RunnerConfig> {
      if (this.isLocalProfile() || !this.selectedProfile) {
        return this.settings.runners;
      }

      const cached = this.profileSettingsCache[this.selectedProfile];
      return cached?.settings?.runners || {};
    },

    // Get a specific runner by name (for current profile)
    getRunner(name: string): RunnerConfig | undefined {
      return this.getCurrentProfileRunners()[name];
    },

    // Get default runner for current profile
    getCurrentDefaultRunner(): string | null {
      if (this.isLocalProfile() || !this.selectedProfile) {
        return this.settings.defaultRunner;
      }

      const cached = this.profileSettingsCache[this.selectedProfile];
      return cached?.settings?.defaultRunner ?? null;
    },

    // Set default runner for current profile
    setCurrentDefaultRunner(name: string | null) {
      if (this.isLocalProfile() || !this.selectedProfile) {
        this.settings.defaultRunner = name;
      } else {
        const cached = this.profileSettingsCache[this.selectedProfile];
        if (cached) {
          if (!cached.settings) cached.settings = {};
          cached.settings.defaultRunner = name;
          cached.dirty = true;
        }
      }
    },

    // Get worker runners map for current profile
    getCurrentWorkerRunners(): Record<string, string> {
      if (this.isLocalProfile() || !this.selectedProfile) {
        return this.settings.workerRunners;
      }

      const cached = this.profileSettingsCache[this.selectedProfile];
      return cached?.settings?.workerRunners || {};
    },

    // Set a runner config for current profile
    setCurrentProfileRunner(name: string, config: RunnerConfig) {
      if (this.isLocalProfile() || !this.selectedProfile) {
        this.settings.runners[name] = config;
      } else {
        const cached = this.profileSettingsCache[this.selectedProfile];
        if (cached) {
          if (!cached.settings) cached.settings = {};
          if (!cached.settings.runners) cached.settings.runners = {};
          cached.settings.runners[name] = config;
          cached.dirty = true;
        }
      }
    },

    // Delete a runner from current profile
    deleteCurrentProfileRunner(name: string) {
      if (this.isLocalProfile() || !this.selectedProfile) {
        delete this.settings.runners[name];
        if (this.settings.defaultRunner === name) {
          this.settings.defaultRunner = null;
        }
        for (const [worker, runner] of Object.entries(this.settings.workerRunners)) {
          if (runner === name) {
            delete this.settings.workerRunners[worker];
          }
        }
      } else {
        const cached = this.profileSettingsCache[this.selectedProfile];
        if (cached?.settings) {
          if (cached.settings.runners) {
            delete cached.settings.runners[name];
          }
          if (cached.settings.defaultRunner === name) {
            cached.settings.defaultRunner = null;
          }
          if (cached.settings.workerRunners) {
            for (const [worker, runner] of Object.entries(cached.settings.workerRunners)) {
              if (runner === name) {
                delete cached.settings.workerRunners[worker];
              }
            }
          }
          cached.dirty = true;
        }
      }
    },

    // Get all profile names as sorted array
    get profileNames(): string[] {
      return Object.keys(this.settings.profiles).sort();
    },

    // Get only remote profile names (exclude 'local')
    get remoteProfileNames(): string[] {
      return Object.keys(this.settings.profiles)
        .filter(name => name !== 'local' && this.settings.profiles[name]?.mode === 'remote')
        .sort();
    },

    // Get all profile names including local
    get allProfileNames(): string[] {
      return ['local', ...this.remoteProfileNames];
    },

    // ==================== NAVIGATION HELPERS ====================

    // Check if current profile is local
    isLocalProfile(): boolean {
      return this.selectedProfile === 'local';
    },

    // Check if current profile is remote
    isRemoteProfile(): boolean {
      return this.selectedProfile !== null &&
        this.selectedProfile !== 'local' &&
        this.settings.profiles[this.selectedProfile]?.mode === 'remote';
    },

    // Check if connection tab should be shown (only for remote profiles)
    canShowConnectionTab(): boolean {
      return this.isRemoteProfile();
    },

    // ==================== CONTEXTUAL RUNNER NAMING ====================

    // Get the display name for the local/built-in runner
    getLocalRunnerName(): string {
      return this.isRemoteProfile() ? 'orchestrator' : 'local';
    },

    // Get the badge text for the local/built-in runner
    getLocalRunnerBadge(): string {
      return this.isRemoteProfile() ? 'Server' : 'Built-in';
    },

    // Get the description for the local/built-in runner
    getLocalRunnerDescription(): string {
      return this.isRemoteProfile()
        ? 'Run workers as subprocesses on the orchestrator server'
        : 'Run workers as subprocesses on this machine';
    },

    // ==================== TAILSCALE INFO ====================

    // Load Tailscale info for this machine
    async loadTailscaleInfo() {
      try {
        this.tailscaleInfo = await window.tauriInvoke?.('get_tailscale_info');
      } catch {
        this.tailscaleInfo = null;
      }
    },

    // Check if we can show the "This Machine" quick-add option
    canShowThisMachineOption(): boolean {
      return this.tailscaleInfo?.connected === true && this.tailscaleInfo?.dns_name != null;
    },

    // Use this machine as an SSH runner (pre-populate fields)
    useThisMachineAsRunner() {
      if (!this.tailscaleInfo?.dns_name) return;

      this.newRunnerType = 'ssh';
      this.editRunnerData = {
        ...defaultSshRunnerConfig(),
        host: this.tailscaleInfo.dns_name.replace(/\.$/, ''),
        sshPort: 22,
        workBase: '/tmp/hirsel-remote',
      };
      this.newRunnerName = this.tailscaleInfo.hostname || 'this-machine';
    },

    // ==================== RUNNER HEALTH POLLING ====================

    // Check health of a single SSH runner
    async checkRunnerHealth(name: string) {
      const runner = this.getRunner(name);
      if (!runner || runner.type !== 'ssh') return;

      const ssh = runner as SshRunnerConfig;

      // Set to checking state
      this.runnerHealth[name] = { status: 'checking' };

      try {
        const result = await window.tauriInvoke?.('check_ssh_runner', {
          host: ssh.host,
          port: ssh.sshPort || 22,
          sshKey: ssh.sshKey || null,
        }) as { reachable: boolean; error: string | null; latency_ms: number | null } | undefined;

        if (result) {
          this.runnerHealth[name] = {
            status: result.reachable ? 'online' : 'offline',
            latencyMs: result.latency_ms ?? undefined,
            error: result.error ?? undefined,
          };
        }
      } catch (err) {
        this.runnerHealth[name] = {
          status: 'offline',
          error: String(err),
        };
      }
    },

    // Poll health for all SSH runners (for current profile)
    async pollRunnerHealth() {
      const runners = this.getCurrentProfileRunners();
      const sshRunnerNames = Object.keys(runners).filter(
        name => runners[name]?.type === 'ssh'
      );
      await Promise.all(sshRunnerNames.map(name => this.checkRunnerHealth(name)));
    },

    // Start runner health polling (called when entering Runners tab)
    startRunnerHealthPolling() {
      this.stopRunnerHealthPolling();
      this.pollRunnerHealth();
      this._runnerHealthPollInterval = setInterval(() => this.pollRunnerHealth(), 10000);
    },

    // Stop runner health polling (called when leaving Runners tab)
    stopRunnerHealthPolling() {
      if (this._runnerHealthPollInterval) {
        clearInterval(this._runnerHealthPollInterval);
        this._runnerHealthPollInterval = null;
      }
    },

    // Get tooltip text for runner health status
    getRunnerHealthTooltip(name: string): string {
      const health = this.runnerHealth[name];
      if (!health) return '';

      const parts: string[] = [];
      if (health.status === 'online') {
        parts.push('Connected');
        if (health.latencyMs !== undefined) {
          parts.push(`Latency: ${health.latencyMs}ms`);
        }
      } else if (health.status === 'offline') {
        parts.push('Offline');
        if (health.error) {
          parts.push(`Error: ${health.error}`);
        }
      } else {
        parts.push('Checking...');
      }
      return parts.join('\n');
    },

    // Navigate to app section
    navigateToApp(section: AppSection) {
      this.activeSection = 'app';
      this.appSection = section;
      this.selectedProfile = null;
      this.stopHealthPolling();
      this.stopRunnerHealthPolling();
    },

    // Navigate to profile
    async navigateToProfile(profileName: string) {
      this.activeSection = 'profile';
      this.selectedProfile = profileName;

      // Clear runner health cache when switching profiles (different runners)
      this.runnerHealth = {};

      // Start health polling for remote profiles
      if (profileName !== 'local') {
        this.startHealthPolling();
      } else {
        this.stopHealthPolling();
      }

      // For remote profiles, set connection as default tab if not already on a valid tab
      if (profileName !== 'local') {
        // Load remote settings if not cached
        await this.loadProfileSettings(profileName);
      } else {
        // Local profile - ensure we're not on connection tab
        if (this.profileTab === 'connection') {
          this.profileTab = 'defaults';
        }
      }

      // If on runners tab, restart runner health polling for new profile's runners
      if (this.profileTab === 'runners') {
        this.startRunnerHealthPolling();
      }
    },

    // Set profile tab
    setProfileTab(tab: ProfileTab) {
      // Don't allow connection tab for local profile
      if (tab === 'connection' && this.isLocalProfile()) {
        return;
      }

      // Stop runner health polling if leaving runners tab
      if (this.profileTab === 'runners' && tab !== 'runners') {
        this.stopRunnerHealthPolling();
      }

      this.profileTab = tab;

      // Start runner health polling if entering runners tab
      if (tab === 'runners') {
        this.startRunnerHealthPolling();
        this.loadTailscaleInfo();
      }
    },

    // Helper function wrappers
    getHostIcon,
    getHostTypeLabel,
    getAuthMethodLabel,
    getDefaultEnvVar,
    getAccessTypeLabel,

    // Apply theme immediately when selected
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

    // ==================== RUNNER METHODS ====================

    // Start adding a new runner
    startAddRunner() {
      this.editingRunner = '__new__';
      this.newRunnerName = '';
      this.newHostType = 'ssh';
      this.editHostData = defaultSshHostConfig();
      this.editContainerEnabled = false;
      this.editContainerImage = '';
    },

    // Change host type when adding new
    onHostTypeChange() {
      if (this.newHostType === 'local') {
        this.editHostData = { type: 'local' };
      } else if (this.newHostType === 'client') {
        this.editHostData = { type: 'client' };
      } else if (this.newHostType === 'ssh') {
        this.editHostData = defaultSshHostConfig();
      } else if (this.newHostType === 'sprite') {
        this.editHostData = defaultSpriteHostConfig();
        // Sprites don't support containers
        this.editContainerEnabled = false;
        this.editContainerImage = '';
      }
    },

    // Start editing an existing runner
    startEditRunner(name: string) {
      const runner = this.getRunner(name);
      if (!runner) return;

      this.editingRunner = name;
      this.newRunnerName = name;
      this.newHostType = runner.host.type as HostType;

      // Merge with defaults to ensure all required fields are present
      if (runner.host.type === 'sprite') {
        this.editHostData = { ...defaultSpriteHostConfig(), ...runner.host };
      } else if (runner.host.type === 'ssh') {
        this.editHostData = { ...defaultSshHostConfig(), ...runner.host };
      } else if (runner.host.type === 'local') {
        this.editHostData = { type: 'local' };
      } else if (runner.host.type === 'client') {
        this.editHostData = { type: 'client' };
      } else {
        this.editHostData = { ...runner.host };
      }

      // Load container settings
      this.editContainerEnabled = !!runner.container;
      this.editContainerImage = runner.container?.image || '';
    },

    // Save runner config and persist to disk
    async saveRunner() {
      const name = this.editingRunner === '__new__' ? this.newRunnerName.trim() : this.editingRunner;
      if (!name) {
        window.toast?.error('Runner name is required');
        return;
      }

      // Validate based on host type
      if (this.editHostData.type === 'ssh') {
        const ssh = this.editHostData as SshHostConfig;
        if (!ssh.address?.trim()) {
          window.toast?.error('SSH address is required');
          return;
        }
      } else if (this.editHostData.type === 'sprite') {
        const sprite = this.editHostData as SpriteHostConfig;
        if (!sprite.apiToken?.trim()) {
          window.toast?.error('API token is required');
          return;
        }
      }

      // Validate container if enabled
      if (this.editContainerEnabled && !this.editContainerImage?.trim()) {
        window.toast?.error('Container image is required when container is enabled');
        return;
      }

      // Sprites don't support containers
      if (this.editHostData.type === 'sprite' && this.editContainerEnabled) {
        window.toast?.error('Sprites do not support Docker containers');
        return;
      }

      // If renaming, delete old entry and update worker assignments
      if (this.editingRunner !== '__new__' && this.editingRunner !== name) {
        this.deleteCurrentProfileRunner(this.editingRunner!);
        // Update any worker assignments to use new name
        const workerRunners = this.getCurrentWorkerRunners();
        for (const [worker, runner] of Object.entries(workerRunners)) {
          if (runner === this.editingRunner) {
            workerRunners[worker] = name;
          }
        }
      }

      // Build the runner config from host + optional container
      const runnerConfig: RunnerConfig = {
        host: { ...this.editHostData },
        container: this.editContainerEnabled ? { image: this.editContainerImage } : undefined,
      };

      // Save the runner to current profile
      this.setCurrentProfileRunner(name, runnerConfig);
      this.editingRunner = null;

      // Persist to disk immediately (without closing modal)
      try {
        await this.persistSettings();
        window.toast?.success('Runner saved');
      } catch (err) {
        const error = err as Error;
        console.error('[settingsModal] Error saving runner:', error);
        window.toast?.error(`Failed to save: ${error.message || String(error)}`);
      }

      // Re-initialize icons for the new runner card
      setTimeout(() => {
        if (window.lucide) {
          window.lucide.createIcons({ inTemplates: true });
        }
      }, 50);
    },

    // Cancel editing runner
    cancelEditRunner() {
      this.editingRunner = null;
    },

    // Delete a runner (from current profile)
    deleteRunner(name: string) {
      this.deleteCurrentProfileRunner(name);
    },

    // ==================== PROFILE METHODS ====================

    // Start adding a new profile
    startAddProfile() {
      this.editingProfile = '__new__';
      this.newProfileName = '';
      this.editProfileData = defaultRemoteProfile();
    },

    // Start editing an existing profile
    startEditProfile(name: string) {
      const profile = this.settings.profiles[name];
      if (!profile) return;

      this.editingProfile = name;
      this.newProfileName = name;
      // Deep clone to avoid modifying original
      this.editProfileData = {
        ...profile,
        access: { ...profile.access },
      };
    },

    // Handle access type change when editing profile
    onAccessTypeChange(newType: OrchestratorAccessType) {
      if (newType === 'tailscale') {
        this.editProfileData.access = defaultTailscaleAccess();
      } else {
        this.editProfileData.access = { type: 'direct' };
      }
    },

    // Save profile config and persist to disk
    async saveProfile() {
      const name = this.editingProfile === '__new__' ? this.newProfileName.trim() : this.editingProfile;
      if (!name) {
        window.toast?.error('Profile name is required');
        return;
      }

      if (name === 'local') {
        window.toast?.error('Cannot modify the local profile');
        return;
      }

      // Validate remote profile has required fields
      if (!this.editProfileData.url?.trim()) {
        window.toast?.error('Server URL is required');
        return;
      }
      if (!this.editProfileData.apiKey?.trim()) {
        window.toast?.error('API key is required');
        return;
      }

      // Validate Tailscale OAuth fields if access type is tailscale
      if (this.editProfileData.access.type === 'tailscale') {
        const access = this.editProfileData.access as TailscaleAccess;
        if (!access.oauth_client_id?.trim()) {
          window.toast?.error('Tailscale OAuth Client ID is required');
          return;
        }
        if (!access.oauth_client_secret?.trim()) {
          window.toast?.error('Tailscale OAuth Client Secret is required');
          return;
        }
      }

      // Ensure mode is set to remote
      this.editProfileData.mode = 'remote';

      this.settings.profiles[name] = {
        ...this.editProfileData,
        access: { ...this.editProfileData.access },
      };
      const savedName = name;
      this.editingProfile = null;

      // Persist to disk immediately (without closing modal)
      try {
        await this.persistSettings();
        window.toast?.success('Profile saved');
        // Force immediate health recheck for this profile
        delete this.profileHealth[savedName]; // Clear cached status
        this.checkProfileHealth(savedName);
      } catch (err) {
        const error = err as Error;
        console.error('[settingsModal] Error saving profile:', error);
        window.toast?.error(`Failed to save: ${error.message || String(error)}`);
      }

      // Re-initialize icons
      setTimeout(() => {
        if (window.lucide) {
          window.lucide.createIcons({ inTemplates: true });
        }
      }, 50);
    },

    // Cancel editing profile
    cancelEditProfile() {
      this.editingProfile = null;
    },

    // Delete a profile
    deleteProfile(name: string) {
      if (name === 'local') {
        window.toast?.error('Cannot delete the local profile');
        return;
      }
      delete this.settings.profiles[name];
      if (this.settings.defaultProfile === name) {
        this.settings.defaultProfile = 'local';
      }
      // If this was the selected profile, navigate to local
      if (this.selectedProfile === name) {
        this.navigateToProfile('local');
      }
    },

    // ==================== REMOTE PROFILE SETTINGS ====================

    // Load settings from a remote profile
    async loadProfileSettings(profileName: string) {
      const profile = this.settings.profiles[profileName];
      if (!profile || profile.mode !== 'remote') return;

      // Check cache - valid for 30 seconds
      const cached = this.profileSettingsCache[profileName];
      if (cached && Date.now() - cached.loadedAt < 30000) {
        return;
      }

      this.loadingProfile = true;

      try {
        const baseUrl = profile.url?.replace(/\/$/, '');
        if (!baseUrl) {
          throw new Error('No server URL configured');
        }

        // Get API key from credential store
        let apiKey: string | null = null;
        if (window.tauriInvoke) {
          apiKey = await window.tauriInvoke<string | null>('get_credential', {
            keyType: `profile_${profileName}_api_key`,
          });
        }

        if (!apiKey) {
          throw new Error('No API key configured');
        }

        const response = await fetch(`${baseUrl}/api/config`, {
          method: 'GET',
          headers: {
            'Accept': 'application/json',
            'Authorization': `Bearer ${apiKey}`,
          },
        });

        if (!response.ok) {
          if (response.status === 401) {
            throw new Error('Invalid API key');
          }
          throw new Error(`HTTP ${response.status}`);
        }

        const remoteConfig = await response.json() as RemoteConfig;

        // Cache the settings
        this.profileSettingsCache[profileName] = {
          settings: {
            agentCommand: remoteConfig.agentCommand.join(' '),
            evalTimeout: remoteConfig.evalTimeout,
            autoLearn: remoteConfig.autoLearn,
            maxIterations: remoteConfig.maxIterations,
            userMessagePause: remoteConfig.userMessagePause,
            humanInTheLoop: remoteConfig.humanInTheLoop,
            compactionEnabled: remoteConfig.compactionEnabled,
            compactionThreshold: remoteConfig.compactionThreshold,
            compactionKeepMessages: remoteConfig.compactionKeepMessages,
            autoImprove: remoteConfig.autoImprove,
            contextWarningThreshold: remoteConfig.contextWarningThreshold,
            auth: remoteConfig.auth,
            runners: remoteConfig.runners || {},
            defaultRunner: remoteConfig.defaultRunner,
            workerRunners: remoteConfig.workerRunners || {},
            git: remoteConfig.git || defaultGitConfig(),
          },
          loadedAt: Date.now(),
          dirty: false,
        };
      } catch (err) {
        const error = err as Error;
        console.error(`[settingsModal] Error loading remote config for ${profileName}:`, error);
        window.toast?.error(`Failed to load remote settings: ${error.message}`);
      } finally {
        this.loadingProfile = false;
      }
    },

    // Get effective settings for current profile (local or cached remote)
    getEffectiveSettings(): Partial<Settings> {
      if (this.isLocalProfile() || !this.selectedProfile) {
        return this.settings;
      }

      const cached = this.profileSettingsCache[this.selectedProfile];
      if (cached) {
        return cached.settings;
      }

      // Fallback to local settings if no cache
      return this.settings;
    },

    // Save settings to remote server
    async saveToRemoteServer(profileName: string): Promise<boolean> {
      const profile = this.settings.profiles[profileName];
      if (!profile || profile.mode !== 'remote') return false;

      const baseUrl = profile.url?.replace(/\/$/, '');
      if (!baseUrl) {
        throw new Error('No server URL configured');
      }

      // Get API key from credential store
      let apiKey: string | null = null;
      if (window.tauriInvoke) {
        apiKey = await window.tauriInvoke<string | null>('get_credential', {
          keyType: `profile_${profileName}_api_key`,
        });
      }

      if (!apiKey) {
        throw new Error('No API key configured');
      }

      const cached = this.profileSettingsCache[profileName];
      if (!cached) {
        throw new Error('No cached settings to save');
      }

      const settings = cached.settings;

      // Parse agent command
      const agentCommand = (settings.agentCommand || '')
        .split(/\s+/)
        .filter((s: string) => s.length > 0);

      // Save to remote server
      const response = await fetch(`${baseUrl}/api/config`, {
        method: 'PATCH',
        headers: {
          'Accept': 'application/json',
          'Content-Type': 'application/json',
          'Authorization': `Bearer ${apiKey}`,
        },
        body: JSON.stringify({
          agentCommand,
          evalTimeout: settings.evalTimeout,
          autoLearn: settings.autoLearn,
          maxIterations: settings.maxIterations || null,
          userMessagePause: settings.userMessagePause,
          humanInTheLoop: settings.humanInTheLoop,
          compactionEnabled: settings.compactionEnabled,
          compactionThreshold: settings.compactionThreshold || null,
          compactionKeepMessages: settings.compactionKeepMessages,
          autoImprove: settings.autoImprove,
          contextWarningThreshold: settings.contextWarningThreshold,
          runners: settings.runners,
          defaultRunner: settings.defaultRunner,
          workerRunners: settings.workerRunners,
          git: {
            defaultProvider: settings.git?.defaultProvider || null,
          },
        }),
      });

      if (!response.ok) {
        if (response.status === 401) {
          throw new Error('Invalid API key');
        }
        const text = await response.text();
        throw new Error(`HTTP ${response.status}: ${text}`);
      }

      // Mark cache as clean
      cached.dirty = false;
      cached.loadedAt = Date.now();

      return true;
    },

    // Delete all runs for a remote profile
    async deleteRemoteRuns(profileName: string): Promise<void> {
      const profile = this.settings.profiles[profileName];
      if (!profile || profile.mode !== 'remote') {
        throw new Error('Not a remote profile');
      }

      const baseUrl = profile.url?.replace(/\/$/, '');
      if (!baseUrl) {
        throw new Error('No server URL configured');
      }

      let apiKey: string | null = null;
      if (window.tauriInvoke) {
        apiKey = await window.tauriInvoke<string | null>('get_credential', {
          keyType: `profile_${profileName}_api_key`,
        });
      }

      if (!apiKey) {
        throw new Error('No API key configured');
      }

      const response = await fetch(`${baseUrl}/api/runs`, {
        method: 'DELETE',
        headers: {
          'Authorization': `Bearer ${apiKey}`,
        },
      });

      if (!response.ok) {
        const text = await response.text();
        throw new Error(`HTTP ${response.status}: ${text}`);
      }
    },

    // Check health of a single remote profile
    async checkProfileHealth(name: string) {
      const profile = this.settings.profiles[name];
      if (!profile?.url) return;

      // Only show 'checking' on first check, otherwise keep previous status
      const prev = this.profileHealth[name];
      if (!prev) {
        this.profileHealth[name] = { status: 'checking' };
      }

      const baseUrl = profile.url.replace(/\/$/, '');

      try {
        // First check health (no auth required)
        const controller = new AbortController();
        const timeout = setTimeout(() => controller.abort(), 5000);

        const healthResponse = await fetch(`${baseUrl}/health`, {
          method: 'GET',
          headers: { 'Accept': 'application/json' },
          signal: controller.signal,
        });
        clearTimeout(timeout);

        if (!healthResponse.ok) {
          this.profileHealth[name] = { status: 'offline', lastChecked: Date.now(), error: `HTTP ${healthResponse.status}` };
          return;
        }

        const healthData = await healthResponse.json();

        // Try to verify auth if we can get the API key
        try {
          let apiKey: string | null = null;
          if (window.tauriInvoke) {
            apiKey = await window.tauriInvoke<string | null>('get_credential', {
              keyType: `profile_${name}_api_key`,
            });
          }

          if (!apiKey) {
            this.profileHealth[name] = { status: 'online', version: healthData.version, lastChecked: Date.now(), error: 'No API key' };
            return;
          }

          // Check auth with /api/runs endpoint
          const authController = new AbortController();
          const authTimeout = setTimeout(() => authController.abort(), 5000);

          const authResponse = await fetch(`${baseUrl}/api/runs`, {
            method: 'GET',
            headers: {
              'Accept': 'application/json',
              'Authorization': `Bearer ${apiKey}`,
            },
            signal: authController.signal,
          });
          clearTimeout(authTimeout);

          if (authResponse.status === 401) {
            this.profileHealth[name] = { status: 'online', version: healthData.version, lastChecked: Date.now(), error: 'Invalid API key' };
          } else if (authResponse.ok) {
            this.profileHealth[name] = { status: 'online', version: healthData.version, lastChecked: Date.now() };
          } else {
            this.profileHealth[name] = { status: 'online', version: healthData.version, lastChecked: Date.now(), error: `HTTP ${authResponse.status}` };
          }
        } catch (authErr) {
          // Auth check failed but server is reachable - show online without auth verification
          this.profileHealth[name] = { status: 'online', version: healthData.version, lastChecked: Date.now(), error: 'Auth check failed' };
        }
      } catch (err) {
        const error = err as Error;
        this.profileHealth[name] = { status: 'offline', lastChecked: Date.now(), error: error.message || 'Connection failed' };
      }
    },

    // Get tooltip text for health status
    getHealthTooltip(name: string): string {
      const health = this.profileHealth[name];
      if (!health) return '';

      const parts: string[] = [];
      if (health.version) parts.push(`Version: ${health.version}`);
      if (health.lastChecked) {
        const ago = Math.round((Date.now() - health.lastChecked) / 1000);
        parts.push(`Checked: ${ago}s ago`);
      }
      if (health.error) parts.push(`Error: ${health.error}`);
      return parts.join('\n');
    },

    // Poll health for all remote profiles
    async pollProfileHealth() {
      const remoteNames = Object.keys(this.settings.profiles)
        .filter(name => name !== 'local' && this.settings.profiles[name]?.mode === 'remote');

      await Promise.all(remoteNames.map(name => this.checkProfileHealth(name)));
    },

    // Start health polling
    startHealthPolling() {
      this.stopHealthPolling();
      this.pollProfileHealth(); // Initial check
      this._healthPollInterval = setInterval(() => this.pollProfileHealth(), 5000);
    },

    // Stop health polling
    stopHealthPolling() {
      if (this._healthPollInterval) {
        clearInterval(this._healthPollInterval);
        this._healthPollInterval = null;
      }
    },

    // ==================== SHORTCUT METHODS ====================

    // Load shortcuts from storage
    loadShortcuts() {
      this.shortcuts = getShortcuts();
    },

    // Get shortcuts filtered by category
    getShortcutsByCategory(category: ShortcutCategory): ShortcutConfig[] {
      return this.shortcuts.filter(s => s.category === category);
    },

    // Get category display label
    getCategoryLabel,

    // Format binding for display
    formatBinding,

    // Start rebinding a shortcut
    startRebind(action: ShortcutAction) {
      this.rebindingAction = action;
      this.shortcutConflict = '';

      // Add keydown listener for capturing the new binding
      this._rebindHandler = (e: KeyboardEvent) => {
        e.preventDefault();
        e.stopPropagation();

        // Cancel on Escape (don't bind Escape as a rebind key unless shift is held)
        if (e.key === 'Escape' && !e.shiftKey) {
          this.cancelRebind();
          return;
        }

        // Ignore modifier-only keypresses
        if (['Control', 'Alt', 'Shift', 'Meta'].includes(e.key)) {
          return;
        }

        // Create binding from event
        const newBinding = bindingFromEvent(e);

        // Check for conflicts
        const conflict = findConflict(action, newBinding, this.shortcuts);
        if (conflict) {
          this.shortcutConflict = `"${formatBinding(newBinding)}" is already used for "${conflict}"`;
          return;
        }

        // Apply the new binding
        const shortcut = this.shortcuts.find(s => s.action === action);
        if (shortcut) {
          shortcut.binding = newBinding;
          saveShortcuts(this.shortcuts);
        }

        this.cancelRebind();
        this.shortcutConflict = '';
      };

      document.addEventListener('keydown', this._rebindHandler, { capture: true });
    },

    // Cancel rebinding
    cancelRebind() {
      this.rebindingAction = null;
      if (this._rebindHandler) {
        document.removeEventListener('keydown', this._rebindHandler, { capture: true });
        this._rebindHandler = null;
      }
    },

    // Reset all shortcuts to defaults
    resetAllShortcuts() {
      resetShortcutsToDefaults();
      this.shortcuts = getShortcuts();
      this.shortcutConflict = '';
      if (window.toast) {
        window.toast.success('Shortcuts reset to defaults');
      }
    },

    async loadSettings() {
      this.loading = true;
      this.error = null;

      // Load current theme
      this.selectedTheme = getTheme();

      // Load keyboard shortcuts
      this.loadShortcuts();

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
            autoImprove: boolean;
            contextWarningThreshold: number;
            coordinatorPort: number;
            auth: AuthConfig;
            runners: Record<string, RunnerConfig>;
            defaultRunner: string | null;
            workerRunners: Record<string, string>;
            profiles: Record<string, OrchestratorProfile>;
            defaultProfile: string;
            git: { defaultProvider: string | null; configuredProviders: string[] };
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
            runners: config.runners || {},
            defaultRunner: config.defaultRunner || null,
            workerRunners: config.workerRunners || {},
            profiles: config.profiles || { local: { mode: 'local', url: null, apiKey: null, access: { type: 'direct' } } },
            defaultProfile: config.defaultProfile || 'local',
            git: config.git || defaultGitConfig(),
          };

          // Load masked API keys from credential store for profiles and ensure access field
          for (const [name, profile] of Object.entries(this.settings.profiles)) {
            // Ensure access field exists
            if (!profile.access) {
              profile.access = { type: 'direct' };
            }
            if (profile.mode === 'remote') {
              const maskedKey = await window.tauriInvoke<string | null>('get_credential_masked', {
                keyType: `profile_${name}_api_key`,
              });
              if (maskedKey) {
                profile.apiKey = maskedKey;
              }
            }
          }

          // Auto-select configured provider if any
          this.initAuthProvider();

          // Select the default profile
          this.selectedProfile = this.settings.defaultProfile || 'local';
          this.activeSection = 'profile';

          // Set appropriate default tab
          if (this.selectedProfile !== 'local') {
            this.profileTab = 'defaults';
            // Start health polling for remote profiles
            this.startHealthPolling();
          } else {
            this.profileTab = 'defaults';
          }
        }
      } catch (err) {
        const error = err as Error;
        console.error('[settingsModal] Error loading settings:', error);
        this.error = error.message || String(error);
      } finally {
        this.loading = false;
        // Re-initialize Lucide icons after Alpine renders the dynamic content
        // Use setTimeout to ensure x-for loops have fully processed
        setTimeout(() => {
          if (window.lucide) {
            window.lucide.createIcons({ inTemplates: true });
          }
        }, 50);
      }
    },

    // Internal function to persist settings without UI side effects
    async persistSettings(): Promise<boolean> {
      if (!window.tauriInvoke) return false;

      // ==================== ALWAYS LOCAL ====================
      // Save theme (already applied, just ensure it's persisted)
      setTheme(this.selectedTheme);

      // Shortcuts are saved in real-time by startRebind, no need to save here

      // ==================== PROFILE CONNECTION INFO (always local) ====================
      // Store orchestrator profile API keys in credential store
      for (const [name, profile] of Object.entries(this.settings.profiles)) {
        if (profile.mode === 'remote' && profile.apiKey && !profile.apiKey.includes('...')) {
          // Only store if it's a new/changed key (not masked)
          await window.tauriInvoke('store_credential', {
            keyType: `profile_${name}_api_key`,
            value: profile.apiKey,
          });
        }
      }

      // Prepare profiles without API keys for config file
      const profilesForConfig: Record<string, OrchestratorProfile> = {};
      for (const [name, profile] of Object.entries(this.settings.profiles)) {
        profilesForConfig[name] = {
          mode: profile.mode,
          url: profile.url,
          apiKey: null, // Stored in credential store
          access: profile.access || { type: 'direct' },
        };
      }

      // ==================== PROFILE-SCOPED SETTINGS ====================
      // These go to either local config or remote API based on current profile

      // Build auth update - save the selected provider's config
      const authUpdate: Record<string, unknown> = {
        defaultMethod: 'env',
      };

      if (this.selectedAuthProvider) {
        // Store API key in encrypted credential store if provided
        if (this.authApiKey && this.selectedAuthMethod === 'apiKey') {
          await window.tauriInvoke('store_credential', {
            keyType: `agent_${this.selectedAuthProvider}_api_key`,
            value: this.authApiKey,
          });
        }

        authUpdate[this.selectedAuthProvider] = {
          method: this.selectedAuthMethod,
          apiKey: null, // Stored in credential store
          envVar: this.authEnvVar || null,
        };
      }

      // Store GitHub token in credential store if provided
      if (this.gitHubToken) {
        await window.tauriInvoke('store_credential', {
          keyType: 'git_github_token',
          value: this.gitHubToken,
        });
      }

      // Parse agent command
      const agentCommand = this.settings.agentCommand
        .split(/\s+/)
        .filter((s: string) => s.length > 0);

      // Always save local config (includes profile definitions and local profile settings)
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
          runners: this.settings.runners,
          defaultRunner: this.settings.defaultRunner,
          workerRunners: this.settings.workerRunners,
          profiles: profilesForConfig,
          defaultProfile: this.settings.defaultProfile,
          git: {
            defaultProvider: this.settings.git?.defaultProvider || null,
          },
        },
      });

      // If viewing a remote profile with cached changes, save to remote server too
      if (this.selectedProfile && this.isRemoteProfile()) {
        const cached = this.profileSettingsCache[this.selectedProfile];
        if (cached && cached.dirty) {
          try {
            await this.saveToRemoteServer(this.selectedProfile);
          } catch (err) {
            const error = err as Error;
            console.error('[settingsModal] Error saving to remote:', error);
            window.toast?.error(`Failed to save to remote: ${error.message}`);
            // Don't return false - local save succeeded
          }
        }
      }

      return true;
    },

    async saveSettings() {
      this.saving = true;
      this.error = null;

      try {
        await this.persistSettings();

        // Close modal on success - dispatch event to parent scope
        window.dispatchEvent(new CustomEvent('close-settings'));

        // Show success toast
        if (window.toast) {
          window.toast.success('Settings saved successfully');
        }
      } catch (err) {
        const error = err as Error;
        console.error('[settingsModal] Error saving settings:', error);
        this.error = error.message || String(error);
      } finally {
        this.saving = false;
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

    async deleteAllRuns() {
      const profileName = this.selectedProfile || 'local';
      const isRemote = this.isRemoteProfile();
      const locationText = isRemote
        ? `on the remote server (${this.settings.profiles[profileName]?.url})`
        : 'on this machine';

      const confirmed = await window.confirmDialog?.show({
        title: 'Delete All Runs',
        message: `Are you sure you want to delete all runs ${locationText}? This cannot be undone.`,
        confirmText: 'Delete All',
        danger: true,
      }) ?? confirm(`Delete all runs ${locationText}? This cannot be undone.`);

      if (!confirmed) return;

      try {
        if (isRemote) {
          await this.deleteRemoteRuns(profileName);
        } else {
          await window.tauriInvoke?.('delete_all_runs');
        }
        window.toast?.success('All runs deleted');
        // Refresh the runs list
        window.dispatchEvent(new CustomEvent('runs-changed'));
      } catch (err) {
        const error = err as Error;
        window.toast?.error(error.message || 'Failed to delete runs');
      }
    },
  };
}
