/**
 * Keyboard shortcuts management module
 *
 * Provides configurable keyboard shortcuts with localStorage persistence,
 * conflict detection, and event-driven updates.
 */

const STORAGE_KEY = 'hirsel-shortcuts';

// =============================================================================
// Types
// =============================================================================

export type ShortcutAction =
  | 'navigate-up'
  | 'navigate-down'
  | 'select-run'
  | 'fullscreen'
  | 'attach'
  | 'pause'
  | 'resume'
  | 'switch-chat'
  | 'focus-message'
  | 'toggle-ai'
  | 'sheep-game'
  | 'close-panel';

export type ShortcutCategory = 'navigation' | 'run-controls' | 'communication' | 'other';

export interface ShortcutModifiers {
  ctrl?: boolean;
  alt?: boolean;
  shift?: boolean;
  meta?: boolean;
}

export interface ShortcutBinding {
  key: string;
  modifiers?: ShortcutModifiers;
}

export interface ShortcutConfig {
  action: ShortcutAction;
  label: string;
  category: ShortcutCategory;
  binding: ShortcutBinding;
  defaultBinding: ShortcutBinding;
}

// =============================================================================
// Default Shortcuts
// =============================================================================

export const DEFAULT_SHORTCUTS: ShortcutConfig[] = [
  // Navigation
  { action: 'navigate-up', label: 'Navigate runs up', category: 'navigation', binding: { key: 'k' }, defaultBinding: { key: 'k' } },
  { action: 'navigate-down', label: 'Navigate runs down', category: 'navigation', binding: { key: 'j' }, defaultBinding: { key: 'j' } },
  { action: 'select-run', label: 'Select run', category: 'navigation', binding: { key: 'Enter' }, defaultBinding: { key: 'Enter' } },
  { action: 'fullscreen', label: 'Fullscreen activity', category: 'navigation', binding: { key: 'f' }, defaultBinding: { key: 'f' } },

  // Run controls
  { action: 'attach', label: 'Attach to worker', category: 'run-controls', binding: { key: 'a' }, defaultBinding: { key: 'a' } },
  { action: 'pause', label: 'Pause run', category: 'run-controls', binding: { key: 'p' }, defaultBinding: { key: 'p' } },
  { action: 'resume', label: 'Resume run', category: 'run-controls', binding: { key: 'r' }, defaultBinding: { key: 'r' } },

  // Communication
  { action: 'switch-chat', label: 'Switch to chat', category: 'communication', binding: { key: 'c' }, defaultBinding: { key: 'c' } },
  { action: 'focus-message', label: 'Focus message input', category: 'communication', binding: { key: 'm' }, defaultBinding: { key: 'm' } },
  { action: 'toggle-ai', label: 'Toggle AI sidebar', category: 'communication', binding: { key: 'i' }, defaultBinding: { key: 'i' } },

  // Other
  { action: 'sheep-game', label: 'Sheep clicker', category: 'other', binding: { key: 'g' }, defaultBinding: { key: 'g' } },
  { action: 'close-panel', label: 'Close panels', category: 'other', binding: { key: 'Escape' }, defaultBinding: { key: 'Escape' } },
];

// =============================================================================
// Storage Functions
// =============================================================================

/**
 * Load shortcuts from localStorage, merging with defaults
 */
export function getShortcuts(): ShortcutConfig[] {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (!stored) {
    return DEFAULT_SHORTCUTS.map(s => ({ ...s, binding: { ...s.defaultBinding } }));
  }

  try {
    const savedBindings: Record<string, ShortcutBinding> = JSON.parse(stored);

    // Merge saved bindings with defaults
    return DEFAULT_SHORTCUTS.map(shortcut => {
      const savedBinding = savedBindings[shortcut.action];
      return {
        ...shortcut,
        binding: savedBinding ? { ...savedBinding } : { ...shortcut.defaultBinding },
      };
    });
  } catch (e) {
    console.warn('Failed to load shortcuts from localStorage:', e);
    return DEFAULT_SHORTCUTS.map(s => ({ ...s, binding: { ...s.defaultBinding } }));
  }
}

/**
 * Save shortcuts to localStorage and dispatch update event
 */
export function saveShortcuts(shortcuts: ShortcutConfig[]): void {
  const bindings: Record<string, ShortcutBinding> = {};
  for (const shortcut of shortcuts) {
    bindings[shortcut.action] = shortcut.binding;
  }

  localStorage.setItem(STORAGE_KEY, JSON.stringify(bindings));
  window.dispatchEvent(new CustomEvent('shortcuts-changed'));
}

/**
 * Reset all shortcuts to defaults
 */
export function resetShortcuts(): void {
  localStorage.removeItem(STORAGE_KEY);
  window.dispatchEvent(new CustomEvent('shortcuts-changed'));
}

