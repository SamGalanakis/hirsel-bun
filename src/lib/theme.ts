/**
 * Hirsel Theme Constants
 *
 * Color palette based on the "Pasture at Dusk" theme.
 * The name "hirsel" is a Scottish term for a flock of sheep.
 *
 * These constants match the CSS variables defined in styles/main.css
 * and can be used in TypeScript code when programmatic access is needed.
 */

// =============================================================================
// Color Palette: Backgrounds (pasture at dusk)
// =============================================================================

export const pasture = {
  900: '#1a1a1a', // darkest - main bg
  800: '#242424', // panels
  700: '#2d2d2d', // hover states
  600: '#333333', // borders
  500: '#404040', // subtle dividers
  400: '#525252', // disabled text bg
} as const;

// =============================================================================
// Color Palette: Text (warm off-white, like wool)
// =============================================================================

export const wool = {
  50: '#f5f3f0',  // brightest
  100: '#e8e4df', // primary text
  200: '#d4cfc7', // secondary bright
  300: '#b5b0a8', // secondary
  400: '#9d9890', // tertiary
  500: '#8a8580', // muted
  600: '#706b66', // dimmer
  700: '#5a5550', // dim
  800: '#3d3a37', // very dim
  900: '#292724', // almost invisible
} as const;

// =============================================================================
// Color Palette: Accent (shepherd's lantern - amber/gold)
// =============================================================================

export const amber = {
  300: '#f0d4ac', // lightest
  400: '#e8c19a', // bright
  500: '#d4a574', // primary accent
  600: '#b8895c', // hover
  700: '#9a7048', // pressed
  800: '#7a5838', // dark accent
} as const;

// =============================================================================
// Color Palette: Status Colors
// =============================================================================

export const status = {
  sage: '#7d9970',       // success/done (healthy pasture)
  sageLight: '#9ab88c',  // success hover
  sageDark: '#5c7852',   // success pressed

  golden: '#c9a227',      // warning/waiting (attention needed)
  goldenLight: '#dbb94a', // warning hover
  goldenDark: '#a6841c',  // warning pressed

  terra: '#c45c4a',       // error (problem)
  terraLight: '#d4786a',  // error hover
  terraDark: '#a34538',   // error pressed
} as const;

// =============================================================================
// Typography
// =============================================================================

export const fonts = {
  sans: "system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif",
  mono: "'JetBrains Mono', 'Fira Code', 'SF Mono', Monaco, 'Cascadia Code', 'Roboto Mono', Menlo, monospace",
} as const;

// =============================================================================
// Spacing & Sizing
// =============================================================================

export const radius = {
  card: '8px',
  button: '6px',
  pill: '9999px',
} as const;

// =============================================================================
// Transitions
// =============================================================================

export const transitions = {
  fast: '150ms ease-out',
  normal: '200ms ease-out',
  slow: '300ms ease-out',
} as const;

// =============================================================================
// Status Classes (for programmatic use)
// =============================================================================

export type RunStatus = 'idle' | 'working' | 'paused' | 'waiting' | 'done' | 'timed_out' | 'error';
export type TaskStatus = 'todo' | 'doing' | 'done' | 'blocked';
export type WorkerStatus = 'idle' | 'working' | 'waiting' | 'awaiting' | 'paused' | 'done' | 'error';

export const statusClasses: Record<RunStatus | TaskStatus | WorkerStatus, string> = {
  // Run/Worker statuses
  idle: 'status-idle',
  working: 'status-working',
  paused: 'status-idle',
  waiting: 'status-waiting',
  awaiting: 'status-waiting',
  done: 'status-done',
  timed_out: 'status-error',
  error: 'status-error',

  // Task statuses
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

// =============================================================================
// Theme Mode
// =============================================================================

export type ThemeMode = 'dark' | 'light';

export function setTheme(mode: ThemeMode): void {
  if (mode === 'light') {
    document.documentElement.classList.add('light');
    document.documentElement.setAttribute('data-theme', 'light');
  } else {
    document.documentElement.classList.remove('light');
    document.documentElement.setAttribute('data-theme', 'dark');
  }
  localStorage.setItem('hirsel-theme', mode);
}

export function getTheme(): ThemeMode {
  const stored = localStorage.getItem('hirsel-theme');
  if (stored === 'light' || stored === 'dark') {
    return stored;
  }
  // Default to dark mode (matches spec)
  return 'dark';
}

export function initTheme(): void {
  const mode = getTheme();
  setTheme(mode);
}

export function toggleTheme(): ThemeMode {
  const current = getTheme();
  const next: ThemeMode = current === 'dark' ? 'light' : 'dark';
  setTheme(next);
  return next;
}
