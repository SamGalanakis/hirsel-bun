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

import {
  getGypChatHistory,
  listenChatEvents,
  respondChatPermission,
  saveGypMessage,
  sendChatMessage,
  startChatSession,
  stopChatSession,
} from '../api';
import type { ChatEvent, ChatToolCall, PendingPermission, UIContext } from '../types';
import { type OutputChunk, chunkRendererHelpers } from './chunk-renderer';

/** A chunk of content in a message - reuse shared type */
type MessageChunk = OutputChunk & {
  // MessageChunk is same as OutputChunk
  content?: string;
  tool?: ChatToolCall;
};

/** A chat message with chronologically ordered chunks */
interface ChatMessageWithChunks {
  id: string;
  role: 'user' | 'assistant' | 'system';
  chunks: MessageChunk[];
  timestamp: Date;
  streaming?: boolean;
}

/**
 * Direct chat Alpine component
 */
export function directChat() {
  return {
    sessionId: null as string | null,
    messages: [] as ChatMessageWithChunks[],
    inputText: '',
    loading: false,
    streaming: false,
    error: null as string | null,
    connected: false,
    pendingPermission: null as PendingPermission | null,
    showThinking: false,
    agentCommand: ['claude-code-acp'] as string[],
    _unlisten: null as (() => void) | null,
    _currentMessage: null as ChatMessageWithChunks | null,
    _lastChunkType: null as 'text' | 'thinking' | null,
    _toolsById: new Map() as Map<string, MessageChunk>,
    _currentRunName: null as string | null,
    // Toggle for run-specific vs general chat (only matters when a run is selected)
    useRunContext: true,

    async init() {
      // Listen for run selection changes
      window.addEventListener('run-selected', (_e: Event) => {
        // Could reconnect session with new run context
      });
    },

    destroy() {
      this.disconnect();
    },

    /** Get the currently selected run name from app state */
    getSelectedRunName(): string | null {
      return this.getAppState()?.selectedRun || null;
    },

    /** Check if run-specific context is available (run has a workspace) */
    canUseRunContext(): boolean {
      const appState = this.getAppState();
      if (!appState?.selectedRun) return false;
      // Run needs a projectPath (workspace) to use run-specific context
      return Boolean(appState.currentRunDetail?.projectPath);
    },

    /** Get the effective run name based on toggle state */
    getEffectiveRunName(): string | null {
      const selectedRun = this.getSelectedRunName();
      // If no run selected, toggle is off, or run has no workspace, use general chat
      if (!selectedRun || !this.useRunContext || !this.canUseRunContext()) {
        return null;
      }
      return selectedRun;
    },

    /** Toggle between run-specific and general chat */
    async toggleContext() {
      this.useRunContext = !this.useRunContext;
      // Reload chat with new context
      await this.reloadChat();
    },

    /** Reload chat history for current context */
    async reloadChat() {
      const runName = this.getEffectiveRunName();
      this._currentRunName = runName;

      // Load history for new context
      try {
        const history = await getGypChatHistory(runName);
        if (history.length > 0) {
          this.messages = history.map((msg) => ({
            id: crypto.randomUUID(),
            role: msg.role as 'user' | 'assistant' | 'system',
            chunks: JSON.parse(msg.chunksJson),
            timestamp: new Date(msg.timestamp),
          }));
          console.log(
            `[DirectChat] Loaded ${history.length} messages for context: ${runName || 'general'}`,
          );
        } else {
          // Show welcome message for empty history
          this.messages = [
            {
              id: crypto.randomUUID(),
              role: 'assistant',
              chunks: [
                {
                  id: crypto.randomUUID(),
                  type: 'text',
                  content: `Hello! I'm Gyp, your AI assistant for Hirsel. I can help you manage runs, tasks, and workers using the hirsel tools.\n\nWhat would you like to do today?`,
                },
              ],
              timestamp: new Date(),
            },
          ];
        }
      } catch (e) {
        console.warn('[DirectChat] Failed to load chat history:', e);
        this.messages = [
          {
            id: crypto.randomUUID(),
            role: 'assistant',
            chunks: [
              {
                id: crypto.randomUUID(),
                type: 'text',
                content: `Hello! I'm Gyp, your AI assistant for Hirsel. I can help you manage runs, tasks, and workers using the hirsel tools.\n\nWhat would you like to do today?`,
              },
            ],
            timestamp: new Date(),
          },
        ];
      }
      this.scrollToBottom();
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
      if (body._x_dataStack?.[0]) {
        // @ts-expect-error Alpine.js internal property
        return body._x_dataStack[0];
      }
      // Fallback: walk up from current element
      // @ts-expect-error Alpine.js $el magic property
      let el = this.$el as HTMLElement;
      while (el?.parentElement) {
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
        this._unlisten = await listenChatEvents((event) => {
          try {
            this.handleChatEvent(event);
          } catch (e) {
            console.error('[DirectChat] Error handling event:', e);
          }
        });

        // Get context - use effective run name based on toggle
        const runName = this.getEffectiveRunName();
        this._currentRunName = runName;

        // Get project path from run detail for working directory context
        const appState = this.getAppState();
        const projectPath = appState?.currentRunDetail?.projectPath || undefined;

        // Start the session (pass selected run for MCP tools, regardless of chat context)
        const sessionId = await startChatSession(this.agentCommand, {
          runName: this.getSelectedRunName() || undefined,
          systemPrompt: this.getSystemPrompt(),
          workingDir: projectPath, // Give Gyp access to the actual project
        });
        this.sessionId = sessionId;

        this.connected = true;

        // Load chat history for effective context
        let loadedHistory = false;
        try {
          const history = await getGypChatHistory(runName);
          if (history.length > 0) {
            // Restore messages from history
            this.messages = history.map((msg) => ({
              id: crypto.randomUUID(),
              role: msg.role as 'user' | 'assistant' | 'system',
              chunks: JSON.parse(msg.chunksJson),
              timestamp: new Date(msg.timestamp),
            }));
            loadedHistory = true;
            console.log(
              `[DirectChat] Loaded ${history.length} messages for context: ${runName || 'general'}`,
            );
          }
        } catch (e) {
          console.warn('[DirectChat] Failed to load chat history:', e);
        }

        // Add static welcome message only if no history loaded
        if (!loadedHistory) {
          this.messages = [
            {
              id: crypto.randomUUID(),
              role: 'assistant',
              chunks: [
                {
                  id: crypto.randomUUID(),
                  type: 'text',
                  content: `Hello! I'm Gyp, your AI assistant for Hirsel. I can help you manage runs, tasks, and workers using the hirsel tools.\n\nWhat would you like to do today?`,
                },
              ],
              timestamp: new Date(),
            },
          ];
        }
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
      this._lastChunkType = null;
      this._toolsById.clear();
      this._currentRunName = null;
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
          this.handleToolCallStart(event.toolCallId, event.title, event.kind, event.input);
          break;
        case 'toolCallUpdate':
          this.handleToolCallUpdate(event.toolCallId, event.status, event.title, event.output);
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
          chunks: [],
          timestamp: new Date(),
          streaming: true,
        };
        this.messages = [...this.messages, this._currentMessage];
        this.streaming = true;
      }

      // If last chunk was text, append to it; otherwise create new text chunk
      const chunks = this._currentMessage.chunks;
      if (this._lastChunkType === 'text' && chunks.length > 0) {
        const lastChunk = chunks[chunks.length - 1];
        if (lastChunk.type === 'text') {
          lastChunk.content = (lastChunk.content || '') + text;
          this.messages = [...this.messages];
          this.scrollToBottom();
          return;
        }
      }

      // Create new text chunk
      const chunk: MessageChunk = {
        id: crypto.randomUUID(),
        type: 'text',
        content: text,
      };
      this._currentMessage.chunks = [...chunks, chunk];
      this._lastChunkType = 'text';
      this.messages = [...this.messages];
      this.scrollToBottom();
    },

    handleThinkingDelta(text: string) {
      if (!this._currentMessage) {
        this._currentMessage = {
          id: crypto.randomUUID(),
          role: 'assistant',
          chunks: [],
          timestamp: new Date(),
          streaming: true,
        };
        this.messages = [...this.messages, this._currentMessage];
        this.streaming = true;
      }

      // If last chunk was thinking, append to it; otherwise create new thinking chunk
      const chunks = this._currentMessage.chunks;
      if (this._lastChunkType === 'thinking' && chunks.length > 0) {
        const lastChunk = chunks[chunks.length - 1];
        if (lastChunk.type === 'thinking') {
          lastChunk.content = (lastChunk.content || '') + text;
          this.messages = [...this.messages];
          return;
        }
      }

      // Create new thinking chunk
      const chunk: MessageChunk = {
        id: crypto.randomUUID(),
        type: 'thinking',
        content: text,
      };
      this._currentMessage.chunks = [...chunks, chunk];
      this._lastChunkType = 'thinking';
      this.messages = [...this.messages];
    },

    handleToolCallStart(id: string, title: string, kind: string | null, input: string | null) {
      // Tool call breaks the text/thinking stream
      this._lastChunkType = null;

      // Skip if this tool already exists (prevents duplicate key errors in x-for)
      if (this._toolsById.has(id)) {
        // Update existing tool's title/input if changed
        const existing = this._toolsById.get(id);
        if (existing?.tool) {
          let updated = false;
          if (existing.tool.title !== title) {
            existing.tool.title = title;
            updated = true;
          }
          if (input && existing.tool.input !== input) {
            existing.tool.input = input;
            updated = true;
          }
          if (updated) {
            this.messages = [...this.messages];
          }
        }
        return;
      }

      if (!this._currentMessage) {
        this._currentMessage = {
          id: crypto.randomUUID(),
          role: 'assistant',
          chunks: [],
          timestamp: new Date(),
          streaming: true,
        };
        this.messages = [...this.messages, this._currentMessage];
        this.streaming = true;
      }

      const toolCall: ChatToolCall = {
        id,
        title,
        kind,
        status: 'in_progress',
        input,
        output: null,
      };

      const chunk: MessageChunk = {
        id,
        type: 'tool',
        tool: toolCall,
      };

      this._toolsById.set(id, chunk);
      this._currentMessage.chunks = [...this._currentMessage.chunks, chunk];
      this.messages = [...this.messages];
      this.scrollToBottom();
    },

    handleToolCallUpdate(id: string, status: string, title: string | null, output: string | null) {
      console.log('[DirectChat] toolCallUpdate:', {
        id,
        status,
        title,
        output: output?.substring(0, 100),
      });
      // First check the active map
      let chunk = this._toolsById.get(id);

      // If not found, search through all messages
      if (!chunk) {
        for (const msg of this.messages) {
          for (const c of msg.chunks) {
            if (c.type === 'tool' && c.tool?.id === id) {
              chunk = c;
              break;
            }
          }
          if (chunk) break;
        }
      }

      if (chunk?.tool) {
        chunk.tool.status = status;
        if (title) {
          chunk.tool.title = title;
        }
        if (output) {
          chunk.tool.output = output;
        }
        this.messages = [...this.messages];
      }
    },

    handlePermissionRequest(request: PendingPermission) {
      this.pendingPermission = request;
    },

    async respondToPermission(optionId: string) {
      if (!this.pendingPermission || !this.sessionId) return;

      try {
        await respondChatPermission(this.sessionId, this.pendingPermission.requestId, optionId);
      } catch (e) {
        console.error('Failed to respond to permission:', e);
      }

      this.pendingPermission = null;
    },

    handleMessageComplete() {
      if (this._currentMessage) {
        this._currentMessage.streaming = false;
        this.messages = [...this.messages];

        // Save assistant message to history if we have a run
        if (this._currentRunName) {
          const msgToSave = this._currentMessage;
          saveGypMessage(this._currentRunName, 'assistant', JSON.stringify(msgToSave.chunks)).catch(
            (e) => console.warn('[DirectChat] Failed to save assistant message:', e),
          );
        }
      }

      this._currentMessage = null;
      this._lastChunkType = null;
      this._toolsById.clear();
      this.streaming = false;

      // Refresh draft if we edited spec/eval files
      this.checkAndRefreshDraft();
    },

    checkAndRefreshDraft() {
      const app = this.getAppState();
      const detail = app?.currentRunDetail;
      if (detail?.status === 'draft' && app?.selectedRun) {
        // Emit event to refresh the draft editor
        window.dispatchEvent(
          new CustomEvent('draft-refresh', {
            detail: app.selectedRun,
          }),
        );
      }
    },

    handleError(message: string) {
      this.error = message;
      this.streaming = false;
      window.toast?.error(message);
    },

    handleSessionEnded() {
      this.connected = false;
      this.streaming = false;
      this._currentMessage = null;
      this._lastChunkType = null;
      this._toolsById.clear();
      this.addSystemMessage('Session ended');
    },

    addSystemMessage(content: string) {
      this.messages = [
        ...this.messages,
        {
          id: crypto.randomUUID(),
          role: 'system',
          chunks: [
            {
              id: crypto.randomUUID(),
              type: 'text',
              content,
            },
          ],
          timestamp: new Date(),
        },
      ];
      this.scrollToBottom();
    },

    async sendMessage() {
      const content = this.inputText.trim();
      if (!content || this.streaming) {
        return;
      }

      // Lazy connect on first message
      if (!this.sessionId && !this.loading) {
        await this.connect();
        if (!this.sessionId) {
          return; // Connection failed
        }
      }

      // Add user message to display (reassign for Alpine reactivity)
      const userMessage: ChatMessageWithChunks = {
        id: crypto.randomUUID(),
        role: 'user',
        chunks: [
          {
            id: crypto.randomUUID(),
            type: 'text',
            content,
          },
        ],
        timestamp: new Date(),
      };
      this.messages = [...this.messages, userMessage];

      this.inputText = '';
      this.scrollToBottom();

      // Save user message to history if we have a run
      if (this._currentRunName) {
        try {
          await saveGypMessage(this._currentRunName, 'user', JSON.stringify(userMessage.chunks));
        } catch (e) {
          console.warn('[DirectChat] Failed to save user message:', e);
        }
      }

      try {
        // Send with UI context (invisible to user)
        // sessionId is guaranteed non-null here due to checks above
        await sendChatMessage(this.sessionId!, content, this.getUIContext());
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

    /**
     * Toggle tool expanded state - searches all messages
     */
    toggleToolExpanded(toolId: string) {
      // First check the active map
      const activeChunk = this._toolsById.get(toolId);
      if (activeChunk?.tool) {
        activeChunk.tool.expanded = !activeChunk.tool.expanded;
        this.messages = [...this.messages];
        return;
      }

      // Search through all messages for completed tools
      for (const msg of this.messages) {
        for (const chunk of msg.chunks) {
          if (chunk.type === 'tool' && chunk.tool?.id === toolId) {
            chunk.tool.expanded = !chunk.tool.expanded;
            this.messages = [...this.messages];
            return;
          }
        }
      }
    },

    // Spread shared chunk renderer helpers
    ...chunkRendererHelpers(),
  };
}
