/**
 * Development console logger
 *
 * In dev mode, intercepts console.log/warn/error and sends them to the backend
 * to be written to a log file for easier debugging.
 *
 * Batches messages and flushes periodically to reduce IPC overhead.
 */

import { invoke } from './invoke';

// Only enable in development
const isDev = (import.meta as { env?: { DEV?: boolean } }).env?.DEV ?? false;

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

// Batch buffer for log messages
let logBuffer: [string, string][] = [];
let flushTimer: ReturnType<typeof setTimeout> | null = null;
const FLUSH_INTERVAL_MS = 2000;
const MAX_BUFFER_SIZE = 50;

function flushLogs() {
  if (logBuffer.length === 0) return;
  const entries = logBuffer;
  logBuffer = [];
  invoke('log_frontend_batch', { entries }).catch(() => {
    // Silently fail
  });
}

function queueLog(level: string, message: string) {
  logBuffer.push([level, message]);
  if (logBuffer.length >= MAX_BUFFER_SIZE) {
    if (flushTimer !== null) clearTimeout(flushTimer);
    flushTimer = null;
    flushLogs();
  } else if (flushTimer === null) {
    flushTimer = setTimeout(() => {
      flushTimer = null;
      flushLogs();
    }, FLUSH_INTERVAL_MS);
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
    queueLog('INFO', formatArgs(args));
  };

  console.info = (...args: unknown[]) => {
    originalConsole.info(...args);
    queueLog('INFO', formatArgs(args));
  };

  console.warn = (...args: unknown[]) => {
    originalConsole.warn(...args);
    queueLog('WARN', formatArgs(args));
  };

  console.error = (...args: unknown[]) => {
    originalConsole.error(...args);
    queueLog('ERROR', formatArgs(args));
  };

  console.debug = (...args: unknown[]) => {
    originalConsole.debug(...args);
    queueLog('DEBUG', formatArgs(args));
  };

  // Log that dev logger is initialized - use the NEW console.log so it goes to backend
  console.log('[DevLogger] Frontend console logging enabled, isDev:', isDev);
}
