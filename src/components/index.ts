/**
 * Hirsel Alpine.js Components
 *
 * Export all component factories and register them for use in templates.
 */

export { runList, registerRunListComponent } from './run-list';
export { runDetail, registerRunDetailComponent } from './run-detail';
export { activityLog, registerActivityLogComponent } from './activity-log';
export { taskPanel, registerTaskPanelComponent } from './task-panel';
export { statusBar, registerStatusBar } from './status-bar';
export { workerPanel, registerWorkerPanelComponent } from './worker-panel';
export type { StatusBarState, StatusBarComponent } from './status-bar';
export type { ActivityLogData } from './activity-log';
export type { TaskPanelData } from './task-panel';
export type { WorkerPanelData } from './worker-panel';

// Re-export utility functions for use outside components
export {
  getStatusDotClass,
  formatElapsed,
  formatTimeRemaining,
  getTimeProgress,
  isTimeWarning,
  getStatusText,
} from './status-bar';

/**
 * Register all components with the global window object for Alpine.js
 */
export function registerAllComponents(): void {
  if (typeof window === 'undefined') return;

  // Import and register each component
  // This makes them available as x-data="componentName()"
  import('./run-list').then(({ registerRunListComponent }) => {
    registerRunListComponent();
  });
  import('./run-detail').then(({ registerRunDetailComponent }) => {
    registerRunDetailComponent();
  });
  import('./activity-log').then(({ registerActivityLogComponent }) => {
    registerActivityLogComponent();
  });
  import('./task-panel').then(({ registerTaskPanelComponent }) => {
    registerTaskPanelComponent();
  });
  import('./status-bar').then(({ registerStatusBar }) => {
    registerStatusBar();
  });
  import('./worker-panel').then(({ registerWorkerPanelComponent }) => {
    registerWorkerPanelComponent();
  });
}

// Auto-register on import
registerAllComponents();
