/**
 * Toast notification system for Hirsel
 *
 * Provides a simple API for showing toast notifications with different
 * severity levels (success, error, warning, info).
 */

import type { GuiError, ErrorCode } from './types';
import { isUserError, getErrorLabel } from './types';

/** Toast notification type */
export type ToastType = 'success' | 'error' | 'warning' | 'info';

/** Toast notification data */
export interface Toast {
  id: string;
  type: ToastType;
  title: string;
  message?: string;
  duration: number;
  exiting?: boolean;
}

/** Toast store state */
interface ToastState {
  toasts: Toast[];
  nextId: number;
}

// Global toast state
const state: ToastState = {
  toasts: [],
  nextId: 1,
};

// Toast update listeners
const listeners: Set<() => void> = new Set();

/** Subscribe to toast updates */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Get current toasts */
export function getToasts(): Toast[] {
  return state.toasts;
}

/** Notify listeners of state change */
function notifyListeners(): void {
  listeners.forEach(listener => listener());
}

/** Generate unique toast ID */
function generateId(): string {
  return `toast-${state.nextId++}`;
}

/** Default duration in ms */
const DEFAULT_DURATION = 5000;

/** Add a toast notification */
export function showToast(
  type: ToastType,
  title: string,
  message?: string,
  duration: number = DEFAULT_DURATION
): string {
  const id = generateId();
  const toast: Toast = {
    id,
    type,
    title,
    message,
    duration,
  };

  state.toasts = [...state.toasts, toast];
  notifyListeners();

  // Auto-dismiss after duration
  if (duration > 0) {
    setTimeout(() => dismissToast(id), duration);
  }

  return id;
}

/** Dismiss a toast (with exit animation) */
export function dismissToast(id: string): void {
  // Mark toast as exiting
  state.toasts = state.toasts.map(t =>
    t.id === id ? { ...t, exiting: true } : t
  );
  notifyListeners();

  // Remove after animation
  setTimeout(() => {
    state.toasts = state.toasts.filter(t => t.id !== id);
    notifyListeners();
  }, 300);
}

/** Clear all toasts */
export function clearToasts(): void {
  state.toasts = [];
  notifyListeners();
}

// Convenience methods

/** Show a success toast */
export function success(title: string, message?: string): string {
  return showToast('success', title, message);
}

/** Show an error toast */
export function error(title: string, message?: string): string {
  return showToast('error', title, message, 8000); // Longer duration for errors
}

/** Show a warning toast */
export function warning(title: string, message?: string): string {
  return showToast('warning', title, message, 6000);
}

/** Show an info toast */
export function info(title: string, message?: string): string {
  return showToast('info', title, message);
}

/** Show a toast from a GuiError */
export function fromGuiError(guiError: GuiError): string {
  const isUser = isUserError(guiError.code);
  const type: ToastType = isUser ? 'warning' : 'error';
  const label = getErrorLabel(guiError.code);

  return showToast(
    type,
    label,
    guiError.message,
    isUser ? 6000 : 8000
  );
}

/** Show a toast from any error */
export function fromError(err: unknown): string {
  // Check if it's a GuiError (has code and message)
  if (err && typeof err === 'object' && 'code' in err && 'message' in err) {
    return fromGuiError(err as GuiError);
  }

  // Handle standard Error
  if (err instanceof Error) {
    return error('Error', err.message);
  }

  // Handle string
  if (typeof err === 'string') {
    return error('Error', err);
  }

  // Unknown error
  return error('Error', 'An unexpected error occurred');
}

// SVG icons for each toast type
export const TOAST_ICONS: Record<ToastType, string> = {
  success: `<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="2" stroke="currentColor">
    <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
  </svg>`,
  error: `<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="2" stroke="currentColor">
    <path stroke-linecap="round" stroke-linejoin="round" d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126zM12 15.75h.007v.008H12v-.008z" />
  </svg>`,
  warning: `<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="2" stroke="currentColor">
    <path stroke-linecap="round" stroke-linejoin="round" d="M12 9v3.75m9-.75a9 9 0 11-18 0 9 9 0 0118 0zm-9 3.75h.008v.008H12v-.008z" />
  </svg>`,
  info: `<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="2" stroke="currentColor">
    <path stroke-linecap="round" stroke-linejoin="round" d="M11.25 11.25l.041-.02a.75.75 0 011.063.852l-.708 2.836a.75.75 0 001.063.853l.041-.021M21 12a9 9 0 11-18 0 9 9 0 0118 0zm-9-3.75h.008v.008H12V8.25z" />
  </svg>`,
};

/**
 * Alpine.js component for toast container
 */
export function toastContainer() {
  return {
    toasts: [] as Toast[],
    unsubscribe: null as (() => void) | null,

    init() {
      // Subscribe to toast updates
      this.unsubscribe = subscribe(() => {
        this.toasts = getToasts();
      });
      this.toasts = getToasts();
    },

    destroy() {
      if (this.unsubscribe) {
        this.unsubscribe();
      }
    },

    dismiss(id: string) {
      dismissToast(id);
    },

    getIcon(type: ToastType): string {
      return TOAST_ICONS[type];
    },

    getClass(toast: Toast): string {
      const classes = ['toast', `toast-${toast.type}`];
      if (toast.exiting) {
        classes.push('toast-exit');
      }
      return classes.join(' ');
    },
  };
}

// Export for global access
export const toast = {
  success,
  error,
  warning,
  info,
  show: showToast,
  dismiss: dismissToast,
  clear: clearToasts,
  fromError,
  fromGuiError,
};

// Make available globally
if (typeof window !== 'undefined') {
  (window as unknown as Record<string, unknown>).toast = toast;
  (window as unknown as Record<string, unknown>).toastContainer = toastContainer;
}
