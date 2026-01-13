/**
 * Toast notification component for Alpine.js
 */

interface Toast {
  id: number;
  type: 'success' | 'error' | 'warning' | 'info';
  title: string | null;
  message: string;
  duration: number;
  visible: boolean;
}

interface ToastOptions {
  type: Toast['type'];
  message: string;
  title?: string | null;
  duration?: number;
}

let toastId = 0;

/**
 * Toast container Alpine component
 */
export function toastContainer() {
  return {
    toasts: [] as Toast[],

    init() {
      window.addEventListener('hirsel:toast', ((e: CustomEvent<ToastOptions>) => {
        this.addToast(e.detail);
      }) as EventListener);
    },

    addToast(options: ToastOptions) {
      const id = ++toastId;
      const toast: Toast = {
        id,
        type: options.type || 'info',
        title: options.title || null,
        message: options.message,
        duration: options.duration || 4000,
        visible: true,
      };
      this.toasts.push(toast);

      // Auto-dismiss
      setTimeout(() => {
        this.dismissToast(id);
      }, toast.duration);
    },

    dismissToast(id: number) {
      const toast = this.toasts.find(t => t.id === id);
      if (toast) {
        toast.visible = false;
        // Remove from array after animation
        setTimeout(() => {
          const idx = this.toasts.findIndex(t => t.id === id);
          if (idx !== -1) {
            this.toasts.splice(idx, 1);
          }
        }, 200);
      }
    },

    getToastClass(type: Toast['type']): string {
      const classes = {
        success: 'border-sage bg-sage/10',
        error: 'border-terra bg-terra/10',
        warning: 'border-golden bg-golden/10',
        info: 'border-sky-500 bg-sky-500/10',
      };
      return classes[type] || classes.info;
    },

    getToastIcon(type: Toast['type']): string {
      const icons = {
        success: '✓',
        error: '✕',
        warning: '⚠',
        info: 'ℹ',
      };
      return icons[type] || icons.info;
    },

    getToastIconClass(type: Toast['type']): string {
      const classes = {
        success: 'text-sage',
        error: 'text-terra',
        warning: 'text-golden',
        info: 'text-sky-500',
      };
      return classes[type] || classes.info;
    },
  };
}

/**
 * Initialize global toast API
 */
export function initToastApi() {
  window.toast = {
    show(options: ToastOptions) {
      window.dispatchEvent(new CustomEvent('hirsel:toast', { detail: options }));
    },
    success(message: string, title: string | null = null) {
      this.show({ type: 'success', message, title });
    },
    error(message: string, title = 'Error') {
      this.show({ type: 'error', message, title, duration: 8000 });
    },
    warning(message: string, title: string | null = null) {
      this.show({ type: 'warning', message, title });
    },
    info(message: string, title: string | null = null) {
      this.show({ type: 'info', message, title });
    },
  };
}
