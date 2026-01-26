/**
 * SolidJS hook for debouncing
 */
import { createEffect, createSignal, onCleanup } from 'solid-js';

/**
 * Create a debounced version of a function
 */
export function createDebouncedFn<T extends (...args: unknown[]) => void>(
  fn: T,
  delayMs: number,
): T {
  let timeoutId: ReturnType<typeof setTimeout> | null = null;

  const debouncedFn = ((...args: Parameters<T>) => {
    if (timeoutId) {
      clearTimeout(timeoutId);
    }
    timeoutId = setTimeout(() => {
      fn(...args);
      timeoutId = null;
    }, delayMs);
  }) as T;

  return debouncedFn;
}

/**
 * Create a debounced signal that updates after a delay
 */
export function createDebouncedSignal<T>(
  value: T,
  delayMs: number,
): [() => T, (v: T) => void, () => T] {
  const [immediateValue, setImmediateValue] = createSignal<T>(value);
  const [debouncedValue, setDebouncedValue] = createSignal<T>(value);
  let timeoutId: ReturnType<typeof setTimeout> | null = null;

  const setValue = (newValue: T) => {
    setImmediateValue(() => newValue);

    if (timeoutId) {
      clearTimeout(timeoutId);
    }
    timeoutId = setTimeout(() => {
      setDebouncedValue(() => newValue);
      timeoutId = null;
    }, delayMs);
  };

  return [debouncedValue, setValue, immediateValue];
}

/**
 * Hook to run an effect with debounce on a signal change
 */
export function useDebouncedEffect<T>(
  value: () => T,
  effect: (value: T) => void,
  delayMs: number,
): void {
  let timeoutId: ReturnType<typeof setTimeout> | null = null;

  createEffect(() => {
    const currentValue = value();

    if (timeoutId) {
      clearTimeout(timeoutId);
    }

    timeoutId = setTimeout(() => {
      effect(currentValue);
      timeoutId = null;
    }, delayMs);
  });

  onCleanup(() => {
    if (timeoutId) {
      clearTimeout(timeoutId);
    }
  });
}
