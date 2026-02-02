import { type Accessor, onCleanup, onMount } from 'solid-js';

/**
 * Hook to detect clicks outside an element
 * @param ref Accessor returning the element to monitor
 * @param callback Function to call when click outside is detected
 */
export function useClickOutside(
  ref: Accessor<HTMLElement | undefined>,
  callback: () => void,
): void {
  const handleClick = (e: MouseEvent) => {
    const element = ref();
    if (element && !element.contains(e.target as Node)) {
      callback();
    }
  };

  onMount(() => {
    // Use setTimeout to avoid triggering on the click that opened the element
    setTimeout(() => {
      document.addEventListener('click', handleClick);
    }, 0);
  });

  onCleanup(() => {
    document.removeEventListener('click', handleClick);
  });
}
