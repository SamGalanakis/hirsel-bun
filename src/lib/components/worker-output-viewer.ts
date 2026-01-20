/**
 * Worker Output Viewer - Shows AI model output for a worker
 *
 * This component displays the streaming output from a worker's AI model,
 * including text, tool calls, and thinking blocks in chronological order.
 * Uses Tauri event streaming for real-time updates (same pattern as Gyp chat).
 */

import {
  getWorkers,
  listenWorkerEvents,
  startWorkerEventStream,
  stopWorkerEventStream,
} from '../api';
import type {
  ChatToolCall,
  SheepConfig,
  WorkerDisplay,
  WorkerEvent,
  WorkerStreamEvent,
} from '../types';
import { type OutputChunk, chunkRendererHelpers } from './chunk-renderer';

/**
 * Worker Output Viewer Alpine component
 */
export function workerOutputViewer() {
  console.log('[WorkerOutput] Component factory called');
  return {
    // State
    runName: null as string | null,
    workerName: null as string | null,
    sheepConfig: null as SheepConfig | null,
    loading: false,
    error: null as string | null,
    visible: false,

    // Output chunks in chronological order
    chunks: [] as OutputChunk[],
    streaming: false,
    _lastChunkType: null as 'text' | 'thinking' | null,
    _toolsById: new Map() as Map<string, OutputChunk>,
    _unlisten: null as (() => void) | null,

    // Configuration
    showThinking: true,
    autoScroll: true,

    async init() {
      console.log('[WorkerOutput] init() called');
      // Listen for show-worker-output events
      window.addEventListener('show-worker-output', ((
        e: CustomEvent<{ runName: string; workerName: string }>,
      ) => {
        console.log('[WorkerOutput] show-worker-output event received:', e.detail);
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

    async cleanup() {
      // Stop listening for events
      if (this._unlisten) {
        this._unlisten();
        this._unlisten = null;
      }

      // Stop the backend stream
      if (this.runName && this.workerName) {
        try {
          await stopWorkerEventStream(this.runName, this.workerName);
        } catch (e) {
          console.error('[WorkerOutput] Error stopping stream:', e);
        }
      }
    },

    async show(runName: string, workerName: string) {
      await this.cleanup();
      this.chunks = [];
      this._lastChunkType = null;
      this._toolsById.clear();
      this.runName = runName;
      this.workerName = workerName;
      this.sheepConfig = null;
      this.loading = true;
      this.error = null;
      this.visible = true;

      try {
        // Fetch worker data for sheep avatar (don't block on this)
        getWorkers(runName)
          .then((workers) => {
            const worker = workers.find((w) => w.name === workerName) as WorkerDisplay | undefined;
            if (worker?.sheepConfig) {
              this.sheepConfig = worker.sheepConfig;
            }
          })
          .catch(() => {
            // Ignore errors fetching worker data
          });
        this._unlisten = await listenWorkerEvents((event) => {
          try {
            this.handleStreamEvent(event);
          } catch (err) {
            console.error('[WorkerOutput] Error handling event:', err);
          }
        });

        // Start the backend stream (will emit history first, then live events)
        await startWorkerEventStream(runName, workerName);

        // Set a timeout to clear loading if no history received within 5 seconds
        setTimeout(() => {
          if (this.loading && this.runName === runName && this.workerName === workerName) {
            console.warn('[WorkerOutput] Timeout waiting for history, showing empty state');
            this.loading = false;
          }
        }, 5000);
      } catch (e) {
        const error = e as Error;
        this.error = error.message || 'Failed to load worker output';
        console.error('[WorkerOutput] Error:', e);
        this.loading = false;
      }
    },

    async hide() {
      await this.cleanup();
      this.visible = false;
      this.runName = null;
      this.workerName = null;
      this.sheepConfig = null;
      this.chunks = [];
    },

    /**
     * Handle stream events from backend
     */
    handleStreamEvent(event: WorkerStreamEvent) {
      // Filter events for our worker
      if (event.runName !== this.runName || event.workerName !== this.workerName) {
        return;
      }

      console.log('[WorkerOutput] Stream event:', event.type);

      switch (event.type) {
        case 'history':
          // Process all historical events
          console.log('[WorkerOutput] Received history with', event.events.length, 'events');
          this.processEvents(event.events);
          this.updateStreamingState(event.workerStatus);
          this.loading = false;
          break;

        case 'event':
          // Process a single new event
          this.handleEvent(event.event);
          break;

        case 'status':
          // Update streaming state
          this.updateStreamingState(event.workerStatus);
          break;

        case 'ended':
          // Stream has ended
          console.log('[WorkerOutput] Stream ended');
          this.streaming = false;
          break;
      }
    },

    /**
     * Process batch of events
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
            event.toolKind || null,
            event.toolInput || null,
          );
          break;
        case 'tool_update':
          this.handleToolCallUpdate(
            event.toolCallId || '',
            event.toolStatus || 'completed',
            event.toolOutput || null,
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

    handleToolCallStart(id: string, title: string, kind: string | null, input: string | null) {
      // Tool call breaks the text/thinking stream
      this._lastChunkType = null;

      // Skip if this tool already exists (can happen with history replay)
      if (this._toolsById.has(id)) {
        return;
      }

      const toolCall: ChatToolCall = {
        id,
        title,
        kind,
        status: 'in_progress',
        input,
        output: null,
        expanded: false,
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
      if (chunk?.tool) {
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
      const isWorking = workerStatus === 'working';
      this.streaming = isWorking;

      // When worker is done, mark any in_progress tools as completed
      if (!isWorking && workerStatus) {
        let updated = false;
        for (const chunk of this._toolsById.values()) {
          if (chunk.tool && chunk.tool.status === 'in_progress') {
            chunk.tool.status = 'completed';
            updated = true;
          }
        }
        if (updated) {
          this.chunks = [...this.chunks]; // Trigger reactivity
        }
      }
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
      const d = date instanceof Date ? date : new Date(date);
      return d.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
    },

    /**
     * Toggle tool expanded state - searches all chunks
     */
    toggleToolExpanded(toolId: string) {
      // First check the active map
      let chunk = this._toolsById.get(toolId);

      // If not found, search through all chunks
      if (!chunk) {
        for (const c of this.chunks) {
          if (c.type === 'tool' && c.tool?.id === toolId) {
            chunk = c;
            break;
          }
        }
      }

      if (chunk?.tool) {
        chunk.tool.expanded = !chunk.tool.expanded;
        this.chunks = [...this.chunks]; // Trigger reactivity
      }
    },

    // Spread shared chunk renderer helpers
    ...chunkRendererHelpers(),
  };
}
