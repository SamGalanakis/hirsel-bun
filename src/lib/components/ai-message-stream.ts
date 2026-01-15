/**
 * AI Message Stream - Reusable component for displaying AI chat messages
 *
 * This component provides a shared rendering system for:
 * - Gyp chat interface
 * - Worker output viewer
 *
 * Features:
 * - Real-time streaming text display
 * - Tool call visualization with Lucide icons
 * - Thinking/reasoning block support
 * - Markdown-ready content
 */

import type { ChatMessage, ChatToolCall } from '../types';
import { getToolKindIcon, getToolStatusIcon as getToolStatusIconSvg } from '../icons';

/**
 * Base configuration for message stream display
 */
export interface MessageStreamConfig {
  showThinking?: boolean;
  showTimestamps?: boolean;
  showToolOutput?: boolean;
}

/**
 * AI Message Stream Alpine component
 *
 * Usage in HTML template:
 * <div x-data="aiMessageStream({ showThinking: true })">
 *   <template x-for="msg in messages">...</template>
 * </div>
 */
export function aiMessageStream(config: MessageStreamConfig = {}) {
  return {
    // Configuration
    showThinking: config.showThinking ?? false,
    showTimestamps: config.showTimestamps ?? true,
    showToolOutput: config.showToolOutput ?? false,

    // Messages can be injected from parent component
    messages: [] as ChatMessage[],

    // Current streaming state
    streaming: false,
    _currentMessage: null as ChatMessage | null,
    _currentToolCalls: new Map() as Map<string, ChatToolCall>,

    /**
     * Format time for display
     */
    formatTime(date: Date): string {
      if (!(date instanceof Date)) {
        date = new Date(date);
      }
      return date.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
    },

    /**
     * Get SVG icon for tool kind
     */
    getToolIcon(kind: string | null): string {
      return getToolKindIcon(kind, 14);
    },

    /**
     * Get SVG icon for tool status
     */
    getToolStatusIcon(status: string): string {
      return getToolStatusIconSvg(status, 12);
    },

    /**
     * Get CSS class for tool status
     */
    getToolStatusClass(status: string): string {
      const classes: Record<string, string> = {
        pending: 'text-wool-500',
        in_progress: 'text-amber-400',
        completed: 'text-sage',
        failed: 'text-terra',
      };
      return classes[status] || 'text-wool-500';
    },

    /**
     * Get message bubble class based on role
     */
    getMessageClass(role: string): string {
      switch (role) {
        case 'user':
          return 'bg-wool-700 text-wool-100';
        case 'assistant':
          return 'bg-wool-800/60 text-wool-200';
        case 'system':
          return 'bg-wool-900/40 text-wool-400 text-center italic text-xs';
        default:
          return 'bg-wool-800 text-wool-200';
      }
    },

    /**
     * Process text delta for streaming
     */
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
    },

    /**
     * Process thinking delta for streaming
     */
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

    /**
     * Handle tool call start
     */
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

    /**
     * Handle tool call update
     */
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

    /**
     * Handle message complete
     */
    handleMessageComplete() {
      if (this._currentMessage) {
        this._currentMessage.streaming = false;
        this.messages = [...this.messages];
      }

      this._currentMessage = null;
      this._currentToolCalls.clear();
      this.streaming = false;
    },

    /**
     * Add a user message
     */
    addUserMessage(content: string): ChatMessage {
      const msg: ChatMessage = {
        id: crypto.randomUUID(),
        role: 'user',
        content,
        timestamp: new Date(),
      };
      this.messages = [...this.messages, msg];
      return msg;
    },

    /**
     * Add a system message
     */
    addSystemMessage(content: string): ChatMessage {
      const msg: ChatMessage = {
        id: crypto.randomUUID(),
        role: 'system',
        content,
        timestamp: new Date(),
      };
      this.messages = [...this.messages, msg];
      return msg;
    },

    /**
     * Clear all messages
     */
    clearMessages() {
      this.messages = [];
      this._currentMessage = null;
      this._currentToolCalls.clear();
      this.streaming = false;
    },
  };
}
