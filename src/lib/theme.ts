/**
 * Hirsel Theme System
 *
 * Theme families with light/dark mode support:
 * - Hirsel (default): Dark and Light variants
 * - Catppuccin: Mocha/Macchiato/Frappe (dark) + Latte (light)
 */

// =============================================================================
// Theme Definitions
// =============================================================================

export type ThemeId =
  | 'hirsel-dark'
  | 'hirsel-light'
  | 'catppuccin-mocha'
  | 'catppuccin-macchiato'
  | 'catppuccin-frappe'
  | 'catppuccin-latte';

export type ThemeFamily = 'hirsel' | 'catppuccin';

export interface ThemeInfo {
  id: ThemeId;
  name: string;
  description: string;
  isDark: boolean;
  family: ThemeFamily;
  /** The light counterpart for dark themes, or dark counterpart for light themes */
  pairedTheme: ThemeId;
  /** Color swatches to preview the theme [background, accent, text] */
  swatches: [string, string, string];
}

export const THEMES: Record<ThemeId, ThemeInfo> = {
  'hirsel-dark': {
    id: 'hirsel-dark',
    name: 'Hirsel Dark',
    description: 'Pasture at Dusk - warm, earthy dark theme',
    isDark: true,
    family: 'hirsel',
    pairedTheme: 'hirsel-light',
    swatches: ['#1a1a1a', '#d4a574', '#e8e4df'], // pasture-900, amber-500, wool-100
  },
  'hirsel-light': {
    id: 'hirsel-light',
    name: 'Hirsel Light',
    description: 'Pasture at Dawn - warm, earthy light theme',
    isDark: false,
    family: 'hirsel',
    pairedTheme: 'hirsel-dark',
    swatches: ['#f5f3f0', '#b8895c', '#292724'], // wool-50, amber-600, wool-900
  },
  'catppuccin-mocha': {
    id: 'catppuccin-mocha',
    name: 'Catppuccin Mocha',
    description: 'The darkest Catppuccin - cozy and color-rich',
    isDark: true,
    family: 'catppuccin',
    pairedTheme: 'catppuccin-latte',
    swatches: ['#1e1e2e', '#cba6f7', '#cdd6f4'], // base, mauve, text
  },
  'catppuccin-macchiato': {
    id: 'catppuccin-macchiato',
    name: 'Catppuccin Macchiato',
    description: 'Medium contrast - gentle and soothing',
    isDark: true,
    family: 'catppuccin',
    pairedTheme: 'catppuccin-latte',
    swatches: ['#24273a', '#c6a0f6', '#cad3f5'], // base, mauve, text
  },
  'catppuccin-frappe': {
    id: 'catppuccin-frappe',
    name: 'Catppuccin Frappé',
    description: 'Muted and subdued - less vibrant alternative',
    isDark: true,
    family: 'catppuccin',
    pairedTheme: 'catppuccin-latte',
    swatches: ['#303446', '#ca9ee6', '#c6d0f5'], // base, mauve, text
  },
  'catppuccin-latte': {
    id: 'catppuccin-latte',
    name: 'Catppuccin Latte',
    description: 'Light pastel theme - bright and harmonious',
    isDark: false,
    family: 'catppuccin',
    pairedTheme: 'catppuccin-mocha',
    swatches: ['#eff1f5', '#8839ef', '#4c4f69'], // base, mauve, text
  },
};

export const THEME_LIST: ThemeInfo[] = Object.values(THEMES);

export const DEFAULT_THEME: ThemeId = 'hirsel-dark';

const STORAGE_KEY = 'hirsel-theme';
const DARK_PREFERENCE_KEY = 'hirsel-theme-dark-preference';

// =============================================================================
// Theme Families for UI
// =============================================================================

export interface ThemeFamilyInfo {
  id: ThemeFamily;
  name: string;
  description: string;
  darkThemes: ThemeId[];
  lightTheme: ThemeId;
}

