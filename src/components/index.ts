/**
 * Hirsel Alpine.js Components
 *
 * Export all component factories and register them for use in templates.
 */

export { runList, registerRunListComponent } from './run-list';
export { runDetail, registerRunDetailComponent } from './run-detail';
export { statusBar, registerStatusBar } from './status-bar';
export type { StatusBarState, StatusBarComponent } from './status-bar';

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
  import('./status-bar').then(({ registerStatusBar }) => {
    registerStatusBar();
  });
}

// Auto-register on import
registerAllComponents();
