/**
 * Direct Chat component - AI chat via ACP
 *
 * This component provides a chat interface that works with any ACP-compatible
 * AI agent. It supports:
 * - Real-time streaming of responses
 * - Tool call visualization
 * - Permission request modals (auto-approve hirsel, ask for others)
 * - Context injection (run, worker, UI section)
 */

import type {
  ChatEvent,
  ChatMessage,
  ChatToolCall,
  PendingPermission,
  UIContext,
} from '../types';
import {
  startChatSession,
  sendChatMessage,
  respondChatPermission,
  stopChatSession,
  listenChatEvents,
} from '../api';

/**
 * Direct chat Alpine component
 */
export function directChat() {
  return {
    sessionId: null as string | null,
    messages: [] as ChatMessage[],
    inputText: '',
    loading: false,
    streaming: false,
    error: null as string | null,
    connected: false,
    pendingPermission: null as PendingPermission | null,
    showThinking: false,
    agentCommand: ['claude-code-acp'] as string[],
    _unlisten: null as (() => void) | null,
    _currentMessage: null as ChatMessage | null,
    _currentToolCalls: new Map() as Map<string, ChatToolCall>,

    async init() {
      // Listen for run selection changes
      window.addEventListener('run-selected', (_e: Event) => {
        // Could reconnect session with new run context
      });
    },

    destroy() {
      this.disconnect();
    },

    getAppState(): {
      selectedRun?: string | null;
      selectedWorker?: { name: string } | null;
      currentSection?: string;
      currentRunDetail?: {
        status: string;
        request: string | null;
        projectPath: string | null;
      } | null;
    } | null {
      // Try to find appState from body element (where x-data="appState()" is defined)
      const body = document.body;
      // @ts-expect-error Alpine.js internal property
      if (body._x_dataStack && body._x_dataStack[0]) {
        // @ts-expect-error Alpine.js internal property
        return body._x_dataStack[0];
      }
      // Fallback: walk up from current element
      // @ts-expect-error Alpine.js $el magic property
      let el = this.$el as HTMLElement;
      while (el && el.parentElement) {
        el = el.parentElement;
        // @ts-expect-error Alpine.js internal property
        if (el._x_dataStack) {
          // @ts-expect-error Alpine.js internal property
          return el._x_dataStack[0];
        }
      }
      return null;
    },

    getUIContext(): UIContext {
      const app = this.getAppState();
      const detail = app?.currentRunDetail;

      const context: UIContext = {
        selectedRun: app?.selectedRun || null,
        selectedWorker: app?.selectedWorker?.name || null,
        uiSection: app?.currentSection || 'chat',
      };

      // Always include file paths when a run with projectPath is selected
      if (detail?.projectPath) {
        context.extra = {
          projectPath: detail.projectPath,
          specFile: `${detail.projectPath}/spec.md`,
          evalFile: `${detail.projectPath}/eval.md`,
          runStatus: detail.status,
        };
        if (detail.request) {
          context.extra.currentSpec = detail.request;
        }
      }

      return context;
    },

    async connect() {
      if (this.sessionId) {
        await this.disconnect();
      }

      this.loading = true;
      this.error = null;

      try {
        // Start listening for events first
        // Capture `this` to ensure correct binding in callback
        const self = this;
        this._unlisten = await listenChatEvents((event) => {
          try {
            self.handleChatEvent(event);
          } catch (e) {
            console.error('[DirectChat] Error handling event:', e);
          }
        });

        // Get context
        const app = this.getAppState();
        const runName = app?.selectedRun || undefined;

        // Start the session
        const sessionId = await startChatSession(this.agentCommand, {
          runName,
          systemPrompt: this.getSystemPrompt(),
        });
        this.sessionId = sessionId;

        this.connected = true;

        // Add static welcome message (no model tokens used)
        this.messages = [{
          id: crypto.randomUUID(),
          role: 'assistant',
          content: `Hello! I'm Gyp, your AI assistant for Hirsel. I can help you manage runs, tasks, and workers using the hirsel tools.\n\nWhat would you like to do today?`,
          timestamp: new Date(),
        }];
      } catch (e) {
        const error = e as Error;
        this.error = error.message || 'Failed to connect';
        console.error('[DirectChat] Failed to connect:', e);
      } finally {
        this.loading = false;
      }
    },

    async disconnect() {
      if (this._unlisten) {
        this._unlisten();
        this._unlisten = null;
      }

      if (this.sessionId) {
        try {
          await stopChatSession(this.sessionId);
        } catch (e) {
          console.error('Error stopping session:', e);
        }
        this.sessionId = null;
      }

      this.connected = false;
      this.streaming = false;
      this._currentMessage = null;
      this._currentToolCalls.clear();
    },

    getSystemPrompt(): string {
      return `You are Gyp, an AI assistant for Hirsel.

RULES:
1. NEVER use "hirsel" CLI commands - they will fail
2. Use hirsel MCP tools for tasks: task_list, task_add, task_done, msg_send, msg_read
3. To edit spec/eval, use Read/Edit/Write on the paths provided in <ui-context>

The <ui-context> block contains:
- specFile: exact path to spec.md (use this for Read/Edit)
- evalFile: exact path to eval.md
- currentSpec: current spec contents

Be concise.`;
    },

    handleChatEvent(event: ChatEvent) {
      // Filter events for our session
      if (event.sessionId !== this.sessionId) {
        return;
      }

      switch (event.type) {
        case 'textDelta':
          this.handleTextDelta(event.text);
          break;
        case 'thinkingDelta':
          this.handleThinkingDelta(event.text);
          break;
        case 'toolCallStart':
          this.handleToolCallStart(event.toolCallId, event.title, event.kind);
          break;
        case 'toolCallUpdate':
          this.handleToolCallUpdate(event.toolCallId, event.status, event.output);
          break;
        case 'permissionRequest':
          this.handlePermissionRequest(event.request);
          break;
        case 'messageComplete':
          this.handleMessageComplete();
          break;
        case 'error':
          this.handleError(event.message);
          break;
        case 'sessionEnded':
          this.handleSessionEnded();
          break;
      }
    },

    handleTextDelta(text: string) {
      if (!this._currentMessage) {
        // Start a new assistant message
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

      // Update content and force Alpine reactivity by replacing the message object
      this._currentMessage.content += text;
      const idx = this.messages.findIndex(m => m.id === this._currentMessage!.id);
      if (idx !== -1) {
        // Create new object to trigger Alpine reactivity
        this._currentMessage = { ...this._currentMessage };
        this.messages[idx] = this._currentMessage;
        this.messages = [...this.messages];
      }
      this.scrollToBottom();
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

    handlePermissionRequest(request: PendingPermission) {
      this.pendingPermission = request;
    },

    async respondToPermission(optionId: string) {
      if (!this.pendingPermission || !this.sessionId) return;

      try {
        await respondChatPermission(
          this.sessionId,
          this.pendingPermission.requestId,
          optionId
        );
      } catch (e) {
        console.error('Failed to respond to permission:', e);
      }

      this.pendingPermission = null;
    },

    handleMessageComplete() {
      if (this._currentMessage) {
        this._currentMessage.streaming = false;
        this.messages = [...this.messages];
      }

      this._currentMessage = null;
      this._currentToolCalls.clear();
      this.streaming = false;

      // Refresh draft if we edited spec/eval files
      this.checkAndRefreshDraft();
    },

    checkAndRefreshDraft() {
      const app = this.getAppState();
      const detail = app?.currentRunDetail;
      if (detail?.status === 'draft' && app?.selectedRun) {
        // Emit event to refresh the draft editor
        window.dispatchEvent(new CustomEvent('draft-refresh', {
          detail: app.selectedRun
        }));
      }
    },

    handleError(message: string) {
      this.error = message;
      this.streaming = false;
      window.toast?.error(message, 'Chat Error');
    },

    handleSessionEnded() {
      this.connected = false;
      this.streaming = false;
      this._currentMessage = null;
      this._currentToolCalls.clear();
      this.addSystemMessage('Session ended');
    },

    addSystemMessage(content: string) {
      this.messages = [...this.messages, {
        id: crypto.randomUUID(),
        role: 'system',
        content,
        timestamp: new Date(),
      }];
      this.scrollToBottom();
    },

    async sendMessage() {
      const content = this.inputText.trim();
      if (!content || !this.sessionId || this.streaming) {
        return;
      }

      // Add user message to display (reassign for Alpine reactivity)
      const userMessage = {
        id: crypto.randomUUID(),
        role: 'user' as const,
        content,
        timestamp: new Date(),
      };
      this.messages = [...this.messages, userMessage];

      this.inputText = '';
      this.scrollToBottom();

      try {
        // Send with UI context (invisible to user)
        await sendChatMessage(this.sessionId, content, this.getUIContext());
      } catch (e) {
        const error = e as Error;
        console.error('[DirectChat] Failed to send:', e);
        this.handleError(error.message || 'Failed to send message');
        // Restore input on error
        this.inputText = content;
      }
    },

    scrollToBottom() {
      // @ts-expect-error Alpine.js $nextTick magic method
      this.$nextTick(() => {
        // @ts-expect-error Alpine.js $el magic property
        const container = (this.$el as HTMLElement).querySelector('.messages-container');
        if (container) {
          container.scrollTop = container.scrollHeight;
        }
      });
    },

    formatTime(date: Date): string {
      return date.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
    },

    getToolIcon(kind: string | null): string {
      const icons: Record<string, string> = {
        read: '\u{1F4C4}',     // 📄
        edit: '\u{270F}',      // ✏️
        execute: '\u{25B6}',   // ▶
        search: '\u{1F50D}',   // 🔍
        think: '\u{1F4AD}',    // 💭
        fetch: '\u{1F310}',    // 🌐
      };
      return icons[kind || ''] || '\u{1F527}'; // 🔧
    },

    getToolStatusClass(status: string): string {
      const classes: Record<string, string> = {
        pending: 'text-wool-500',
        in_progress: 'text-amber-500 animate-pulse',
        completed: 'text-sage',
        failed: 'text-terra',
      };
      return classes[status] || 'text-wool-500';
    },
  };
}
