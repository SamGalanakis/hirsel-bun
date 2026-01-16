/**
 * Worker Output Viewer - Shows AI model output for a worker
 *
 * This component displays the streaming output from a worker's AI model,
 * including text, tool calls, and thinking blocks in chronological order.
 */

import type { ChatToolCall, WorkerEvent } from '../types';
import { getIcon, getToolKindIcon, getToolStatusIcon as getToolStatusIconSvg } from '../icons';
import { getWorkerEvents, createWorkerEventsPoller } from '../api';

/** A chunk of content in the output stream */
interface OutputChunk {
  id: string;
  type: 'text' | 'thinking' | 'tool';
  content?: string;
  tool?: ChatToolCall;
}

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

    // Output chunks in chronological order
    chunks: [] as OutputChunk[],
    streaming: false,
    _lastChunkType: null as 'text' | 'thinking' | null,
    _toolsById: new Map() as Map<string, OutputChunk>,
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
      this.chunks = [];
      this._lastChunkType = null;
      this._toolsById.clear();
      this.runName = runName;
      this.workerName = workerName;
      this.loading = true;
      this.error = null;
      this.visible = true;

      try {
        // Load existing events from DB
        const response = await getWorkerEvents(runName, workerName);
        console.log('[WorkerOutput] Initial load:', {
          eventCount: response.events.length,
          workerStatus: response.workerStatus,
          firstEvent: response.events[0]
        });
        this.processEvents(response.events);
        console.log('[WorkerOutput] After processEvents, chunks:', this.chunks.length);
        this.updateStreamingState(response.workerStatus);

        // Start polling for real-time updates
        this._poller = createWorkerEventsPoller(runName, workerName, (events, isNew, workerStatus) => {
          if (isNew) {
            this.processEvents(events);
          }
          this.updateStreamingState(workerStatus);
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
      this.chunks = [];
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
      console.log('[WorkerOutput] handleEvent:', event.eventType, event);
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
      // If last chunk was text, append to it; otherwise create new text chunk
      if (this._lastChunkType === 'text' && this.chunks.length > 0) {
        const lastChunk = this.chunks[this.chunks.length - 1];
        if (lastChunk.type === 'text') {
          lastChunk.content = (lastChunk.content || '') + text;
          this.chunks = [...this.chunks]; // Trigger reactivity
          if (this.autoScroll) this.scrollToBottom();
          return;
        }
      }

      // Create new text chunk
      const chunk: OutputChunk = {
        id: crypto.randomUUID(),
        type: 'text',
        content: text,
      };
      this.chunks = [...this.chunks, chunk];
      this._lastChunkType = 'text';
      this.streaming = true;
      if (this.autoScroll) this.scrollToBottom();
    },

    handleThinkingDelta(text: string) {
      // If last chunk was thinking, append to it; otherwise create new thinking chunk
      if (this._lastChunkType === 'thinking' && this.chunks.length > 0) {
        const lastChunk = this.chunks[this.chunks.length - 1];
        if (lastChunk.type === 'thinking') {
          lastChunk.content = (lastChunk.content || '') + text;
          this.chunks = [...this.chunks]; // Trigger reactivity
          return;
        }
      }

      // Create new thinking chunk
      const chunk: OutputChunk = {
        id: crypto.randomUUID(),
        type: 'thinking',
        content: text,
      };
      this.chunks = [...this.chunks, chunk];
      this._lastChunkType = 'thinking';
    },

    handleToolCallStart(id: string, title: string, kind: string | null) {
      // Tool call breaks the text/thinking stream
      this._lastChunkType = null;

      const toolCall: ChatToolCall = {
        id,
        title,
        kind,
        status: 'in_progress',
        output: null,
      };

      const chunk: OutputChunk = {
        id,
        type: 'tool',
        tool: toolCall,
      };

      this._toolsById.set(id, chunk);
      this.chunks = [...this.chunks, chunk];
      if (this.autoScroll) this.scrollToBottom();
    },

    handleToolCallUpdate(id: string, status: string, output: string | null) {
      const chunk = this._toolsById.get(id);
      if (chunk && chunk.tool) {
        chunk.tool.status = status;
        chunk.tool.output = output;
        this.chunks = [...this.chunks]; // Trigger reactivity
      }
    },

    handleMessageComplete() {
      this._lastChunkType = null;
      this.streaming = false;
    },

    /**
     * Update streaming state based on worker status
     * Only show streaming when worker is actively working
     */
    updateStreamingState(workerStatus: string | null) {
      // Worker is streaming only when actively working
      this.streaming = workerStatus === 'working';
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
