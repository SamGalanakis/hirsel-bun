// Main entry point - sets up Tauri API access and Alpine components
import Alpine from 'alpinejs';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen, emit } from '@tauri-apps/api/event';

// Import Alpine components
import {
  appState,
  runList,
  runDetail,
  draftEditor,
  workerPanel,
  taskPanel,
  activityLog,
  chatPanel,
  directChat,
  permissionModal,
  notifications,
  tasksTab,
  evalTab,
  sheepClickerGame,
  settingsModal,
} from './lib/components';

// Initialize toast system (uses basecoat toaster)
import './lib/toast';

// Import Lucide icons
import {
  initLucideIcons,
  getIcon,
  getActionIcon,
  getTaskStatusIcon,
  getWorkerStatusIcon,
} from './lib/icons';

// Make Alpine available globally
(window as any).Alpine = Alpine;

// Export Tauri APIs globally
(window as any).tauriInvoke = invoke;
(window as any).tauriGetCurrentWindow = getCurrentWindow;

// Export icon utilities globally for Alpine templates
(window as any).getIcon = getIcon;
(window as any).getActionIcon = getActionIcon;
(window as any).getTaskStatusIcon = getTaskStatusIcon;
(window as any).getWorkerStatusIcon = getWorkerStatusIcon;

// Export Alpine components globally for x-data bindings
(window as any).appState = appState;
(window as any).runList = runList;
(window as any).runDetail = runDetail;
(window as any).draftEditor = draftEditor;
(window as any).workerPanel = workerPanel;
(window as any).taskPanel = taskPanel;
(window as any).activityLog = activityLog;
(window as any).chatPanel = chatPanel;
(window as any).directChat = directChat;
(window as any).permissionModal = permissionModal;
(window as any).notifications = notifications;
(window as any).tasksTab = tasksTab;
(window as any).evalTab = evalTab;
(window as any).sheepClickerGame = sheepClickerGame;
(window as any).settingsModal = settingsModal;

// Initialize Lucide icons
initLucideIcons();

// Start Alpine
Alpine.start();

console.log('[Hirsel] App initialized');

// =============================================================================
// MCP JavaScript Execution Handler
// =============================================================================
// This handler enables the tauri-plugin-mcp to execute JavaScript in the webview
// and return results. Without this, JS execution times out.

listen<string>('execute-js', async (event) => {
  const code = event.payload;

  try {
    // Execute the JavaScript code
    // Using Function() instead of eval() for slightly better security
    // The code runs in global scope
    const fn = new Function(`
      "use strict";
      return (async () => {
        ${code}
      })();
    `);

    const result = await fn();

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
  selectorType: 'id' | 'class' | 'tag' | 'text';
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
      case 'text':
        // Find element by text content
        const walker = document.createTreeWalker(
          document.body,
          NodeFilter.SHOW_TEXT,
          null
        );
        let node;
        while ((node = walker.nextNode())) {
          if (node.textContent?.includes(selectorValue)) {
            element = node.parentElement;
            break;
          }
        }
        break;
    }

    if (!element) {
      await emit('get-element-position-response', JSON.stringify({
        success: false,
        error: `Element not found: ${selectorType}="${selectorValue}"`,
      }));
      return;
    }

    const rect = element.getBoundingClientRect();
    const centerX = Math.round(rect.left + rect.width / 2);
    const centerY = Math.round(rect.top + rect.height / 2);

    // Click if requested
    if (shouldClick && element instanceof HTMLElement) {
      element.click();
    }

    await emit('get-element-position-response', JSON.stringify({
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
    }));
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : String(error);
    console.error('[MCP] Element position error:', errorMessage);
    await emit('get-element-position-response', JSON.stringify({
      success: false,
      error: errorMessage,
    }));
  }
});

// Handle send-text-to-element requests
interface SendTextRequest {
  selectorType: 'id' | 'class' | 'tag' | 'text';
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
      case 'text':
        // Find by text content
        const allElements = document.querySelectorAll('input, textarea, [contenteditable]');
        for (const el of allElements) {
          if (el.textContent?.includes(selectorValue) ||
              (el as HTMLInputElement).value?.includes(selectorValue)) {
            element = el;
            break;
          }
        }
        break;
    }

    if (!element) {
      await emit('send-text-to-element-response', JSON.stringify({
        success: false,
        error: `Element not found: ${selectorType}="${selectorValue}"`,
      }));
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
        await new Promise(resolve => setTimeout(resolve, delayMs));
      }
    }

    await emit('send-text-to-element-response', JSON.stringify({
      success: true,
      data: { textLength: text.length },
    }));
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : String(error);
    console.error('[MCP] Send text error:', errorMessage);
    await emit('send-text-to-element-response', JSON.stringify({
      success: false,
      error: errorMessage,
    }));
  }
});