export const THEME_FAMILIES: Record<ThemeFamily, ThemeFamilyInfo> = {
  hirsel: {
    id: 'hirsel',
    name: 'Hirsel',
    description: 'Warm, earthy tones inspired by pastoral landscapes',
    darkThemes: ['hirsel-dark'],
    lightTheme: 'hirsel-light',
  },
  catppuccin: {
    id: 'catppuccin',
    name: 'Catppuccin',
    description: 'Soothing pastel colors for the high-spirited',
    darkThemes: ['catppuccin-mocha', 'catppuccin-macchiato', 'catppuccin-frappe'],
    lightTheme: 'catppuccin-latte',
  },
};

export const THEME_FAMILY_LIST: ThemeFamilyInfo[] = Object.values(THEME_FAMILIES);

// =============================================================================
// Theme Application
// =============================================================================

/**
 * Apply a theme by ID
 */
export function setTheme(themeId: ThemeId): void {
  const theme = THEMES[themeId];
  if (!theme) {
    console.warn(`Unknown theme: ${themeId}, falling back to default`);
    setTheme(DEFAULT_THEME);
    return;
  }

  // Update data-theme attribute
  document.documentElement.setAttribute('data-theme', themeId);

  // Handle light class for backwards compatibility
  if (theme.isDark) {
    document.documentElement.classList.remove('light');
    // Remember this as the preferred dark theme for this family
    saveDarkPreference(theme.family, themeId);
  } else {
    document.documentElement.classList.add('light');
  }

  // Persist to localStorage
  localStorage.setItem(STORAGE_KEY, themeId);

  // Dispatch event for components that need to react
  window.dispatchEvent(new CustomEvent('theme-changed', { detail: { themeId, theme } }));
}

/**
 * Get the current theme ID from localStorage or return default based on system preference
 */
export function getTheme(): ThemeId {
  const stored = localStorage.getItem(STORAGE_KEY);

  // Check if it's a valid theme ID (user has explicitly chosen)
  if (stored && stored in THEMES) {
    return stored as ThemeId;
  }

  // Handle legacy 'dark'/'light' values
  if (stored === 'dark') {
    return 'hirsel-dark';
  }
  if (stored === 'light') {
    return 'hirsel-light';
  }

  // No stored preference - use system preference with Hirsel theme
  const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
  return prefersDark ? 'hirsel-dark' : 'hirsel-light';
}

/**
 * Get the current theme info
 */
export function getThemeInfo(): ThemeInfo {
  return THEMES[getTheme()];
}

/**
 * Check if current theme is dark
 */
export function isDarkTheme(): boolean {
  return getThemeInfo().isDark;
}

/**
 * Initialize theme on app startup
 */
export function initTheme(): void {
  const themeId = getTheme();
  setTheme(themeId);
}

// =============================================================================
// Dark Theme Preference (per family)
// =============================================================================

interface DarkPreferences {
  hirsel: ThemeId;
  catppuccin: ThemeId;
}

function getDarkPreferences(): DarkPreferences {
  try {
    const stored = localStorage.getItem(DARK_PREFERENCE_KEY);
    if (stored) {
      return JSON.parse(stored);
    }
  } catch {
    // ignore parse errors
  }
  return {
    hirsel: 'hirsel-dark',
    catppuccin: 'catppuccin-mocha',
  };
}

function saveDarkPreference(family: ThemeFamily, themeId: ThemeId): void {
  const prefs = getDarkPreferences();
  prefs[family] = themeId;
  localStorage.setItem(DARK_PREFERENCE_KEY, JSON.stringify(prefs));
}

/**
 * Get the preferred dark theme for a family
 */
export function getPreferredDarkTheme(family: ThemeFamily): ThemeId {
  return getDarkPreferences()[family];
}

// =============================================================================
// Theme Toggling
// =============================================================================

/**
 * Toggle between light and dark within the same theme family.
 * Remembers your preferred dark variant for each family.
 */
