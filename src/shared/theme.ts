// Hirsel Theme - Warm Pastoral Design
// The name "hirsel" is a Scottish term for a flock of sheep.
// The theme evokes the feeling of a shepherd overseeing their flock.

// Color Palette
export const COLORS = {
  // Backgrounds
  background: "#1a1a1a", // Dark charcoal, like evening pasture
  surface: "#242424", // Slightly lighter panels
  border: "#333333", // Subtle separation

  // Text
  textPrimary: "#e8e4df", // Warm off-white, like wool
  textSecondary: "#8a8580", // Muted warmth
  textDim: "#5a5550",

  // Accent - Shepherd's lantern
  accent: "#d4a574", // Warm amber/gold
  accentBright: "#e8c19a",

  // Status colors
  success: "#7d9970", // Sage green - healthy pasture
  warning: "#c9a227", // Golden yellow - attention needed
  error: "#c45c4a", // Terracotta red - problem

  // Worker/Run states
  working: "#d4a574", // Amber glow - active
  waiting: "#c9a227", // Needs attention
  done: "#7d9970", // Completed, at peace
  idle: "#5a5550", // Resting
} as const;

// Status Icons
export const ICONS = {
  // Worker states
  working: "\u25cf", // ● filled, glowing
  waiting: "\u25d0", // ◐ half, attention
  idle: "\u25cb", // ○ empty, resting
  done: "\u2713", // ✓ checkmark
  error: "\u2717", // ✗ failed
  paused: "\u23f8", // ⏸ paused
  awaiting: "\u25d4", // ◔ quarter, waiting for tasks

  // Task states
  taskDoing: "\u25ba", // ► in progress
  taskTodo: "\u25cb", // ○ pending
  taskDone: "\u2713", // ✓ complete
  taskBlocked: "\u29d7", // ⧗ blocked

  // Run states
  runActive: "\u25cf", // ● active run
  runPaused: "\u25cc", // ◌ paused
  runDelivered: "\u2713", // ✓ delivered

  // UI elements
  leader: "\u2605", // ★ leader badge
  unread: "\u25cf", // ● notification dot
  arrow: "\u2192", // → transition
  claim: "\u25ba", // ► claim action
  unclaim: "\u25cb", // ○ unclaim
  add: "+", // + add task
} as const;

// Status to icon mapping
export const STATUS_ICON: Record<string, string> = {
  working: ICONS.working,
  waiting: ICONS.waiting,
  awaiting: ICONS.awaiting,
  idle: ICONS.idle,
  done: ICONS.done,
  error: ICONS.error,
  paused: ICONS.paused,
  // Run statuses
  eval: ICONS.working,
  eval_failed: ICONS.error,
  delivered: ICONS.done,
  merged: ICONS.done,
  timed_out: ICONS.paused,
  runaway: ICONS.error,
};

// Status to color mapping
export const STATUS_COLOR: Record<string, string> = {
  working: COLORS.working,
  waiting: COLORS.waiting,
  awaiting: COLORS.warning,
  idle: COLORS.idle,
  done: COLORS.done,
  error: COLORS.error,
  paused: COLORS.warning,
  // Run statuses
  eval: COLORS.working,
  eval_failed: COLORS.error,
  delivered: COLORS.done,
  merged: COLORS.done,
  timed_out: COLORS.warning,
  runaway: COLORS.error,
};

// Task status styling
export const TASK_STATUS_ICON: Record<string, string> = {
  todo: ICONS.taskTodo,
  doing: ICONS.taskDoing,
  done: ICONS.taskDone,
};

export const TASK_STATUS_COLOR: Record<string, string> = {
  todo: COLORS.textDim,
  doing: COLORS.accent,
  done: COLORS.done,
};

// Dithering characters for visual texture (progress bars)
export const DITHER = {
  empty: " ",
  light: "\u2591", // ░
  medium: "\u2592", // ▒
  heavy: "\u2593", // ▓
  solid: "\u2588", // █
} as const;

