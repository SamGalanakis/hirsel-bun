/**
 * App-wide context for theme, shortcuts, version info
 */
import { invoke } from '../lib/invoke';
import { emit, on } from '../lib/events';
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
    const cleanup = on('theme-changed', (detail) => {
      const { themeId, theme } = detail as { themeId: ThemeId; theme: ThemeInfo };
      setCurrentTheme(themeId);
      setIsDark(theme.isDark);
    });

    onCleanup(cleanup);
  });

  // Listen for shortcuts changes
  createEffect(() => {
    const cleanup = on('shortcuts-changed', () => {
      setShortcuts(getShortcuts());
    });

    onCleanup(cleanup);
  });

  // Listen for close-settings event
  createEffect(() => {
    const cleanup = on('close-settings', () => setShowSettings(false));
    onCleanup(cleanup);
  });

  createEffect(() => {
    const cleanup = on('open-backend-settings', () => setShowSettings(true));
    onCleanup(cleanup);
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
        emit('toggle-activity-fullscreen');
        break;
      case 'toggle-ai':
        setAiChatOpen((c) => !c);
        break;
      case 'toggle-theme':
        handleToggleTheme();
        break;
      case 'show-help':
        // Help is now in Settings > Shortcuts
        setShowSettings(true);
        break;
      case 'close-panel':
        setShowSettings(false);
        setNotificationsOpen(false);
        setAiChatOpen(false);
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
