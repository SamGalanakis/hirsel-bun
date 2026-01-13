/**
 * Hirsel Alpine.js Components
 *
 * Export all component factories and register them for use in templates.
 */

export { runList, registerRunListComponent } from './run-list';

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
}

// Auto-register on import
registerAllComponents();
