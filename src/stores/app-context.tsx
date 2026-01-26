/**
 * App-wide context for theme, shortcuts, version info
 */
import { invoke } from '@tauri-apps/api/core';
import {
  type ParentComponent,
  createContext,
  createEffect,
  createSignal,
  onCleanup,
  useContext,
} from 'solid-js';
import {
  type ShortcutAction,
  type ShortcutConfig,
  findMatchingAction,
  formatBinding,
  getShortcuts,
} from '../lib/shortcuts';
import {
  type ThemeId,
  type ThemeInfo,
  THEMES,
  getTheme,
  isDarkTheme,
  setTheme,
  toggleTheme as themeToggle,
  isSystemTheme as checkSystemTheme,
  setSystemTheme,
  setExplicitTheme,
  applySystemTheme,
} from '../lib/theme';
import type { VersionInfo } from '../lib/types';

interface AppContextValue {
  // Theme
  currentTheme: () => ThemeId;
  isDark: () => boolean;
  themeInfo: () => ThemeInfo;
  isSystemTheme: () => boolean;
  setTheme: (themeId: ThemeId) => void;
  setThemeToSystem: () => void;
  toggleTheme: () => void;

  // Version
  versionInfo: () => VersionInfo | null;

  // Shortcuts
  shortcuts: () => ShortcutConfig[];
  formatBinding: typeof formatBinding;

  // UI State
  aiChatOpen: () => boolean;
  setAiChatOpen: (open: boolean) => void;
  toggleAiChat: () => void;

  showHelp: () => boolean;
  setShowHelp: (show: boolean) => void;

  showSettings: () => boolean;
  setShowSettings: (show: boolean) => void;

  notificationsOpen: () => boolean;
  setNotificationsOpen: (open: boolean) => void;
  toggleNotifications: () => void;

  sidebarCollapsed: () => boolean;
  setSidebarCollapsed: (collapsed: boolean) => void;
  toggleSidebar: () => void;

  // Actions
  executeAction: (action: ShortcutAction) => void;
}

const AppContext = createContext<AppContextValue>();

export const AppProvider: ParentComponent = (props) => {
  // Theme state
  const [currentTheme, setCurrentTheme] = createSignal<ThemeId>(getTheme());
  const [isDark, setIsDark] = createSignal(isDarkTheme());
  const [isSystemTheme, setIsSystemTheme] = createSignal(checkSystemTheme());

  // Version info
  const [versionInfo, setVersionInfo] = createSignal<VersionInfo | null>(null);

  // Shortcuts
  const [shortcuts, setShortcuts] = createSignal<ShortcutConfig[]>(getShortcuts());

  // UI State
  const [aiChatOpen, setAiChatOpen] = createSignal(false);
  const [showHelp, setShowHelp] = createSignal(false);
  const [showSettings, setShowSettings] = createSignal(false);
  const [notificationsOpen, setNotificationsOpen] = createSignal(false);
  const [sidebarCollapsed, setSidebarCollapsed] = createSignal(false);

  // Load version info
  createEffect(() => {
    invoke<VersionInfo>('get_version')
      .then(setVersionInfo)
      .catch((err) => console.error('Failed to load version info:', err));
  });

  // Listen for theme changes
  createEffect(() => {
    const handler = ((e: CustomEvent<{ themeId: ThemeId; theme: ThemeInfo }>) => {
      setCurrentTheme(e.detail.themeId);
      setIsDark(e.detail.theme.isDark);
    }) as EventListener;

    window.addEventListener('theme-changed', handler);
    onCleanup(() => window.removeEventListener('theme-changed', handler));
  });

  // Listen for shortcuts changes
  createEffect(() => {
    const handler = () => {
      setShortcuts(getShortcuts());
    };

    window.addEventListener('shortcuts-changed', handler);
    onCleanup(() => window.removeEventListener('shortcuts-changed', handler));
  });

  // Listen for close-settings event
  createEffect(() => {
    const handler = () => setShowSettings(false);
    window.addEventListener('close-settings', handler);
    onCleanup(() => window.removeEventListener('close-settings', handler));
  });

  // Keyboard shortcuts handler
  createEffect(() => {
    const handler = (e: KeyboardEvent) => {
      // Ignore if typing in input
      const target = e.target as HTMLElement;
      if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') return;

      const action = findMatchingAction(e, shortcuts());
      if (action) {
        executeAction(action);
      }
    };

    document.addEventListener('keydown', handler);
    onCleanup(() => document.removeEventListener('keydown', handler));
  });

  const handleSetTheme = (themeId: ThemeId) => {
    setExplicitTheme(themeId);
    setCurrentTheme(themeId);
    setIsDark(THEMES[themeId].isDark);
    setIsSystemTheme(false);
  };

  const handleSetThemeToSystem = () => {
    setSystemTheme();
    setIsSystemTheme(true);
    const themeId = applySystemTheme();
    setCurrentTheme(themeId);
    setIsDark(THEMES[themeId].isDark);
  };

  const handleToggleTheme = () => {
    const newTheme = themeToggle();
    setCurrentTheme(newTheme);
    setIsDark(THEMES[newTheme].isDark);
    setIsSystemTheme(false);
  };

  const executeAction = (action: ShortcutAction) => {
    switch (action) {
      case 'toggle-sidebar':
        setSidebarCollapsed((c) => !c);
        break;
      case 'fullscreen':
        window.dispatchEvent(new CustomEvent('toggle-activity-fullscreen'));
        break;
      case 'toggle-ai':
        setAiChatOpen((c) => !c);
        break;
      case 'toggle-theme':
        handleToggleTheme();
        break;
      case 'show-help':
        setShowHelp(true);
        break;
      case 'close-panel':
        setShowHelp(false);
        setShowSettings(false);
        setNotificationsOpen(false);
        setAiChatOpen(false);
        break;
      // Navigation and run-specific actions are handled by SelectionContext
      case 'navigate-up':
      case 'navigate-down':
      case 'select-run':
      case 'attach':
      case 'pause':
      case 'resume':
      case 'switch-chat':
      case 'focus-message':
      case 'sheep-game':
        window.dispatchEvent(new CustomEvent('shortcut-action', { detail: action }));
        break;
    }
  };

  const value: AppContextValue = {
    currentTheme,
    isDark,
    themeInfo: () => THEMES[currentTheme()],
    isSystemTheme,
    setTheme: handleSetTheme,
    setThemeToSystem: handleSetThemeToSystem,
    toggleTheme: handleToggleTheme,
    versionInfo,
    shortcuts,
    formatBinding,
    aiChatOpen,
    setAiChatOpen,
    toggleAiChat: () => setAiChatOpen((c) => !c),
    showHelp,
    setShowHelp,
    showSettings,
    setShowSettings,
    notificationsOpen,
    setNotificationsOpen,
    toggleNotifications: () => setNotificationsOpen((c) => !c),
    sidebarCollapsed,
    setSidebarCollapsed,
    toggleSidebar: () => setSidebarCollapsed((c) => !c),
    executeAction,
  };

  return <AppContext.Provider value={value}>{props.children}</AppContext.Provider>;
};

export function useApp(): AppContextValue {
  const context = useContext(AppContext);
  if (!context) {
    throw new Error('useApp must be used within an AppProvider');
  }
  return context;
}