// Box drawing for borders
export const BOX = {
  tl: "\u250c", // ┌
  tr: "\u2510", // ┐
  bl: "\u2514", // └
  br: "\u2518", // ┘
  h: "\u2500", // ─
  v: "\u2502", // │
  cross: "\u253c", // ┼
  tDown: "\u252c", // ┬
  tUp: "\u2534", // ┴
  tRight: "\u251c", // ├
  tLeft: "\u2524", // ┤
} as const;

// Tree drawing
export const TREE = {
  branch: "\u251c\u2500", // ├─
  last: "\u2514\u2500", // └─
  vertical: "\u2502 ", // │
  space: "  ",
} as const;

// ASCII art logo with sheep
export const LOGO = `       ,ww
    wWWWWWW_)
    \`WWWWWW'
      || ||
  \u254b \u254b\u254b\u250f\u2501\u2513\u250f\u2501\u2513\u250f\u2501\u2578\u254b
  \u2523\u2501\u252b\u2503\u2523\u2533\u251b\u2517\u2501\u2513\u2523\u2578 \u2503
  \u2579 \u2579\u2579\u2579\u2517\u2578\u2517\u2501\u251b\u2517\u2501\u2578\u2517\u2501\u2578`;

export const LOGO_SMALL = "@ hirsel";

// ANSI escape codes for terminal formatting
const ANSI = {
  reset: "\x1b[0m",
  bold: "\x1b[1m",
  dim: "\x1b[2m",
  underline: "\x1b[4m",
  // Foreground colors
  fgBlack: "\x1b[30m",
  fgRed: "\x1b[31m",
  fgGreen: "\x1b[32m",
  fgYellow: "\x1b[33m",
  fgBlue: "\x1b[34m",
  fgMagenta: "\x1b[35m",
  fgCyan: "\x1b[36m",
  fgWhite: "\x1b[37m",
  fgBrightBlack: "\x1b[90m",
  fgBrightRed: "\x1b[91m",
  fgBrightGreen: "\x1b[92m",
  fgBrightYellow: "\x1b[93m",
  fgBrightBlue: "\x1b[94m",
  fgBrightMagenta: "\x1b[95m",
  fgBrightCyan: "\x1b[96m",
  fgBrightWhite: "\x1b[97m",
} as const;

// Color name to ANSI mapping
const COLOR_MAP: Record<string, string> = {
  black: ANSI.fgBlack,
  red: ANSI.fgRed,
  green: ANSI.fgGreen,
  yellow: ANSI.fgYellow,
  blue: ANSI.fgBlue,
  magenta: ANSI.fgMagenta,
  cyan: ANSI.fgCyan,
  white: ANSI.fgWhite,
  brightBlack: ANSI.fgBrightBlack,
  brightRed: ANSI.fgBrightRed,
  brightGreen: ANSI.fgBrightGreen,
  brightYellow: ANSI.fgBrightYellow,
  brightBlue: ANSI.fgBrightBlue,
  brightMagenta: ANSI.fgBrightMagenta,
  brightCyan: ANSI.fgBrightCyan,
  brightWhite: ANSI.fgBrightWhite,
};

// Helper functions

/**
 * Make text dim (lower intensity)
 */
export function dim(text: string): string {
  return `${ANSI.dim}${text}${ANSI.reset}`;
}

/**
 * Make text bold
 */
export function bold(text: string): string {
  return `${ANSI.bold}${text}${ANSI.reset}`;
}

/**
 * Colorize text with a named color
 */
export function colorize(text: string, color: string): string {
  const code = COLOR_MAP[color];
  if (!code) return text;
  return `${code}${text}${ANSI.reset}`;
}

/**
 * Format status with icon and color
 */
