/**
 * SolidJS hook for polling data at intervals
 */
import { createEffect, onCleanup } from 'solid-js';

/**
 * Poll a function at a regular interval with automatic cleanup
 */
export function usePolling(
  fn: () => void | Promise<void>,
  intervalMs: number,
  options: { immediate?: boolean } = {},
): void {
  createEffect(() => {
    // Run immediately if requested
    if (options.immediate) {
      fn();
    }

    const interval = setInterval(() => {
      fn();
    }, intervalMs);

    onCleanup(() => {
      clearInterval(interval);
    });
  });
}

/**
 * Conditionally poll based on a predicate
 */
export function useConditionalPolling(
  fn: () => void | Promise<void>,
  intervalMs: number,
  shouldPoll: () => boolean,
): void {
  createEffect(() => {
    if (!shouldPoll()) return;

    const interval = setInterval(() => {
      if (shouldPoll()) {
        fn();
      }
    }, intervalMs);

    onCleanup(() => {
      clearInterval(interval);
    });
  });
}