export function toggleTheme(): ThemeId {
  const current = getTheme();
  const currentTheme = THEMES[current];
  const family = currentTheme.family;

  let next: ThemeId;

  if (currentTheme.isDark) {
    // Switch to light variant
    next = THEME_FAMILIES[family].lightTheme;
  } else {
    // Switch to preferred dark variant for this family
    next = getPreferredDarkTheme(family);
  }

  setTheme(next);
  return next;
}

/**
 * Switch to a different theme family, preserving light/dark mode
 */
export function switchFamily(family: ThemeFamily): ThemeId {
  const currentTheme = getThemeInfo();
  const familyInfo = THEME_FAMILIES[family];

  let next: ThemeId;

  if (currentTheme.isDark) {
    // Keep dark mode, use preferred dark theme for new family
    next = getPreferredDarkTheme(family);
  } else {
    // Keep light mode
    next = familyInfo.lightTheme;
  }

  setTheme(next);
  return next;
}

/**
 * Cycle to the next theme (all themes)
 */
export function cycleTheme(): ThemeId {
  const current = getTheme();
  const themeIds = Object.keys(THEMES) as ThemeId[];
  const currentIndex = themeIds.indexOf(current);
  const nextIndex = (currentIndex + 1) % themeIds.length;
  const next = themeIds[nextIndex];
  setTheme(next);
  return next;
}

// =============================================================================
// Color Palette Constants (for TypeScript access)
// =============================================================================

export const pasture = {
  900: '#1a1a1a',
  800: '#242424',
  700: '#2d2d2d',
  600: '#333333',
  500: '#404040',
  400: '#525252',
} as const;

export const wool = {
  50: '#f5f3f0',
  100: '#e8e4df',
  200: '#d4cfc7',
  300: '#b5b0a8',
  400: '#9d9890',
  500: '#8a8580',
  600: '#706b66',
  700: '#5a5550',
  800: '#3d3a37',
  900: '#292724',
} as const;

export const amber = {
  300: '#f0d4ac',
  400: '#e8c19a',
  500: '#d4a574',
  600: '#b8895c',
  700: '#9a7048',
  800: '#7a5838',
} as const;

export const status = {
  sage: '#7d9970',
  sageLight: '#9ab88c',
  sageDark: '#5c7852',
  golden: '#c9a227',
  goldenLight: '#dbb94a',
  goldenDark: '#a6841c',
  terra: '#c45c4a',
  terraLight: '#d4786a',
  terraDark: '#a34538',
} as const;

export const fonts = {
  sans: "system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif",
  mono: "'JetBrains Mono', 'Fira Code', 'SF Mono', Monaco, 'Cascadia Code', 'Roboto Mono', Menlo, monospace",
} as const;

export const radius = {
  card: '8px',
  button: '6px',
  pill: '9999px',
} as const;

export const transitions = {
  fast: '150ms ease-out',
  normal: '200ms ease-out',
  slow: '300ms ease-out',
} as const;

// =============================================================================
// Status Types and Classes
// =============================================================================

export type RunStatus = 'idle' | 'working' | 'paused' | 'waiting' | 'done' | 'timed_out' | 'error';
export type TaskStatus = 'todo' | 'doing' | 'done' | 'blocked';
export type WorkerStatus =
  | 'idle'
  | 'working'
  | 'waiting'
  | 'awaiting'
  | 'paused'
  | 'done'
  | 'error';

export const statusClasses: Record<RunStatus | TaskStatus | WorkerStatus, string> = {
  idle: 'status-idle',
  working: 'status-working',
  paused: 'status-idle',
  waiting: 'status-waiting',
  awaiting: 'status-waiting',
  done: 'status-done',
  timed_out: 'status-error',
  error: 'status-error',
  todo: 'status-idle',
  doing: 'status-working',
  blocked: 'status-blocked',
};

export const statusBadgeClasses: Record<TaskStatus, string> = {
  todo: 'badge',
  doing: 'badge badge-warning',
  done: 'badge badge-success',
  blocked: 'badge',
};

// Legacy type alias for backwards compatibility
export type ThemeMode = 'dark' | 'light';
