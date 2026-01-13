// Main entry point - sets up Tauri API access
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

// Export Tauri APIs globally so inline scripts can use them
(window as any).tauriInvoke = invoke;
(window as any).tauriGetCurrentWindow = getCurrentWindow;

console.log('[Hirsel] Tauri APIs loaded');
