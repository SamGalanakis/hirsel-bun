/**
 * Lifecycle utilities for Alpine.js components
 *
 * Provides helpers for managing event listeners and cleanup
 * in a consistent way across components.
 */

export type CleanupFunction = () => void;

/**
 * Tracks event listeners and other cleanup functions for a component.
 * Call cleanup() in the component's destroy() method.
 */
export class LifecycleManager {
  private cleanupFunctions: CleanupFunction[] = [];
  private eventListeners: Array<{
    target: EventTarget;
    type: string;
    handler: EventListener;
    options?: AddEventListenerOptions;
  }> = [];

  /**
   * Register a cleanup function to be called on destroy
   */
  onCleanup(fn: CleanupFunction): void {
    this.cleanupFunctions.push(fn);
  }

  /**
   * Add an event listener that will be automatically removed on cleanup
   */
  addEventListener<K extends keyof WindowEventMap>(
    target: Window,
    type: K,
    handler: (this: Window, ev: WindowEventMap[K]) => void,
    options?: AddEventListenerOptions
  ): void;
  addEventListener<K extends keyof DocumentEventMap>(
    target: Document,
    type: K,
    handler: (this: Document, ev: DocumentEventMap[K]) => void,
    options?: AddEventListenerOptions
  ): void;
  addEventListener(
    target: EventTarget,
    type: string,
    handler: EventListener,
    options?: AddEventListenerOptions
  ): void;
  addEventListener(
    target: EventTarget,
    type: string,
    handler: EventListener,
    options?: AddEventListenerOptions
  ): void {
    target.addEventListener(type, handler, options);
    this.eventListeners.push({ target, type, handler, options });
  }

  /**
   * Set an interval that will be automatically cleared on cleanup
   */
  setInterval(handler: () => void, timeout: number): ReturnType<typeof setInterval> {
    const id = setInterval(handler, timeout);
    this.onCleanup(() => clearInterval(id));
    return id;
  }

  /**
   * Set a timeout that will be automatically cleared on cleanup
   */
  setTimeout(handler: () => void, timeout: number): ReturnType<typeof setTimeout> {
    const id = setTimeout(handler, timeout);
    this.onCleanup(() => clearTimeout(id));
    return id;
  }

  /**
   * Perform all cleanup
   */
  cleanup(): void {
    // Remove all event listeners
    for (const { target, type, handler, options } of this.eventListeners) {
      target.removeEventListener(type, handler, options);
    }
    this.eventListeners = [];

    // Call all cleanup functions
    for (const fn of this.cleanupFunctions) {
      try {
        fn();
      } catch (err) {
        console.error('[LifecycleManager] Cleanup error:', err);
      }
    }
    this.cleanupFunctions = [];
  }
}

/**
 * Create a new lifecycle manager
 */
export function createLifecycle(): LifecycleManager {
  return new LifecycleManager();
}
