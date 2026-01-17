/**
 * Polling utilities
 *
 * Provides standardized polling with visibility API support.
 * Pauses polling when the tab is hidden to save resources.
 */

export interface PollingOptions {
  /** Polling interval in milliseconds */
  interval: number;
  /** Whether to poll immediately on start (default: true) */
  immediate?: boolean;
  /** Whether to pause when tab is hidden (default: true) */
  pauseOnHidden?: boolean;
  /** Callback when polling starts */
  onStart?: () => void;
  /** Callback when polling stops */
  onStop?: () => void;
}

export interface PollingController {
  /** Start polling */
  start(): void;
  /** Stop polling */
  stop(): void;
  /** Check if currently polling */
  isPolling(): boolean;
  /** Trigger an immediate poll (resets the interval) */
  poll(): void;
}

/**
 * Create a polling controller for a callback function.
 *
 * The controller handles:
 * - Regular interval-based polling
 * - Automatic pause/resume based on page visibility
 * - Manual start/stop control
 *
 * Usage:
 *   const poller = createPoller(async () => {
 *     const data = await fetchData();
 *     updateUI(data);
 *   }, { interval: 5000 });
 *
 *   // In init()
 *   poller.start();
 *
 *   // In destroy()
 *   poller.stop();
 */
export function createPoller(
  callback: () => void | Promise<void>,
  options: PollingOptions
): PollingController {
  const {
    interval,
    immediate = true,
    pauseOnHidden = true,
    onStart,
    onStop,
  } = options;

  let intervalId: ReturnType<typeof setInterval> | null = null;
  let isActive = false;
  let isPaused = false;

  const handleVisibilityChange = () => {
    if (!pauseOnHidden || !isActive) return;

    if (document.hidden) {
      // Pause polling when hidden
      if (intervalId) {
        clearInterval(intervalId);
        intervalId = null;
        isPaused = true;
      }
    } else if (isPaused) {
      // Resume polling when visible
      isPaused = false;
      startInterval();
      // Poll immediately after becoming visible
      callback();
    }
  };

  const startInterval = () => {
    if (intervalId) return;
    intervalId = setInterval(() => {
      callback();
    }, interval);
  };

  const start = () => {
    if (isActive) return;
    isActive = true;
    isPaused = false;

    if (pauseOnHidden) {
      document.addEventListener('visibilitychange', handleVisibilityChange);
    }

    if (!document.hidden || !pauseOnHidden) {
      startInterval();
      if (immediate) {
        callback();
      }
    }

    onStart?.();
  };

  const stop = () => {
    if (!isActive) return;
    isActive = false;
    isPaused = false;

    if (intervalId) {
      clearInterval(intervalId);
      intervalId = null;
    }

    if (pauseOnHidden) {
      document.removeEventListener('visibilitychange', handleVisibilityChange);
    }

    onStop?.();
  };

  const poll = () => {
    if (!isActive) return;

    // Clear and restart the interval
    if (intervalId) {
      clearInterval(intervalId);
    }

    callback();
    startInterval();
  };

  return {
    start,
    stop,
    isPolling: () => isActive && !isPaused,
    poll,
  };
}

/**
 * Create a simple debounced function
 */
export function debounce<T extends (...args: unknown[]) => void>(
  fn: T,
  delay: number
): T & { cancel: () => void } {
  let timeoutId: ReturnType<typeof setTimeout> | null = null;

  const debounced = ((...args: unknown[]) => {
    if (timeoutId) {
      clearTimeout(timeoutId);
    }
    timeoutId = setTimeout(() => {
      fn(...args);
      timeoutId = null;
    }, delay);
  }) as T & { cancel: () => void };

  debounced.cancel = () => {
    if (timeoutId) {
      clearTimeout(timeoutId);
      timeoutId = null;
    }
  };

  return debounced;
}

/**
 * Create a simple throttled function
 */
export function throttle<T extends (...args: unknown[]) => void>(
  fn: T,
  delay: number
): T {
  let lastCall = 0;

  return ((...args: unknown[]) => {
    const now = Date.now();
    if (now - lastCall >= delay) {
      lastCall = now;
      fn(...args);
    }
  }) as T;
}
