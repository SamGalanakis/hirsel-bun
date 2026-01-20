/**
 * Toast notification system using Basecoat UI
 *
 * Dispatches basecoat:toast events for display via the toaster component.
 * See: https://basecoatui.com/components/toast/
 */

import type { GuiError } from './types';
import { getErrorLabel, isUserError } from './types';

/** Toast category (basecoat terminology) */
export type ToastCategory = 'success' | 'error' | 'warning' | 'info';

/** Toast configuration matching basecoat's API */
interface ToastConfig {
  category: ToastCategory;
  title: string;
  description?: string;
  duration?: number;
  cancel?: { label: string };
  action?: { label: string; onClick: () => void };
}

/** Default durations by category (shorter for less intrusive UX) */
const DEFAULT_DURATIONS: Record<ToastCategory, number> = {
  success: 2000,
  info: 2000,
  warning: 4000,
  error: 5000,
};

/**
 * Show a toast notification via basecoat
 */
function showToast(config: ToastConfig): void {
  const duration = config.duration ?? DEFAULT_DURATIONS[config.category];

  document.dispatchEvent(
    new CustomEvent('basecoat:toast', {
      detail: {
        config: {
          category: config.category,
          title: config.title,
          description: config.description,
          duration,
          // Only include cancel/action if explicitly provided
          ...(config.cancel && { cancel: config.cancel }),
          ...(config.action && { action: config.action }),
        },
      },
    }),
  );
}

/** Show a success toast */
export function success(title: string, description?: string): void {
  showToast({ category: 'success', title, description });
}

/** Show an error toast */
export function error(title: string, description?: string): void {
  showToast({ category: 'error', title, description });
}

/** Show a warning toast */
export function warning(title: string, description?: string): void {
  showToast({ category: 'warning', title, description });
}

/** Show an info toast */
export function info(title: string, description?: string): void {
  showToast({ category: 'info', title, description });
}

/** Show a toast from a GuiError */
export function fromGuiError(guiError: GuiError): void {
  const isUser = isUserError(guiError.code);
  const category: ToastCategory = isUser ? 'warning' : 'error';
  const label = getErrorLabel(guiError.code);
  showToast({ category, title: label, description: guiError.message });
}

/** Show a toast from any error */
export function fromError(err: unknown): void {
  if (err && typeof err === 'object' && 'code' in err && 'message' in err) {
    fromGuiError(err as GuiError);
    return;
  }

  if (err instanceof Error) {
    error('Error', err.message);
    return;
  }

  if (typeof err === 'string') {
    error('Error', err);
    return;
  }

  error('Error', 'An unexpected error occurred');
}

// Export convenience object
export const toast = {
  success,
  error,
  warning,
  info,
  fromError,
  fromGuiError,
};

// Make available globally for components (Window interface is in types.ts)
if (typeof window !== 'undefined') {
  window.toast = toast;
}
