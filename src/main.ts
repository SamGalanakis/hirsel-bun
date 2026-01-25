import { invoke } from '@tauri-apps/api/core';
import { emit, listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
// Main entry point - sets up Tauri API access and Alpine components
import Alpine from 'alpinejs';

// Import Alpine components
import {
  activityLog,
  aiMessageStream,
  appState,
  chatPanel,
  debugPanel,
  directChat,
  draftEditor,
  notifications,
  permissionModal,
  projectSettings,
  projectSetup,
  runDetail,
  runList,
  settingsModal,
  sheepClickerGame,
  sortButton,
  sortToggle,
  specflowBoard,
  taskPanel,
  tasksTab,
  workerOutputViewer,
  workerPanel,
} from './lib/components';

// Initialize toast system (uses basecoat toaster)
import './lib/toast';

// Initialize dev logger (must be early to capture all console logs)
import { initDevLogger } from './lib/dev-logger';
initDevLogger();

// Initialize confirm dialog
import { initConfirmDialog } from './lib/confirm-dialog';

// Initialize shared data cache
import { dataCache } from './lib/data-cache';

// Import Lucide icons
import {
  getActionIcon,
  getIcon,
  getTaskStatusIcon,
  getWorkerStatusIcon,
  initLucideIcons,
} from './lib/icons';

// Import sheep avatar utilities
import { generateSheepSvg, getHatName, getWorkerSheepSvg } from './lib/sheep-avatar';

// Import keyboard shortcuts utilities
import { formatBinding, getShortcuts } from './lib/shortcuts';

// Import types to extend global Window interface
import './lib/types';

// Make Alpine available globally
window.Alpine = Alpine;

// Export Tauri APIs globally
window.tauriInvoke = invoke;
window.tauriGetCurrentWindow = getCurrentWindow;

// Export icon utilities globally for Alpine templates
window.getIcon = getIcon;
window.getActionIcon = getActionIcon;
window.getTaskStatusIcon = getTaskStatusIcon;
window.getWorkerStatusIcon = getWorkerStatusIcon;

// Export sheep avatar utilities globally for Alpine templates
window.generateSheepSvg = generateSheepSvg;
window.getWorkerSheepSvg = getWorkerSheepSvg;
window.getHatName = getHatName;

// Export keyboard shortcuts utilities globally for Alpine templates
window.getShortcuts = getShortcuts;
window.formatBinding = formatBinding;

// Export Alpine components globally for x-data bindings
window.appState = appState;
window.runList = runList;
window.runDetail = runDetail;
window.draftEditor = draftEditor;
window.workerPanel = workerPanel;
window.taskPanel = taskPanel;
window.activityLog = activityLog;
window.chatPanel = chatPanel;
window.directChat = directChat;
window.permissionModal = permissionModal;
window.notifications = notifications;
window.tasksTab = tasksTab;
window.sheepClickerGame = sheepClickerGame;
window.settingsModal = settingsModal;
window.aiMessageStream = aiMessageStream;
window.workerOutputViewer = workerOutputViewer;
window.sortToggle = sortToggle;
window.sortButton = sortButton;
window.debugPanel = debugPanel;
window.specflowBoard = specflowBoard;
window.projectSetup = projectSetup;
window.projectSettings = projectSettings;

// Initialize Lucide icons
initLucideIcons();

// Start Alpine
Alpine.start();

// Initialize confirm dialog after DOM is ready
initConfirmDialog();

// Start shared data cache polling
dataCache.start();

// Right-click to dismiss toasts
document.addEventListener('contextmenu', (e) => {
  const toast = (e.target as HTMLElement).closest('.toaster .toast');
  if (toast) {
    e.preventDefault();
    // Trigger the toast's dismiss by setting aria-hidden
    toast.setAttribute('aria-hidden', 'true');
    // Remove after animation
    setTimeout(() => toast.remove(), 300);
  }
});

console.log('[Hirsel] App initialized');

// =============================================================================
// MCP JavaScript Execution Handler
// =============================================================================
// This handler enables the tauri-plugin-mcp to execute JavaScript in the webview
// and return results. Without this, JS execution times out.

listen<string>('execute-js', async (event) => {
  const code = event.payload;
  const TIMEOUT_MS = 5000; // 5 second timeout

  try {
    // Execute the JavaScript code with timeout
    // Using Function() instead of eval() for slightly better security
    // The code runs in global scope
    const fn = new Function(`
      "use strict";
      return (async () => {
        ${code}
      })();
    `);

    // Race between execution and timeout
    const timeoutPromise = new Promise<never>((_, reject) => {
      setTimeout(() => reject(new Error('JavaScript execution timed out (5s limit)')), TIMEOUT_MS);
    });

    const result = await Promise.race([fn(), timeoutPromise]);

    // Determine the type of the result
    let resultType: string = typeof result;
    if (result === null) resultType = 'null';
    else if (Array.isArray(result)) resultType = 'array';
    else if (result instanceof Error) resultType = 'error';

    // Stringify the result
    let resultString: string;
    try {
      resultString = JSON.stringify(result) ?? String(result);
    } catch {
      resultString = String(result);
    }

    // Send back the response
    await emit('execute-js-response', {
      result: resultString,
      type: resultType,
    });
  } catch (error) {
    // Send back the error
    const errorMessage = error instanceof Error ? error.message : String(error);
    console.error('[MCP] JS execution error:', errorMessage);

    await emit('execute-js-response', {
      error: errorMessage,
    });
  }
});

// Handle DOM retrieval - responds to 'got-dom-content' with the full DOM
listen('got-dom-content', async () => {
  try {
    const html = document.documentElement.outerHTML;
    await emit('got-dom-content-response', html);
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : String(error);
    console.error('[MCP] DOM retrieval error:', errorMessage);
    await emit('got-dom-content-response', '');
  }
});

// Handle get-element-position requests
interface ElementPositionRequest {
  selectorType: 'id' | 'class' | 'tag' | 'text' | 'data-test';
  selectorValue: string;
  shouldClick?: boolean;
  rawCoordinates?: boolean;
}

listen<ElementPositionRequest>('get-element-position', async (event) => {
  const { selectorType, selectorValue, shouldClick, rawCoordinates } = event.payload;
  void rawCoordinates; // unused

  try {
    let element: Element | null = null;

    switch (selectorType) {
      case 'id':
        element = document.getElementById(selectorValue);
        break;
      case 'class':
        element = document.querySelector(`.${selectorValue}`);
        break;
      case 'tag':
        element = document.querySelector(selectorValue);
        break;
      case 'text': {
        // Find element by text content
        const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, null);
        let node: Node | null = walker.nextNode();
        while (node) {
          if (node.textContent?.includes(selectorValue)) {
            element = node.parentElement;
            break;
          }
          node = walker.nextNode();
        }
        break;
      }
      case 'data-test':
        element = document.querySelector(`[data-test="${selectorValue}"]`);
        break;
    }

    if (!element) {
      await emit(
        'get-element-position-response',
        JSON.stringify({
          success: false,
          error: `Element not found: ${selectorType}="${selectorValue}"`,
        }),
      );
      return;
    }

    const rect = element.getBoundingClientRect();
    const centerX = Math.round(rect.left + rect.width / 2);
    const centerY = Math.round(rect.top + rect.height / 2);

    // Click if requested
    if (shouldClick && element instanceof HTMLElement) {
      element.click();
    }

    await emit(
      'get-element-position-response',
      JSON.stringify({
        success: true,
        data: {
          x: centerX,
          y: centerY,
          width: rect.width,
          height: rect.height,
          top: rect.top,
          left: rect.left,
          clicked: shouldClick || false,
        },
      }),
    );
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : String(error);
    console.error('[MCP] Element position error:', errorMessage);
    await emit(
      'get-element-position-response',
      JSON.stringify({
        success: false,
        error: errorMessage,
      }),
    );
  }
});

