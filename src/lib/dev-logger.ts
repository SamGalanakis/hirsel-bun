/**
 * Development console logger
 *
 * In dev mode, intercepts console.log/warn/error and sends them to the backend
 * to be written to a log file for easier debugging.
 */

import { invoke } from '@tauri-apps/api/core';

// Only enable in development
const isDev = import.meta.env.DEV;

// Store original console methods
const originalConsole = {
  log: console.log.bind(console),
  warn: console.warn.bind(console),
  error: console.error.bind(console),
  info: console.info.bind(console),
  debug: console.debug.bind(console),
};

// Format arguments for logging
function formatArgs(args: unknown[]): string {
  return args
    .map((arg) => {
      if (arg === null) return 'null';
      if (arg === undefined) return 'undefined';
      if (typeof arg === 'object') {
        try {
          return JSON.stringify(arg, null, 2);
        } catch {
          return String(arg);
        }
      }
      return String(arg);
    })
    .join(' ');
}

// Send log to backend
async function sendToBackend(level: string, message: string) {
  try {
    await invoke('log_frontend', { level, message });
  } catch {
    // Silently fail - don't want logging to break the app
  }
}

/**
 * Initialize development console logging
 * Call this early in app initialization
 */
export function initDevLogger() {
  if (!isDev) {
    return;
  }

  // Override console methods
  console.log = (...args: unknown[]) => {
    originalConsole.log(...args);
    sendToBackend('INFO', formatArgs(args));
  };

  console.info = (...args: unknown[]) => {
    originalConsole.info(...args);
    sendToBackend('INFO', formatArgs(args));
  };

  console.warn = (...args: unknown[]) => {
    originalConsole.warn(...args);
    sendToBackend('WARN', formatArgs(args));
  };

  console.error = (...args: unknown[]) => {
    originalConsole.error(...args);
    sendToBackend('ERROR', formatArgs(args));
  };

  console.debug = (...args: unknown[]) => {
    originalConsole.debug(...args);
    sendToBackend('DEBUG', formatArgs(args));
  };

  // Log that dev logger is initialized - use the NEW console.log so it goes to backend
  console.log('[DevLogger] Frontend console logging enabled, isDev:', isDev);
}
