/**
 * Root application component with provider hierarchy
 */
import { type Component, createEffect, onCleanup } from 'solid-js';
import { emit, listen } from '@tauri-apps/api/event';

import { Layout } from './components/layout/Layout';
import { initLucideIcons } from './lib/icons';
import './lib/toast';
import { initDevLogger } from './lib/dev-logger';
import { initTheme } from './lib/theme';
import {
  AppProvider,
  ProjectProvider,
  RunsProvider,
  SelectionProvider,
  DeltaProvider,
} from './stores';

// Initialize dev logger early
initDevLogger();

const App: Component = () => {
  // Initialize on mount
  createEffect(() => {
    // Initialize theme
    initTheme();

    // Initialize Lucide icons
    initLucideIcons();


    // Right-click to dismiss toasts
    const contextMenuHandler = (e: MouseEvent) => {
      const toast = (e.target as HTMLElement).closest('.toaster .toast');
      if (toast) {
        e.preventDefault();
        toast.setAttribute('aria-hidden', 'true');
        setTimeout(() => toast.remove(), 300);
      }
    };
    document.addEventListener('contextmenu', contextMenuHandler);

    console.log('[Hirsel] App initialized');

    onCleanup(() => {
      document.removeEventListener('contextmenu', contextMenuHandler);
    });
  });

  // MCP JavaScript Execution Handler
  createEffect(() => {
    const TIMEOUT_MS = 5000;

    const unlistenExecuteJs = listen<string>('execute-js', async (event) => {
      const code = event.payload;

      try {
        const fn = new Function(`
          "use strict";
          return (async () => {
            ${code}
          })();
        `);

        const timeoutPromise = new Promise<never>((_, reject) => {
          setTimeout(
            () => reject(new Error('JavaScript execution timed out (5s limit)')),
            TIMEOUT_MS,
          );
        });

        const result = await Promise.race([fn(), timeoutPromise]);

        let resultType: string = typeof result;
        if (result === null) resultType = 'null';
        else if (Array.isArray(result)) resultType = 'array';
        else if (result instanceof Error) resultType = 'error';

        let resultString: string;
        try {
          resultString = JSON.stringify(result) ?? String(result);
        } catch {
          resultString = String(result);
        }

        await emit('execute-js-response', {
          result: resultString,
          type: resultType,
        });
      } catch (error) {
        const errorMessage = error instanceof Error ? error.message : String(error);
        console.error('[MCP] JS execution error:', errorMessage);
        await emit('execute-js-response', { error: errorMessage });
      }
    });

    const unlistenDomContent = listen('got-dom-content', async () => {
      try {
        const html = document.documentElement.outerHTML;
        await emit('got-dom-content-response', html);
      } catch (error) {
        console.error('[MCP] DOM retrieval error:', error);
        await emit('got-dom-content-response', '');
      }
    });

    interface ElementPositionRequest {
      selectorType: 'id' | 'class' | 'tag' | 'text' | 'data-test';
      selectorValue: string;
      shouldClick?: boolean;
      rawCoordinates?: boolean;
    }

    const unlistenElementPosition = listen<ElementPositionRequest>(
      'get-element-position',
      async (event) => {
        const { selectorType, selectorValue, shouldClick } = event.payload;

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
              const walker = document.createTreeWalker(
                document.body,
                NodeFilter.SHOW_TEXT,
                null,
              );
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
            JSON.stringify({ success: false, error: errorMessage }),
          );
        }
      },
    );

    interface SendTextRequest {
      selectorType: 'id' | 'class' | 'tag' | 'text' | 'data-test';
      selectorValue: string;
      text: string;
      delayMs?: number;
    }

    const unlistenSendText = listen<SendTextRequest>(
      'send-text-to-element',
      async (event) => {
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
              const allElements = document.querySelectorAll(
                'input, textarea, [contenteditable]',
              );
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

          if (element instanceof HTMLElement) {
            element.focus();
          }

          const inputEl = element as HTMLInputElement | HTMLTextAreaElement;
          const isInput = element.tagName === 'INPUT' || element.tagName === 'TEXTAREA';

          for (const char of text) {
            if (isInput) {
              inputEl.value += char;
              inputEl.dispatchEvent(new Event('input', { bubbles: true }));
            } else if (
              element instanceof HTMLElement &&
              element.isContentEditable
            ) {
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
            JSON.stringify({ success: false, error: errorMessage }),
          );
        }
      },
    );

    onCleanup(() => {
      unlistenExecuteJs.then((fn) => fn());
      unlistenDomContent.then((fn) => fn());
      unlistenElementPosition.then((fn) => fn());
      unlistenSendText.then((fn) => fn());
    });
  });

  return (
    <AppProvider>
      <ProjectProvider>
        <RunsProvider>
          <SelectionProvider>
            <DeltaProvider>
              <Layout />
            </DeltaProvider>
          </SelectionProvider>
        </RunsProvider>
      </ProjectProvider>
    </AppProvider>
  );
};

export default App;
