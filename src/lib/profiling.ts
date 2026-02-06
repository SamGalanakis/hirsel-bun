/**
 * Frontend profiling infrastructure
 *
 * When HIRSEL_PROFILING is enabled, wraps IPC invoke() calls to measure
 * round-trip times and collects Core Web Vitals. Data is periodically
 * flushed to the backend for offline analysis.
 *
 * Zero overhead when profiling is not active.
 */

import { invoke } from '@tauri-apps/api/core';

/** A single IPC timing measurement */
interface IpcMeasurement {
  command: string;
  startTime: number;
  duration: number;
  payloadSize: number | null;
  timestamp: string;
  error: boolean;
}

/** Web vitals snapshot */
interface WebVitals {
  lcp: number | null;
  cls: number | null;
}

/** Memory snapshot at a point in time */
interface MemorySnapshot {
  timestamp: string;
  elapsedMs: number;
  frontend: {
    jsHeapUsedMB: number | null;
    jsHeapTotalMB: number | null;
  };
  backend: {
    rssMB: number | null;
  };
}

/** Profiling data written to disk */
interface ProfilingData {
  session: {
    startTime: string;
    endTime: string;
    durationMs: number;
  };
  webVitals: WebVitals;
  memory: MemorySnapshot[];
  ipc: {
    totalCalls: number;
    measurements: IpcMeasurement[];
    summary: Record<string, { count: number; totalMs: number; avgMs: number; maxMs: number }>;
  };
}

let enabled = false;
const measurements: IpcMeasurement[] = [];
const memorySnapshots: MemorySnapshot[] = [];
const sessionStart = Date.now();
let flushInterval: ReturnType<typeof setInterval> | null = null;
let memoryInterval: ReturnType<typeof setInterval> | null = null;
const vitals: WebVitals = { lcp: null, cls: null };

/** Check if profiling is active */
export function isProfilingEnabled(): boolean {
  return enabled;
}

/** Initialize profiling - call once on startup after checking backend flag */
export async function initProfiling(): Promise<void> {
  try {
    enabled = await invoke<boolean>('get_profiling_enabled');
  } catch {
    enabled = false;
  }

  if (!enabled) return;

  console.log('[profiling] Frontend profiling enabled');

  observeWebVitals();

  // Sample memory every 5 seconds
  captureMemorySnapshot();
  memoryInterval = setInterval(captureMemorySnapshot, 5_000);

  // Flush every 30 seconds
  flushInterval = setInterval(flushData, 30_000);

  // Flush on page unload
  window.addEventListener('beforeunload', () => {
    flushDataSync();
  });
}

/** Record an IPC call measurement */
export function recordIpc(
  command: string,
  startTime: number,
  duration: number,
  args: unknown,
  error: boolean,
): void {
  if (!enabled) return;

  let payloadSize: number | null = null;
  if (args != null) {
    try {
      payloadSize = JSON.stringify(args).length;
    } catch {
      // ignore
    }
  }

  measurements.push({
    command,
    startTime,
    duration,
    payloadSize,
    timestamp: new Date(startTime).toISOString(),
    error,
  });

  // Cap measurements to prevent unbounded memory growth
  if (measurements.length > 5000) {
    flushData();
    measurements.length = 0;
  }
}

