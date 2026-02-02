import { onCleanup, onMount } from 'solid-js';

/**
 * Hook to handle escape key press
 * @param callback Function to call when escape is pressed
 */
export function useEscapeKey(callback: () => void): void {
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Escape') {
      callback();
    }
  };

  onMount(() => {
    document.addEventListener('keydown', handleKeyDown);
  });

  onCleanup(() => {
    document.removeEventListener('keydown', handleKeyDown);
  });
}
