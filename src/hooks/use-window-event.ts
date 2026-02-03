import { onCleanup, onMount } from 'solid-js';

/**
 * Hook to handle window events with automatic cleanup.
 *
 * @param eventName The name of the window event to listen for
 * @param handler Callback to handle the event
 */
export function useWindowEvent<T extends Event = Event>(
  eventName: string,
  handler: (event: T) => void,
): void {
  const wrappedHandler = (e: Event) => handler(e as T);

  onMount(() => {
    window.addEventListener(eventName, wrappedHandler);
  });

  onCleanup(() => {
    window.removeEventListener(eventName, wrappedHandler);
  });
}
