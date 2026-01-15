/**
 * Toast container Alpine component
 *
 * Manages toast notifications display with auto-dismiss and deduplication.
 */

interface Toast {
  id: number;
  type: 'success' | 'error' | 'warning' | 'info';
  title: string;
  message?: string;
  visible: boolean;
  key: string; // For deduplication
}

let toastId = 0;

// Durations by type (reduced for less intrusive UX)
const DURATIONS: Record<string, number> = {
  success: 2000,
  info: 2000,
  warning: 4000,
  error: 5000,
};

// Maximum number of toasts to show at once
const MAX_TOASTS = 3;

export function toastContainer() {
  return {
    toasts: [] as Toast[],

    init() {
      // Listen for toast events from window.toast
      window.addEventListener('hirsel:toast', ((e: CustomEvent) => {
        this.addToast(e.detail);
      }) as EventListener);

      // Also listen for basecoat:toast events for compatibility
      document.addEventListener('basecoat:toast', ((e: CustomEvent) => {
        const config = e.detail?.config;
        if (config) {
          this.addToast({
            type: config.category || 'info',
            title: config.title,
            message: config.description,
          });
        }
      }) as EventListener);
    },

    addToast(options: { type: Toast['type']; title: string; message?: string }) {
      // Create a key for deduplication (same type + title + message within short window)
      const key = `${options.type}:${options.title}:${options.message || ''}`;

      // Check for duplicate - don't add if same toast exists and is visible
      const existing = this.toasts.find(t => t.key === key && t.visible);
      if (existing) {
        return; // Skip duplicate
      }

      const toast: Toast = {
        id: ++toastId,
        type: options.type,
        title: options.title,
        message: options.message,
        visible: true,
        key,
      };

      // Remove oldest toasts if we're at max
      while (this.toasts.filter(t => t.visible).length >= MAX_TOASTS) {
        const oldest = this.toasts.find(t => t.visible);
        if (oldest) {
          this.dismissToast(oldest.id);
        }
      }

      this.toasts.push(toast);

      // Auto dismiss based on type
      setTimeout(() => {
        this.dismissToast(toast.id);
      }, DURATIONS[toast.type] || 2000);
    },

    dismissToast(id: number) {
      const toast = this.toasts.find((t) => t.id === id);
      if (toast) {
        toast.visible = false;
        // Remove from array after animation
        setTimeout(() => {
          this.toasts = this.toasts.filter((t) => t.id !== id);
        }, 200);
      }
    },

    getIcon(type: string): string {
      switch (type) {
        case 'success': return 'check-circle';
        case 'error': return 'x-circle';
        case 'warning': return 'alert-triangle';
        case 'info': return 'info';
        default: return 'info';
      }
    },
  };
}
