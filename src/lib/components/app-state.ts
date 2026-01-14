/**
 * Main application state Alpine component
 */

import { formatElapsed, formatTimeRemaining, formatTimeShort } from '../utils/formatters';
import { getStatusBadgeClass, getStatusDotClass } from '../utils/status';
import { getTheme, setTheme, toggleTheme as themeToggle, isDarkTheme, THEMES, type ThemeId } from '../theme';
import type { RunDetail } from '../types';

/**
 * Main app state component
 */
export function appState() {
  return {
    // State
    selectedRun: null as string | null,
    currentRunDetail: null as RunDetail | null,
    aiChatOpen: false,
    notificationsOpen: false,
    showHelp: false,
    showSettings: false,
    unreadCount: 0,
    totalUnreadCount: 0,
    tasksDone: 0,
    tasksTotal: 0,
    isDarkTheme: isDarkTheme(),
    isMaximized: false,
    isWayland: false,
    sidebarCollapsed: false,
    _focusedRunIndex: -1,

    // UI toggles
    toggleSidebar() {
      this.sidebarCollapsed = !this.sidebarCollapsed;
    },

    toggleNotifications() {
      this.notificationsOpen = !this.notificationsOpen;
    },

    toggleAiChat() {
      this.aiChatOpen = !this.aiChatOpen;
    },

    // Window controls (for frameless window)
    async minimizeWindow() {
      try {
        if (window.tauriGetCurrentWindow) {
          await window.tauriGetCurrentWindow().minimize();
        }
      } catch (e) {
        console.error('minimize failed', e);
      }
    },

    async toggleMaximize() {
      try {
        if (window.tauriGetCurrentWindow) {
          const win = window.tauriGetCurrentWindow();
          this.isMaximized = await win.isMaximized();
          if (this.isMaximized) {
            await win.unmaximize();
          } else {
            await win.maximize();
          }
          this.isMaximized = !this.isMaximized;
        }
      } catch (e) {
        console.error('maximize failed', e);
      }
    },

    async closeWindow() {
      try {
        if (window.tauriGetCurrentWindow) {
          await window.tauriGetCurrentWindow().close();
        }
      } catch (e) {
        console.error('close failed', e);
      }
    },

    // Theme
    currentTheme: getTheme() as ThemeId,

    toggleTheme() {
      const newTheme = themeToggle();
      this.currentTheme = newTheme;
      this.isDarkTheme = isDarkTheme();
    },

    setTheme(themeId: ThemeId) {
      setTheme(themeId);
      this.currentTheme = themeId;
      this.isDarkTheme = THEMES[themeId].isDark;
    },

    initTheme() {
      const themeId = getTheme();
      setTheme(themeId);
      this.currentTheme = themeId;
      this.isDarkTheme = isDarkTheme();

      // Listen for theme changes from settings modal
      window.addEventListener('theme-changed', ((e: CustomEvent) => {
        this.currentTheme = e.detail.themeId;
        this.isDarkTheme = e.detail.theme.isDark;
      }) as EventListener);

      // Listen for close-settings event
      window.addEventListener('close-settings', () => {
        this.showSettings = false;
      });
    },

    // Formatting helpers (bound to this for templates)
    formatElapsed,
    formatTimeRemaining,
    formatTime: formatTimeShort,
    getStatusBadgeClass,
    getStatusDotClass,

    // Run actions (implemented in run-detail.ts)
    async pauseRun() {
      // Stub - use run-detail component actions
    },

    async resumeRun() {
      // Stub - use run-detail component actions
    },

    // Keyboard navigation
    navigateRuns(direction: number) {
      const runItems = document.querySelectorAll('.run-item');
      if (runItems.length === 0) return;

      this._focusedRunIndex += direction;

      // Clamp to valid range
      if (this._focusedRunIndex < 0) this._focusedRunIndex = 0;
      if (this._focusedRunIndex >= runItems.length) this._focusedRunIndex = runItems.length - 1;

      // Update visual focus
      runItems.forEach((item, idx) => {
        item.classList.toggle('ring-2', idx === this._focusedRunIndex);
        item.classList.toggle('ring-amber-500', idx === this._focusedRunIndex);
      });

      // Scroll into view
      runItems[this._focusedRunIndex]?.scrollIntoView({ block: 'nearest' });
    },

    selectFocusedRun() {
      const runItems = document.querySelectorAll('.run-item');
      if (this._focusedRunIndex >= 0 && this._focusedRunIndex < runItems.length) {
        (runItems[this._focusedRunIndex] as HTMLElement).click();
      }
    },

    async attachToWorker() {
      if (window.tauriInvoke) {
        try {
          const workers = await window.tauriInvoke<Array<{ name: string }>>('get_workers', {
            runName: this.selectedRun,
          });
          if (workers && workers.length > 0) {
            await window.tauriInvoke('attach_worker', {
              runName: this.selectedRun,
              workerName: workers[0].name,
            });
          }
        } catch (e) {
          const error = e as Error;
          window.toast.error(error.message || String(e), 'Failed to attach');
        }
      }
    },

    // Initialization
    async init() {
      this.initTheme();

      // Check if running on Wayland (minimize/maximize don't work there)
      if (window.tauriInvoke) {
        try {
          this.isWayland = await window.tauriInvoke<boolean>('is_wayland');
        } catch (e) {
          console.warn('Failed to check Wayland:', e);
        }
      }

      // Initialize window state
      if (window.tauriGetCurrentWindow) {
        try {
          this.isMaximized = await window.tauriGetCurrentWindow().isMaximized();
        } catch (e) {
          console.warn('Failed to get window state:', e);
        }
      }

      // Listen for run selection
      window.addEventListener('run-selected', async (e: Event) => {
        const customEvent = e as CustomEvent<string | null>;
        const runName = customEvent.detail;
        if (runName) {
          this.selectedRun = runName;
          try {
            const detail = await window.tauriInvoke<RunDetail>('get_run_detail', { runName });
            this.currentRunDetail = detail;
            this.tasksDone = detail.tasksDone || 0;
            this.tasksTotal = detail.tasksTotal || 0;
          } catch (err) {
            console.error('Failed to load run detail:', err);
            this.currentRunDetail = null;
          }
        } else {
          this.selectedRun = null;
          this.currentRunDetail = null;
          this.tasksDone = 0;
          this.tasksTotal = 0;
        }
      });

      // Keyboard shortcuts
      document.addEventListener('keydown', (e: KeyboardEvent) => {
        // Ignore if typing in input
        const target = e.target as HTMLElement;
        if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') return;

        switch (e.key) {
          case 'j':
            this.navigateRuns(1);
            break;
          case 'k':
            this.navigateRuns(-1);
            break;
          case 'Enter':
            this.selectFocusedRun();
            break;
          case 'a':
            if (this.selectedRun) this.attachToWorker();
            break;
          case 'c':
            // Switch to messages tab
            if (this.selectedRun) {
              window.dispatchEvent(new CustomEvent('switch-tab', { detail: 'messages' }));
            }
            break;
          case 'i':
            this.toggleAiChat();
            break;
          case 'm':
            // Switch to messages tab and focus input
            if (this.selectedRun) {
              window.dispatchEvent(new CustomEvent('switch-tab', { detail: 'messages' }));
              setTimeout(() => {
                const input = document.querySelector('.inline-chat-input') as HTMLElement;
                if (input) input.focus();
              }, 100);
            }
            break;
          case '?':
            this.showHelp = true;
            break;
          case 'Escape':
            this.showHelp = false;
            this.notificationsOpen = false;
            this.aiChatOpen = false;
            break;
          case 'p':
            if (this.selectedRun) this.pauseRun();
            break;
          case 'r':
            if (this.selectedRun) this.resumeRun();
            break;
          case 't':
            this.toggleTheme();
            break;
          case '[':
            this.toggleSidebar();
            break;
          case 'f':
            window.dispatchEvent(new CustomEvent('toggle-activity-fullscreen'));
            break;
        }
      });
    },
  };
}
