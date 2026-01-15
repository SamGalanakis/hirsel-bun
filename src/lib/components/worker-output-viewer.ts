/**
 * Worker Output Viewer - Shows AI model output for a worker
 *
 * This component displays the streaming output from a worker's AI model,
 * including text, tool calls, and thinking blocks. Uses the shared
 * AI message stream component for consistent rendering.
 */

import type { ChatMessage, ChatToolCall, WorkerEvent } from '../types';
import { getIcon, getToolKindIcon, getToolStatusIcon as getToolStatusIconSvg } from '../icons';
import { getWorkerEvents, createWorkerEventsPoller } from '../api';

/**
 * Worker Output Viewer Alpine component
 */
export function workerOutputViewer() {
  return {
    // State
    runName: null as string | null,
    workerName: null as string | null,
    loading: false,
    error: null as string | null,
    visible: false,

    // Messages
    messages: [] as ChatMessage[],
    streaming: false,
    _currentMessage: null as ChatMessage | null,
    _currentToolCalls: new Map() as Map<string, ChatToolCall>,
    _poller: null as { start: () => void; stop: () => void } | null,

    // Configuration
    showThinking: true,
    autoScroll: true,

    async init() {
      // Listen for show-worker-output events
      window.addEventListener('show-worker-output', ((e: CustomEvent<{ runName: string; workerName: string }>) => {
        this.show(e.detail.runName, e.detail.workerName);
      }) as EventListener);

      // Listen for hide-worker-output events
      window.addEventListener('hide-worker-output', () => {
        this.hide();
      });
    },

    destroy() {
      this.cleanup();
    },

    cleanup() {
      if (this._poller) {
        this._poller.stop();
        this._poller = null;
      }
    },

    async show(runName: string, workerName: string) {
      this.cleanup();
      this.messages = [];
      this._currentMessage = null;
      this._currentToolCalls.clear();
      this.runName = runName;
      this.workerName = workerName;
      this.loading = true;
      this.error = null;
      this.visible = true;

      try {
        // Load existing events from DB
        const response = await getWorkerEvents(runName, workerName);
        this.processEvents(response.events);

        // Start polling for real-time updates
        this._poller = createWorkerEventsPoller(runName, workerName, (events, isNew) => {
          if (isNew) {
            this.processEvents(events);
          }
        }, 200);
        this._poller.start();
      } catch (e) {
        const error = e as Error;
        this.error = error.message || 'Failed to load worker output';
        console.error('[WorkerOutput] Error:', e);
      } finally {
        this.loading = false;
      }
    },

    hide() {
      this.cleanup();
      this.visible = false;
      this.runName = null;
      this.workerName = null;
      this.messages = [];
    },

    /**
     * Process batch of events from DB
     */
    processEvents(events: WorkerEvent[]) {
      for (const event of events) {
        this.handleEvent(event);
      }
    },

    /**
     * Handle a single worker event
     */
    handleEvent(event: WorkerEvent) {
      switch (event.eventType) {
        case 'text':
          this.handleTextDelta(event.content || '');
          break;
        case 'thought':
          this.handleThinkingDelta(event.content || '');
          break;
        case 'tool_start':
          this.handleToolCallStart(
            event.toolCallId || crypto.randomUUID(),
            event.toolTitle || 'Unknown tool',
            event.toolKind || null
          );
          break;
        case 'tool_update':
          this.handleToolCallUpdate(
            event.toolCallId || '',
            event.toolStatus || 'completed',
            event.toolOutput || null
          );
          break;
      }
    },

    handleTextDelta(text: string) {
      if (!this._currentMessage) {
        this._currentMessage = {
          id: crypto.randomUUID(),
          role: 'assistant',
          content: '',
          toolCalls: [],
          timestamp: new Date(),
          streaming: true,
        };
        this.messages = [...this.messages, this._currentMessage];
        this.streaming = true;
      }

      this._currentMessage.content += text;
      const idx = this.messages.findIndex(m => m.id === this._currentMessage!.id);
      if (idx !== -1) {
        this._currentMessage = { ...this._currentMessage };
        this.messages[idx] = this._currentMessage;
        this.messages = [...this.messages];
      }
      if (this.autoScroll) this.scrollToBottom();
    },

    handleThinkingDelta(text: string) {
      if (!this._currentMessage) {
        this._currentMessage = {
          id: crypto.randomUUID(),
          role: 'assistant',
          content: '',
          thinking: '',
          toolCalls: [],
          timestamp: new Date(),
          streaming: true,
        };
        this.messages = [...this.messages, this._currentMessage];
        this.streaming = true;
      }

      this._currentMessage.thinking = (this._currentMessage.thinking || '') + text;
      const idx = this.messages.findIndex(m => m.id === this._currentMessage!.id);
      if (idx !== -1) {
        this._currentMessage = { ...this._currentMessage };
        this.messages[idx] = this._currentMessage;
        this.messages = [...this.messages];
      }
    },

    handleToolCallStart(id: string, title: string, kind: string | null) {
      const toolCall: ChatToolCall = {
        id,
        title,
        kind,
        status: 'in_progress',
        output: null,
      };

      this._currentToolCalls.set(id, toolCall);

      if (this._currentMessage) {
        this._currentMessage.toolCalls = Array.from(this._currentToolCalls.values());
        this.messages = [...this.messages];
      }
    },

    handleToolCallUpdate(id: string, status: string, output: string | null) {
      const toolCall = this._currentToolCalls.get(id);
      if (toolCall) {
        toolCall.status = status;
        toolCall.output = output;

        if (this._currentMessage) {
          this._currentMessage.toolCalls = Array.from(this._currentToolCalls.values());
          this.messages = [...this.messages];
        }
      }
    },

    handleMessageComplete() {
      if (this._currentMessage) {
        this._currentMessage.streaming = false;
        this.messages = [...this.messages];
      }

      this._currentMessage = null;
      this._currentToolCalls.clear();
      this.streaming = false;
    },

    scrollToBottom() {
      // @ts-expect-error Alpine.js $nextTick magic method
      this.$nextTick(() => {
        // @ts-expect-error Alpine.js $el magic property
        const container = (this.$el as HTMLElement).querySelector('.output-messages');
        if (container) {
          container.scrollTop = container.scrollHeight;
        }
      });
    },

    formatTime(date: Date): string {
      if (!(date instanceof Date)) {
        date = new Date(date);
      }
      return date.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
    },

    getIcon(name: string, size = 16): string {
      return getIcon(name, size);
    },

    getToolIcon(kind: string | null): string {
      return getToolKindIcon(kind, 14);
    },

    getToolStatusIcon(status: string): string {
      return getToolStatusIconSvg(status, 12);
    },

    getToolStatusClass(status: string): string {
      const classes: Record<string, string> = {
        pending: 'text-wool-500',
        in_progress: 'text-amber-400',
        completed: 'text-sage',
        failed: 'text-terra',
      };
      return classes[status] || 'text-wool-500';
    },
  };
}
