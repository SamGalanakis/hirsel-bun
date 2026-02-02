/**
 * useElapsedTime - Reactive elapsed time tracking from a start timestamp
 *
 * Creates a signal that updates every second with the formatted elapsed time.
 * Handles cleanup automatically on unmount.
 */
import { type Accessor, createEffect, createSignal, onCleanup } from 'solid-js';
import { formatElapsedTime } from '../lib/utils/formatters';

/**
 * Hook for tracking elapsed time from a start timestamp
 *
 * @param startTimestamp - Accessor returning the start timestamp string (or undefined/null)
 * @returns Accessor<string> - Formatted elapsed time (e.g., "5m 30s", "1h 2m")
 *
 * @example
 * const elapsedTime = useElapsedTime(() => worker.sessionStartedAt);
 * // In JSX: <span>{elapsedTime()}</span>
 */
export function useElapsedTime(
  startTimestamp: Accessor<string | null | undefined>,
): Accessor<string> {
  const [elapsedTime, setElapsedTime] = createSignal('');

  createEffect(() => {
    const timestamp = startTimestamp();
    if (!timestamp) {
      setElapsedTime('');
      return;
    }

    // Initial update
    setElapsedTime(formatElapsedTime(timestamp));

    // Update every second
    const interval = setInterval(() => {
      setElapsedTime(formatElapsedTime(timestamp));
    }, 1000);

    onCleanup(() => clearInterval(interval));
  });

  return elapsedTime;
}
