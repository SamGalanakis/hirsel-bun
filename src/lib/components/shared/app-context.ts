/**
 * App context utilities
 *
 * Provides safe access to the global app state from nested components.
 * Uses Alpine.js's $root to access the root component data.
 */

export interface AppState {
  selectedRun: string | null;
  selectedDraft: string | null;
  showSettings: boolean;
  showHelp: boolean;
  aiChatOpen: boolean;
  sidebarCollapsed: boolean;
  isDarkTheme: boolean;
}

/**
 * Get the app state from an Alpine component's context.
 *
 * This is safer than traversing _x_dataStack as it uses Alpine's
 * built-in $root magic property which always points to the root.
 *
 * Usage in component:
 *   const appState = getAppState(this);
 *   if (appState?.selectedRun) { ... }
 */
export function getAppState(component: unknown): AppState | null {
  // The component should have access to Alpine's $root magic
  const comp = component as Record<string, unknown>;

  // Try the $root property (available in Alpine 3.x)
  if (comp.$root && typeof comp.$root === 'object') {
    return comp.$root as AppState;
  }

  // Fallback: try to find appState in window
  if (typeof window !== 'undefined') {
    const win = window as unknown as Record<string, unknown>;
    if (typeof win.appState === 'function') {
      // appState is a function that returns the state
      return (win.appState as () => AppState)();
    }
  }

  return null;
}

/**
 * Get the currently selected run name
 */
export function getSelectedRun(component: unknown): string | null {
  return getAppState(component)?.selectedRun ?? null;
}

/**
 * Get the currently selected draft name
 */
export function getSelectedDraft(component: unknown): string | null {
  return getAppState(component)?.selectedDraft ?? null;
}

/**
 * Check if we're currently viewing a draft
 */
export function isViewingDraft(component: unknown): boolean {
  return getAppState(component)?.selectedDraft !== null;
}
