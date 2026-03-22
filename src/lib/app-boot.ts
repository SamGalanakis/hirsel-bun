import { createEffect, onCleanup } from 'solid-js';
import { initTheme } from './theme';

export function useAppBoot() {
  createEffect(() => {
    initTheme();

    const contextMenuHandler = (e: MouseEvent) => {
      const toast = (e.target as HTMLElement).closest('.toaster .toast');
      if (toast) {
        e.preventDefault();
        toast.setAttribute('aria-hidden', 'true');
        setTimeout(() => toast.remove(), 300);
      }
    };

    document.addEventListener('contextmenu', contextMenuHandler);
    console.log('[Hirsel] App initialized');

    onCleanup(() => {
      document.removeEventListener('contextmenu', contextMenuHandler);
    });
  });
}
