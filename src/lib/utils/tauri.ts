/**
 * Tauri IPC utilities
 */

declare global {
  interface Window {
    tauriInvoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
    tauriGetCurrentWindow: () => {
      minimize: () => Promise<void>;
      maximize: () => Promise<void>;
      unmaximize: () => Promise<void>;
      close: () => Promise<void>;
      isMaximized: () => Promise<boolean>;
    };
    toast: {
      show: (options: ToastOptions) => void;
      success: (message: string, title?: string | null) => void;
      error: (message: string, title?: string) => void;
      warning: (message: string, title?: string | null) => void;
      info: (message: string, title?: string | null) => void;
    };
  }
}

interface ToastOptions {
  type: 'success' | 'error' | 'warning' | 'info';
  message: string;
  title?: string | null;
  duration?: number;
}

/**
 * Invoke a Tauri command with proper error handling
 */
export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!window.tauriInvoke) {
    throw new Error('Tauri not initialized');
  }
  return window.tauriInvoke<T>(cmd, args);
}

/**
 * Check if Tauri is ready
 */
export function isTauriReady(): boolean {
  return !!window.tauriInvoke;
}

/**
 * Wait for Tauri to be ready
 */
export function waitForTauri(callback: () => void, maxAttempts = 50): void {
  let attempts = 0;
  const check = () => {
    if (window.tauriInvoke) {
      callback();
    } else if (attempts < maxAttempts) {
      attempts++;
      setTimeout(check, 100);
    } else {
      console.error('Tauri not available after', maxAttempts, 'attempts');
    }
  };
  check();
}

export { ToastOptions };
