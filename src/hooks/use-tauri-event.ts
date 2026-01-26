/**
 * SolidJS hook for Tauri events
 */
import { type UnlistenFn, listen } from '@tauri-apps/api/event';
import { createEffect, onCleanup } from 'solid-js';

/**
 * Subscribe to a Tauri event with automatic cleanup
 */
export function useTauriEvent<T>(eventName: string, handler: (payload: T) => void): void {
  createEffect(() => {
    let unlisten: UnlistenFn | null = null;

    listen<T>(eventName, (event) => {
      handler(event.payload);
    }).then((fn) => {
      unlisten = fn;
    });

    onCleanup(() => {
      unlisten?.();
    });
  });
}

/**
 * Subscribe to multiple Tauri events with automatic cleanup
 */
export function useTauriEvents(
  events: Array<{ name: string; handler: (payload: unknown) => void }>,
): void {
  createEffect(() => {
    const unlisteners: UnlistenFn[] = [];

    for (const { name, handler } of events) {
      listen(name, (event) => {
        handler(event.payload);
      }).then((fn) => {
        unlisteners.push(fn);
      });
    }

    onCleanup(() => {
      for (const unlisten of unlisteners) {
        unlisten();
      }
    });
  });
}