export function formatStatus(status: string): string {
  const icon = STATUS_ICON[status] || ICONS.idle;
  const colorCode =
    status === "working" || status === "eval"
      ? ANSI.fgYellow
      : status === "done" || status === "delivered" || status === "merged"
      ? ANSI.fgGreen
      : status === "waiting" || status === "paused" || status === "timed_out"
      ? ANSI.fgBrightYellow
      : status === "error" || status === "eval_failed" || status === "runaway"
      ? ANSI.fgRed
      : ANSI.dim;
  return `${colorCode}${icon} ${status}${ANSI.reset}`;
}

/**
 * Format task status with icon
 */
export function formatTaskStatus(status: string): string {
  const icon = TASK_STATUS_ICON[status] || ICONS.taskTodo;
  const colorCode =
    status === "doing"
      ? ANSI.fgYellow
      : status === "done"
      ? ANSI.fgGreen
      : ANSI.dim;
  return `${colorCode}${icon}${ANSI.reset}`;
}

/**
 * Creates a dithered progress bar
 */
export function ditherBar(filled: number, total: number, width = 20): string {
  if (total === 0) return DITHER.light.repeat(width);

  const ratio = filled / total;

  if (ratio >= 1.0) return DITHER.solid.repeat(width);
  if (ratio <= 0) return DITHER.light.repeat(width);

  let bar = "";
  for (let i = 0; i < width; i++) {
    const pos = (i + 0.5) / width;
    if (pos < ratio - 0.08) {
      bar += DITHER.solid;
    } else if (pos < ratio - 0.04) {
      bar += DITHER.heavy;
    } else if (pos < ratio) {
      bar += DITHER.medium;
    } else if (pos < ratio + 0.04) {
      bar += DITHER.light;
    } else {
      bar += DITHER.empty;
    }
  }
  return bar;
}

/**
 * Formats elapsed time in human readable format
 */
export function formatElapsed(minutes: number): string {
  if (minutes < 1) return "<1m";
  if (minutes < 60) return `${Math.floor(minutes)}m`;
  const hours = Math.floor(minutes / 60);
  const mins = Math.floor(minutes % 60);
  if (hours < 24) {
    return mins > 0 ? `${hours}h${mins}m` : `${hours}h`;
  }
  const days = Math.floor(hours / 24);
  const remainingHours = hours % 24;
  return remainingHours > 0 ? `${days}d${remainingHours}h` : `${days}d`;
}

/**
 * Formats a timestamp as HH:MM
 */
export function formatTime(timestamp: string): string {
  const date = new Date(timestamp);
  return date.toLocaleTimeString("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

/**
 * Get status styling (icon and color) for a run status
 */
export interface StatusStyle {
  icon: string;
  color: string;
}

export function getRunStatusStyle(status: string): StatusStyle {
  return {
    icon: STATUS_ICON[status] ?? ICONS.idle,
    color: STATUS_COLOR[status] ?? COLORS.textDim,
  };
}

/**
 * Get status styling for a worker status
 */
export function getWorkerStatusStyle(status: string): StatusStyle {
  return {
    icon: STATUS_ICON[status] ?? ICONS.idle,
    color: STATUS_COLOR[status] ?? COLORS.textDim,
  };
}

// CSS variables for the desktop app
export const CSS_VARIABLES = `
:root {
  --color-background: ${COLORS.background};
  --color-surface: ${COLORS.surface};
  --color-border: ${COLORS.border};

  --color-text-primary: ${COLORS.textPrimary};
  --color-text-secondary: ${COLORS.textSecondary};
  --color-text-dim: ${COLORS.textDim};

  --color-accent: ${COLORS.accent};
  --color-accent-bright: ${COLORS.accentBright};

  --color-success: ${COLORS.success};
  --color-warning: ${COLORS.warning};
  --color-error: ${COLORS.error};

  --color-working: ${COLORS.working};
  --color-waiting: ${COLORS.waiting};
  --color-done: ${COLORS.done};
  --color-idle: ${COLORS.idle};

  --font-mono: "JetBrains Mono", "Fira Code", monospace;
  --font-system: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
}
`;