// =============================================================================
// Matching Functions
// =============================================================================

/**
 * Check if a KeyboardEvent matches a shortcut binding
 */
export function matchesBinding(e: KeyboardEvent, binding: ShortcutBinding): boolean {
  // Normalize key comparison
  const eventKey = e.key;
  const bindingKey = binding.key;

  // Handle special cases for key matching
  if (eventKey !== bindingKey) {
    return false;
  }

  // Check modifiers
  const mods = binding.modifiers || {};
  if (!!mods.ctrl !== e.ctrlKey) return false;
  if (!!mods.alt !== e.altKey) return false;
  if (!!mods.shift !== e.shiftKey) return false;
  if (!!mods.meta !== e.metaKey) return false;

  return true;
}

/**
 * Find which action a KeyboardEvent matches, if any
 */
export function findMatchingAction(e: KeyboardEvent, shortcuts: ShortcutConfig[]): ShortcutAction | null {
  for (const shortcut of shortcuts) {
    if (matchesBinding(e, shortcut.binding)) {
      return shortcut.action;
    }
  }
  return null;
}

// =============================================================================
// Conflict Detection
// =============================================================================

/**
 * Check if two bindings are equal
 */
export function bindingsEqual(a: ShortcutBinding, b: ShortcutBinding): boolean {
  if (a.key !== b.key) return false;

  const aMods = a.modifiers || {};
  const bMods = b.modifiers || {};

  return (
    !!aMods.ctrl === !!bMods.ctrl &&
    !!aMods.alt === !!bMods.alt &&
    !!aMods.shift === !!bMods.shift &&
    !!aMods.meta === !!bMods.meta
  );
}

/**
 * Find conflicts when assigning a binding to an action
 * Returns the label of the conflicting shortcut, or null if no conflict
 */
export function findConflict(
  action: ShortcutAction,
  binding: ShortcutBinding,
  shortcuts: ShortcutConfig[]
): string | null {
  for (const shortcut of shortcuts) {
    if (shortcut.action === action) continue;
    if (bindingsEqual(shortcut.binding, binding)) {
      return shortcut.label;
    }
  }
  return null;
}

// =============================================================================
// Display Functions
// =============================================================================

/**
 * Format a binding for display (e.g., "Ctrl + K")
 */
export function formatBinding(binding: ShortcutBinding): string {
  const parts: string[] = [];
  const mods = binding.modifiers || {};

  if (mods.ctrl) parts.push('Ctrl');
  if (mods.alt) parts.push('Alt');
  if (mods.shift) parts.push('Shift');
  if (mods.meta) parts.push('Cmd');

  // Format special keys nicely
  let keyDisplay = binding.key;
  switch (binding.key) {
    case ' ':
      keyDisplay = 'Space';
      break;
    case 'ArrowUp':
      keyDisplay = '\u2191';
      break;
    case 'ArrowDown':
      keyDisplay = '\u2193';
      break;
    case 'ArrowLeft':
      keyDisplay = '\u2190';
      break;
    case 'ArrowRight':
      keyDisplay = '\u2192';
      break;
    case 'Escape':
      keyDisplay = 'Esc';
      break;
    case 'Enter':
      keyDisplay = 'Enter';
      break;
    default:
      // Capitalize single letters
      if (keyDisplay.length === 1) {
        keyDisplay = keyDisplay.toUpperCase();
      }
  }

  parts.push(keyDisplay);

  return parts.join(' + ');
}

/**
 * Create a binding from a KeyboardEvent
 */
export function bindingFromEvent(e: KeyboardEvent): ShortcutBinding {
  const binding: ShortcutBinding = { key: e.key };

  if (e.ctrlKey || e.altKey || e.shiftKey || e.metaKey) {
    binding.modifiers = {};
    if (e.ctrlKey) binding.modifiers.ctrl = true;
    if (e.altKey) binding.modifiers.alt = true;
    if (e.shiftKey) binding.modifiers.shift = true;
    if (e.metaKey) binding.modifiers.meta = true;
  }

  return binding;
}

/**
 * Get shortcuts grouped by category
 */
export function getShortcutsByCategory(shortcuts: ShortcutConfig[]): Record<ShortcutCategory, ShortcutConfig[]> {
  const grouped: Record<ShortcutCategory, ShortcutConfig[]> = {
    navigation: [],
    'run-controls': [],
    communication: [],
    other: [],
  };

  for (const shortcut of shortcuts) {
    grouped[shortcut.category].push(shortcut);
  }

  return grouped;
}

/**
 * Get category display name
 */
export function getCategoryLabel(category: ShortcutCategory): string {
  switch (category) {
    case 'navigation':
      return 'Navigation';
    case 'run-controls':
      return 'Run Controls';
    case 'communication':
      return 'Communication';
    case 'other':
      return 'Other';
  }
}
