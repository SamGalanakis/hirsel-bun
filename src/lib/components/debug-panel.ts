/**
 * Debug Panel Component
 *
 * Provides process monitoring and debug utilities.
 * Only active in development mode.
 * Toggle with Ctrl+D.
 */

declare const lucide:
  | {
      createIcons: (options?: { inTemplates?: boolean }) => void;
    }
  | undefined;

declare global {
  interface Window {
    tauriInvoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
  }
}

interface AlpineComponent {
  $watch: (property: string, callback: (value: boolean) => void) => void;
}

interface ProcessCounts {
  claude: number;
  acp: number;
  node: number;
  details: string;
}

interface DebugPanelData {
  isOpen: boolean;
  counts: ProcessCounts;
  pollInterval: ReturnType<typeof setInterval> | null;
  isDevMode: boolean;
}

interface DebugPanelMethods {
  init(): void;
  toggle(): void;
  startPolling(): void;
  stopPolling(): void;
  refreshCounts(): Promise<void>;
  killOrphanedProcesses(): Promise<void>;
  destroy(): void;
}

export function debugPanel(): DebugPanelData & DebugPanelMethods & Partial<AlpineComponent> {
  return {
    isOpen: false,
    counts: {
      claude: 0,
      acp: 0,
      node: 0,
      details: '',
    } as ProcessCounts,
    pollInterval: null as ReturnType<typeof setInterval> | null,
    isDevMode: (import.meta as { env?: { DEV?: boolean } }).env?.DEV ?? false,

    init(this: AlpineComponent & ReturnType<typeof debugPanel>) {
      // Only initialize in dev mode
      if (!this.isDevMode) {
        return;
      }

      // Listen for Ctrl+D to toggle
      window.addEventListener('keydown', (e: KeyboardEvent) => {
        if (e.ctrlKey && e.key.toLowerCase() === 'd') {
          e.preventDefault();
          this.toggle();
        }
      });

      // Start polling when opened
      this.$watch('isOpen', (open: boolean) => {
        if (open) {
          this.refreshCounts();
          this.startPolling();
          // Refresh lucide icons for the panel
          setTimeout(() => {
            if (typeof lucide !== 'undefined') {
              lucide.createIcons({ inTemplates: true });
            }
          }, 50);
        } else {
          this.stopPolling();
        }
      });
    },

    toggle() {
      if (!this.isDevMode) {
        return;
      }
      this.isOpen = !this.isOpen;
    },

    startPolling() {
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
      }
      this.pollInterval = setInterval(() => {
        this.refreshCounts();
      }, 2000);
    },

    stopPolling() {
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }
    },

    async refreshCounts() {
      try {
        const result = await window.tauriInvoke<ProcessCounts>('get_process_counts');
        this.counts = result;
      } catch (err) {
        console.error('[DebugPanel] Failed to get process counts:', err);
      }
    },

    async killOrphanedProcesses() {
      try {
        const result = await window.tauriInvoke<{ killed: number }>('kill_orphaned_acp_processes');
        if (result.killed > 0) {
          window.toast?.success(`Killed ${result.killed} orphaned claude-code-acp processes`);
        } else {
          window.toast?.info('No orphaned processes found');
        }
        // Refresh after a short delay
        setTimeout(() => this.refreshCounts(), 500);
      } catch (err) {
        console.error('[DebugPanel] Failed to kill processes:', err);
        window.toast?.error('Failed to kill orphaned processes');
        setTimeout(() => this.refreshCounts(), 500);
      }
    },

    destroy() {
      this.stopPolling();
    },
  };
}
