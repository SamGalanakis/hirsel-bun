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
    sidebarCollapsed: false,
    _focusedRunIndex: -1,
    _eventCleanups: [] as (() => void)[],

    // Attach picker state
    attachPickerOpen: false,
    attachPickerWorkers: [] as Array<{ name: string; status: string }>,
    attachPickerEvals: [] as Array<{ id: number; evalName: string; status: string }>,
    attachPickerLoading: false,

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
      const themeChangedHandler = ((e: CustomEvent) => {
        this.currentTheme = e.detail.themeId;
        this.isDarkTheme = e.detail.theme.isDark;
      }) as EventListener;
      window.addEventListener('theme-changed', themeChangedHandler);
      this._eventCleanups.push(() => window.removeEventListener('theme-changed', themeChangedHandler));

      // Listen for close-settings event
      const closeSettingsHandler = () => {
        this.showSettings = false;
      };
      window.addEventListener('close-settings', closeSettingsHandler);
      this._eventCleanups.push(() => window.removeEventListener('close-settings', closeSettingsHandler));
    },

    // Formatting helpers (bound to this for templates)
    formatElapsed,
    formatTimeRemaining,
    formatTime: formatTimeShort,
    getStatusBadgeClass,
    getStatusDotClass,

    // Run actions
    async pauseRun() {
      if (!this.selectedRun || !window.tauriInvoke) return;
      try {
        await window.tauriInvoke('pause_run', { runName: this.selectedRun });
      } catch (e) {
        console.error('Failed to pause run:', e);
      }
    },

    async resumeRun() {
      if (!this.selectedRun || !window.tauriInvoke) return;
      try {
        await window.tauriInvoke('resume_run', { runName: this.selectedRun });
      } catch (e) {
        console.error('Failed to resume run:', e);
      }
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

    async handleAttach() {
      if (!this.selectedRun || !window.tauriInvoke) return;

      // Check if we're on evals tab with a selected eval
      const runDetailEl = document.querySelector('[x-data*="runDetail"]') as HTMLElement & { _x_dataStack?: Array<{ selectedEval: { evalName: string } | null; activeTab: string }> };
      if (runDetailEl?._x_dataStack?.[0]) {
        const runDetailData = runDetailEl._x_dataStack[0];
        if (runDetailData.activeTab === 'evals' && runDetailData.selectedEval) {
          window.dispatchEvent(new CustomEvent('show-worker-output', {
            detail: {
              runName: this.selectedRun,
              workerName: runDetailData.selectedEval.evalName,
            },
          }));
          return;
        }
      }

      // Check if we're on overview with a highlighted or selected worker in the panel
      const workerPanelEl = document.querySelector('[x-data*="workerPanel"]') as HTMLElement & { _x_dataStack?: Array<{ highlightedWorker: string | null; selectedWorker: { name: string } | null }> };
      if (workerPanelEl?._x_dataStack?.[0]) {
        const panelData = workerPanelEl._x_dataStack[0];
        // Prefer highlighted worker (single-click selection), then fall back to modal selection
        const workerName = panelData.highlightedWorker || panelData.selectedWorker?.name;
        if (workerName) {
          window.dispatchEvent(new CustomEvent('show-worker-output', {
            detail: {
              runName: this.selectedRun,
              workerName,
            },
          }));
          return;
        }
      }

      // Neither selected - show the attach picker
      await this.openAttachPicker();
    },

    async openAttachPicker() {
      if (!this.selectedRun || !window.tauriInvoke) return;

      try {
        const [workers, evals] = await Promise.all([
          window.tauriInvoke<Array<{ name: string; status: string }>>('get_workers', { runName: this.selectedRun }),
          window.tauriInvoke<Array<{ id: number; evalName: string; status: string }>>('get_evals', { runName: this.selectedRun }),
        ]);

        const workerList = workers || [];
        const evalList = evals || [];
        const totalChoices = workerList.length + evalList.length;

        // If only one choice, attach directly without showing picker
        if (totalChoices === 1) {
          if (workerList.length === 1) {
            this.attachToTarget('worker', workerList[0].name);
          } else if (evalList.length === 1) {
            this.attachToTarget('eval', evalList[0].evalName);
          }
          return;
        }

        // Multiple choices - show picker
        this.attachPickerWorkers = workerList;
        this.attachPickerEvals = evalList;
        this.attachPickerOpen = true;
      } catch (e) {
        console.error('Failed to load attach picker data:', e);
        this.attachPickerWorkers = [];
        this.attachPickerEvals = [];
      }
    },

    closeAttachPicker() {
      this.attachPickerOpen = false;
      this.attachPickerWorkers = [];
      this.attachPickerEvals = [];
    },

    attachToTarget(type: 'worker' | 'eval', name: string) {
      if (!this.selectedRun) return;
      window.dispatchEvent(new CustomEvent('show-worker-output', {
        detail: {
          runName: this.selectedRun,
          workerName: name,
        },
      }));
      this.closeAttachPicker();
    },

    // Initialization
    async init() {
      this.initTheme();

      // Listen for run selection
      const runSelectedHandler = async (e: Event) => {
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
      };
      window.addEventListener('run-selected', runSelectedHandler);
      this._eventCleanups.push(() => window.removeEventListener('run-selected', runSelectedHandler));

      // Keyboard shortcuts
      const keydownHandler = (e: KeyboardEvent) => {
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
            if (this.selectedRun && !this.attachPickerOpen) this.handleAttach();
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
      };
      document.addEventListener('keydown', keydownHandler);
      this._eventCleanups.push(() => document.removeEventListener('keydown', keydownHandler));
    },

    destroy() {
      this._eventCleanups.forEach(fn => fn());
      this._eventCleanups = [];
    },
  };
}
