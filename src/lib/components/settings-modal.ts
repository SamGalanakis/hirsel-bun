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

interface Settings {
  agentCommand: string;
  evalTimeout: number;
  autoLearn: boolean;
  maxIterations: number | null;
  userMessagePause: string;
  humanInTheLoop: boolean;
  compactionThreshold: number | null;
  compactionKeepMessages: number;
  contextWarningThreshold: number;
  coordinatorPort: number;
}

/**
 * Settings modal component
 */
export function settingsModal() {
  return {
    loading: false,
    saving: false,
    error: null as string | null,
    settings: {
      agentCommand: 'claude-code-acp',
      evalTimeout: 1800,
      autoLearn: true,
      maxIterations: null,
      userMessagePause: 'sender',
      humanInTheLoop: true,
      compactionThreshold: 10000,
      compactionKeepMessages: 40,
      contextWarningThreshold: 0.5,
      coordinatorPort: 19700,
    } as Settings,

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
            compactionThreshold: number | null;
            compactionKeepMessages: number;
            contextWarningThreshold: number;
            coordinatorPort: number;
          }>('get_config');

          this.settings = {
            agentCommand: config.agentCommand.join(' '),
            evalTimeout: config.evalTimeout,
            autoLearn: config.autoLearn,
            maxIterations: config.maxIterations,
            userMessagePause: config.userMessagePause,
            humanInTheLoop: config.humanInTheLoop,
            compactionThreshold: config.compactionThreshold,
            compactionKeepMessages: config.compactionKeepMessages,
            contextWarningThreshold: config.contextWarningThreshold,
            coordinatorPort: config.coordinatorPort,
          };
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

          await window.tauriInvoke('save_config', {
            updates: {
              agentCommand,
              evalTimeout: this.settings.evalTimeout,
              autoLearn: this.settings.autoLearn,
              maxIterations: this.settings.maxIterations || null,
              userMessagePause: this.settings.userMessagePause,
              humanInTheLoop: this.settings.humanInTheLoop,
              compactionThreshold: this.settings.compactionThreshold || null,
              compactionKeepMessages: this.settings.compactionKeepMessages,
              contextWarningThreshold: this.settings.contextWarningThreshold,
              coordinatorPort: this.settings.coordinatorPort,
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
  };
}
