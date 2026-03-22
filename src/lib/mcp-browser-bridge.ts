import { emit, listen } from '@tauri-apps/api/event';
import { createEffect, onCleanup } from 'solid-js';

interface ElementPositionRequest {
  selectorType: 'id' | 'class' | 'tag' | 'text' | 'data-test';
  selectorValue: string;
  shouldClick?: boolean;
}

interface SendTextRequest {
  selectorType: 'id' | 'class' | 'tag' | 'text' | 'data-test';
  selectorValue: string;
  text: string;
  delayMs?: number;
}

export function useMcpBrowserBridge() {
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
        const resultType =
          result === null ? 'null' : Array.isArray(result) ? 'array' : typeof result;

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
        await emit('got-dom-content-response', document.documentElement.outerHTML);
      } catch (error) {
        console.error('[MCP] DOM retrieval error:', error);
        await emit('got-dom-content-response', '');
      }
    });

    const unlistenElementPosition = listen<ElementPositionRequest>(
      'get-element-position',
      async (event) => {
        const { selectorType, selectorValue, shouldClick } = event.payload;

        try {
          const element = findElement(selectorType, selectorValue);
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
          if (shouldClick && element instanceof HTMLElement) {
            element.click();
          }

          await emit(
            'get-element-position-response',
            JSON.stringify({
              success: true,
              data: {
                x: Math.round(rect.left + rect.width / 2),
                y: Math.round(rect.top + rect.height / 2),
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

    const unlistenSendText = listen<SendTextRequest>('send-text-to-element', async (event) => {
      const { selectorType, selectorValue, text, delayMs = 20 } = event.payload;

      try {
        const element = findElement(selectorType, selectorValue, true);
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
          JSON.stringify({ success: false, error: errorMessage }),
        );
      }
    });

    onCleanup(() => {
      unlistenExecuteJs.then((fn) => fn());
      unlistenDomContent.then((fn) => fn());
      unlistenElementPosition.then((fn) => fn());
      unlistenSendText.then((fn) => fn());
    });
  });
}

function findElement(
  selectorType: ElementPositionRequest['selectorType'],
  selectorValue: string,
  inputsOnly = false,
): Element | null {
  if (inputsOnly && selectorType === 'text') {
    const allElements = document.querySelectorAll('input, textarea, [contenteditable]');
    for (const el of allElements) {
      if (
        el.textContent?.includes(selectorValue) ||
        (el as HTMLInputElement).value?.includes(selectorValue)
      ) {
        return el;
      }
    }
    return null;
  }

  switch (selectorType) {
    case 'id':
      return document.getElementById(selectorValue);
    case 'class':
      return document.querySelector(`.${selectorValue}`);
    case 'tag':
      return document.querySelector(selectorValue);
    case 'data-test':
      return document.querySelector(`[data-test="${selectorValue}"]`);
    case 'text': {
      const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, null);
      let node: Node | null = walker.nextNode();
      while (node) {
        if (node.textContent?.includes(selectorValue)) {
          return node.parentElement;
        }
        node = walker.nextNode();
      }
      return null;
    }
  }
}