// Handle send-text-to-element requests
interface SendTextRequest {
  selectorType: 'id' | 'class' | 'tag' | 'text' | 'data-test';
  selectorValue: string;
  text: string;
  delayMs?: number;
}

listen<SendTextRequest>('send-text-to-element', async (event) => {
  const { selectorType, selectorValue, text, delayMs = 20 } = event.payload;

  try {
    let element: Element | null = null;

    switch (selectorType) {
      case 'id':
        element = document.getElementById(selectorValue);
        break;
      case 'class':
        element = document.querySelector(`.${selectorValue}`);
        break;
      case 'tag':
        element = document.querySelector(selectorValue);
        break;
      case 'text': {
        // Find by text content
        const allElements = document.querySelectorAll('input, textarea, [contenteditable]');
        for (const el of allElements) {
          if (
            el.textContent?.includes(selectorValue) ||
            (el as HTMLInputElement).value?.includes(selectorValue)
          ) {
            element = el;
            break;
          }
        }
        break;
      }
      case 'data-test':
        element = document.querySelector(`[data-test="${selectorValue}"]`);
        break;
    }

    if (!element) {
      await emit(
        'send-text-to-element-response',
        JSON.stringify({
          success: false,
          error: `Element not found: ${selectorType}="${selectorValue}"`,
        }),
      );
      return;
    }

    // Focus the element
    if (element instanceof HTMLElement) {
      element.focus();
    }

    // Type text character by character with delay
    const inputEl = element as HTMLInputElement | HTMLTextAreaElement;
    const isInput = element.tagName === 'INPUT' || element.tagName === 'TEXTAREA';

    for (const char of text) {
      if (isInput) {
        inputEl.value += char;
        inputEl.dispatchEvent(new Event('input', { bubbles: true }));
      } else if (element instanceof HTMLElement && element.isContentEditable) {
        document.execCommand('insertText', false, char);
      }

      if (delayMs > 0) {
        await new Promise((resolve) => setTimeout(resolve, delayMs));
      }
    }

    await emit(
      'send-text-to-element-response',
      JSON.stringify({
        success: true,
        data: { textLength: text.length },
      }),
    );
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : String(error);
    console.error('[MCP] Send text error:', errorMessage);
    await emit(
      'send-text-to-element-response',
      JSON.stringify({
        success: false,
        error: errorMessage,
      }),
    );
  }
});
