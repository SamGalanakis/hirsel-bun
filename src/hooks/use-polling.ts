import { onCleanup, onMount } from 'solid-js';
import { createPoller } from '../lib/api';

/**
 * Hook for polling with automatic lifecycle management.
 * Starts polling on mount and stops on cleanup.
 *
 * @param fetcher Async function to fetch data
 * @param onUpdate Callback when new data is received
 * @param intervalMs Polling interval in milliseconds (default: 2000)
 */
export function usePolling<T>(
  fetcher: () => Promise<T>,
  onUpdate: (data: T) => void,
  intervalMs = 2000,
): void {
  onMount(() => {
    const { start, stop } = createPoller(fetcher, onUpdate, intervalMs);
    start();
    onCleanup(stop);
  });
}