/** Capture a memory snapshot (frontend JS heap + backend RSS) */
async function captureMemorySnapshot(): Promise<void> {
  const now = Date.now();
  let jsHeapUsedMB: number | null = null;
  let jsHeapTotalMB: number | null = null;
  let rssMB: number | null = null;

  // Frontend JS heap (WebKit may support performance.memory)
  const perfMemory = (
    performance as unknown as { memory?: { usedJSHeapSize: number; totalJSHeapSize: number } }
  ).memory;
  if (perfMemory) {
    jsHeapUsedMB = Math.round((perfMemory.usedJSHeapSize / 1024 / 1024) * 100) / 100;
    jsHeapTotalMB = Math.round((perfMemory.totalJSHeapSize / 1024 / 1024) * 100) / 100;
  }

  // Backend RSS from Tauri command
  try {
    rssMB = await invoke<number | null>('get_process_memory');
  } catch {
    // Command may not exist yet
  }

  memorySnapshots.push({
    timestamp: new Date(now).toISOString(),
    elapsedMs: now - sessionStart,
    frontend: { jsHeapUsedMB, jsHeapTotalMB },
    backend: { rssMB },
  });

  // Cap snapshots to prevent unbounded memory growth
  if (memorySnapshots.length > 500) {
    memorySnapshots.splice(0, memorySnapshots.length - 100);
  }
}

/** Observe LCP and CLS */
function observeWebVitals(): void {
  if (typeof PerformanceObserver === 'undefined') return;

  try {
    const lcpObserver = new PerformanceObserver((list) => {
      const entries = list.getEntries();
      if (entries.length > 0) {
        vitals.lcp = entries[entries.length - 1].startTime;
      }
    });
    lcpObserver.observe({ type: 'largest-contentful-paint', buffered: true });
  } catch {
    // LCP not supported
  }

  try {
    let clsValue = 0;
    const clsObserver = new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        const layoutShift = entry as PerformanceEntry & {
          hadRecentInput?: boolean;
          value?: number;
        };
        if (!layoutShift.hadRecentInput && layoutShift.value != null) {
          clsValue += layoutShift.value;
          vitals.cls = clsValue;
        }
      }
    });
    clsObserver.observe({ type: 'layout-shift', buffered: true });
  } catch {
    // CLS not supported
  }
}

/** Build the profiling data payload */
function buildData(): ProfilingData {
  const now = Date.now();
  const summary: ProfilingData['ipc']['summary'] = {};

  for (const m of measurements) {
    if (!summary[m.command]) {
      summary[m.command] = { count: 0, totalMs: 0, avgMs: 0, maxMs: 0 };
    }
    const s = summary[m.command];
    s.count++;
    s.totalMs += m.duration;
    s.maxMs = Math.max(s.maxMs, m.duration);
    s.avgMs = s.totalMs / s.count;
  }

  // Round summary values
  for (const s of Object.values(summary)) {
    s.totalMs = Math.round(s.totalMs * 100) / 100;
    s.avgMs = Math.round(s.avgMs * 100) / 100;
    s.maxMs = Math.round(s.maxMs * 100) / 100;
  }

  return {
    session: {
      startTime: new Date(sessionStart).toISOString(),
      endTime: new Date(now).toISOString(),
      durationMs: now - sessionStart,
    },
    webVitals: vitals,
    memory: memorySnapshots,
    ipc: {
      totalCalls: measurements.length,
      measurements,
      summary,
    },
  };
}

/** Flush profiling data to backend (async) */
async function flushData(): Promise<void> {
  if (measurements.length === 0) return;

  try {
    const data = buildData();
    await invoke('save_profiling_data', { data: JSON.stringify(data, null, 2) });
  } catch (e) {
    console.error('[profiling] Failed to flush:', e);
  }
}

/** Synchronous flush for beforeunload (best-effort using sendBeacon pattern) */
function flushDataSync(): void {
  if (measurements.length === 0) return;

  try {
    const data = buildData();
    // Use synchronous invoke via navigator.sendBeacon isn't available for IPC,
    // so we fire-and-forget the async version
    invoke('save_profiling_data', { data: JSON.stringify(data, null, 2) }).catch(() => {});
  } catch {
    // Best effort
  }
}

/** Stop profiling and do a final flush */
export async function stopProfiling(): Promise<void> {
  if (!enabled) return;

  if (flushInterval) {
    clearInterval(flushInterval);
    flushInterval = null;
  }
  if (memoryInterval) {
    clearInterval(memoryInterval);
    memoryInterval = null;
  }

  await flushData();
  enabled = false;
}
