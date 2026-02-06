/**
 * Visibility-aware polling utilities
 *
 * Provides a createPoll primitive that:
 * - Pauses/slows polling when the document is hidden (minimized/background tab)
 * - Supports adaptive intervals based on app state
 * - Integrates with SolidJS lifecycle (onCleanup)
 */

import { onCleanup } from 'solid-js';

const BACKGROUND_INTERVAL_MS = 30_000;

/** Whether the document is currently visible */
let _visible = typeof document !== 'undefined' ? !document.hidden : true;

if (typeof document !== 'undefined') {
  document.addEventListener('visibilitychange', () => {
    _visible = !document.hidden;
  });
}

export function isPageVisible(): boolean {
  return _visible;
}

interface PollOptions {
  /** Normal interval when page is visible (ms) */
  interval: number;
  /** Interval when page is hidden (ms). Defaults to 30s. Set to 0 to pause entirely. */
  backgroundInterval?: number;
  /** Whether to run immediately on creation */
  immediate?: boolean;
}

/**
 * Create a visibility-aware polling loop.
 * Automatically integrates with SolidJS onCleanup.
 *
 * Returns a handle to manually trigger a refresh.
 */
export function createPoll(fn: () => void | Promise<void>, opts: PollOptions): () => void {
  const { interval, backgroundInterval = BACKGROUND_INTERVAL_MS, immediate = true } = opts;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let stopped = false;

  const schedule = () => {
    if (stopped) return;
    const ms = _visible ? interval : backgroundInterval;
    if (ms <= 0) return; // paused entirely when hidden
    timer = setTimeout(tick, ms);
  };

  const tick = async () => {
    if (stopped) return;
    try {
      await fn();
    } catch {
      // swallow — individual pollers handle their own errors
    }
    schedule();
  };

  // Visibility change: reschedule with the right interval
  const onVisChange = () => {
    if (stopped) return;
    if (timer !== null) clearTimeout(timer);
    // When becoming visible, fire immediately to catch up
    if (_visible) {
      tick();
    } else {
      schedule();
    }
  };

  document.addEventListener('visibilitychange', onVisChange);

  if (immediate) {
    // Use microtask to not block the current synchronous flow
    queueMicrotask(() => tick());
  } else {
    schedule();
  }

  const stop = () => {
    stopped = true;
    if (timer !== null) clearTimeout(timer);
    document.removeEventListener('visibilitychange', onVisChange);
  };

  onCleanup(stop);

  // Return a manual refresh trigger
  return () => {
    if (stopped) return;
    if (timer !== null) clearTimeout(timer);
    tick();
  };
}
