/**
 * Chat Panel Component for Hirsel Desktop App
 *
 * This Alpine.js component implements the slide-out chat panel with:
 * - Thread list with unread counts
 * - Message display with timestamps
 * - Message input and sending
 * - Real-time polling for new messages
 */

import {
  getThreads,
  getMessages,
  sendMessage as apiSendMessage,
  markMessagesRead,
  createPoller,
  dispatchEvent,
} from '../lib/api';
import type { ThreadSummary, Message } from '../lib/types';

// Polling interval for messages (ms)
const MESSAGE_POLL_INTERVAL = 2000;

/**
 * Alpine.js magic properties
 */
interface AlpineMagic {
  $nextTick: (fn: () => void) => void;
  $el: HTMLElement;
  $data: Record<string, unknown>;
}

/**
 * Chat panel component state and methods
 */
export interface ChatPanelState extends AlpineMagic {
  threads: ThreadSummary[];
  selectedThread: string;
  messages: Message[];
  newMessage: string;
  loading: boolean;
  error: string | null;

  // Methods
  init(): Promise<void>;
  fetchThreads(runName: string): Promise<void>;
  selectThread(name: string): Promise<void>;
  sendMessage(): Promise<void>;
  formatTime(timestamp: string): string;
  formatDate(timestamp: string): string;
  scrollToBottom(): void;
  destroy(): void;
  getRunName(): string | null;
  handleRunChange(event: CustomEvent): void;
  updateUnreadCount(): void;
}

/**
 * Internal component data (without Alpine magic properties which are injected at runtime)
 */
interface ChatPanelData {
  threads: ThreadSummary[];
  selectedThread: string;
  messages: Message[];
  newMessage: string;
  loading: boolean;
  error: string | null;
}

/**
 * Create the chat panel Alpine.js component
 *
 * Note: Alpine.js magic properties ($nextTick, $el, $data) are injected at runtime
 * so we use type assertions where needed.
 */
export function chatPanel() {
  let threadPoller: ReturnType<typeof createPoller<ThreadSummary[]>> | null = null;
  let messagePoller: ReturnType<typeof createPoller<Message[]>> | null = null;

  const component: ChatPanelData & {
    init(this: ChatPanelState): Promise<void>;
    fetchThreads(this: ChatPanelState, runName: string): Promise<void>;
    selectThread(this: ChatPanelState, name: string): Promise<void>;
    sendMessage(this: ChatPanelState): Promise<void>;
    formatTime(timestamp: string): string;
    formatDate(timestamp: string): string;
    scrollToBottom(this: ChatPanelState): void;
    destroy(this: ChatPanelState): void;
    getRunName(this: ChatPanelState): string | null;
    handleRunChange(this: ChatPanelState, event: CustomEvent): void;
    updateUnreadCount(this: ChatPanelState): void;
  } = {
    threads: [],
    selectedThread: 'user',
    messages: [],
    newMessage: '',
    loading: false,
    error: null,

    async init(this: ChatPanelState) {
      // Get run name from parent scope
      const runName = this.getRunName();
      if (!runName) {
        this.threads = [];
        this.messages = [];
        return;
      }

      // Initial fetch
      await this.fetchThreads(runName);

      // Select user thread by default if available
      if (this.threads.length > 0) {
        const userThread = this.threads.find((t: ThreadSummary) => t.name === 'user');
        if (userThread) {
          await this.selectThread('user');
        } else {
          await this.selectThread(this.threads[0].name);
        }
      }

      // Start polling for thread updates
      const self = this;
      threadPoller = createPoller(
        () => getThreads(runName),
        (threads: ThreadSummary[]) => {
          self.threads = threads;
          self.updateUnreadCount();
        },
        MESSAGE_POLL_INTERVAL * 2 // Poll threads less frequently
      );
      threadPoller.start();

      // Listen for run selection changes
      window.addEventListener('run-selected', this.handleRunChange.bind(this) as EventListener);
    },

    async fetchThreads(this: ChatPanelState, runName: string) {
      try {
        this.threads = await getThreads(runName);
        this.updateUnreadCount();
      } catch (e) {
        console.error('Failed to fetch threads:', e);
        this.threads = [];
      }
    },

    async selectThread(this: ChatPanelState, name: string) {
      this.selectedThread = name;
      this.messages = [];
      this.loading = true;
      this.error = null;

      // Stop existing message poller
      if (messagePoller) {
        messagePoller.stop();
        messagePoller = null;
      }

      const runName = this.getRunName();
      if (!runName) {
        this.loading = false;
        return;
      }

      try {
        // Fetch messages for selected thread
        this.messages = await getMessages(runName, name);

        // Mark as read
        await markMessagesRead(runName, name);

        // Update thread unread count
        const thread = this.threads.find((t: ThreadSummary) => t.name === name);
        if (thread) {
          thread.unreadCount = 0;
        }
        this.updateUnreadCount();

        // Scroll to bottom after messages load
        this.$nextTick(() => this.scrollToBottom());

        // Start polling for new messages in this thread
        const self = this;
        messagePoller = createPoller(
          () => getMessages(runName, name),
          async (messages: Message[]) => {
            const hadNewMessages = messages.length > self.messages.length;
            self.messages = messages;

            if (hadNewMessages) {
              // Mark new messages as read
              await markMessagesRead(runName, name);
              self.$nextTick(() => self.scrollToBottom());
            }
          },
          MESSAGE_POLL_INTERVAL
        );
        messagePoller.start();

      } catch (e) {
        console.error('Failed to fetch messages:', e);
        this.error = 'Failed to load messages';
      } finally {
        this.loading = false;
      }
    },

    async sendMessage(this: ChatPanelState) {
      const content = this.newMessage.trim();
      if (!content) return;

      const runName = this.getRunName();
      if (!runName) {
        this.error = 'No run selected';
        return;
      }

      // Clear input immediately for responsiveness
      this.newMessage = '';

      try {
        await apiSendMessage(runName, this.selectedThread, content);

        // Refresh messages
        this.messages = await getMessages(runName, this.selectedThread);
        this.$nextTick(() => this.scrollToBottom());

        // Dispatch event for other components
        dispatchEvent('message-sent', {
          thread: this.selectedThread,
          content,
        });

      } catch (e) {
        console.error('Failed to send message:', e);
        this.error = 'Failed to send message';
        // Restore the message on error
        this.newMessage = content;
      }
    },

    formatTime(timestamp: string): string {
      if (!timestamp) return '';
      const d = new Date(timestamp);
      return d.toLocaleTimeString('en-US', {
        hour: '2-digit',
        minute: '2-digit',
        hour12: true
      });
    },

    formatDate(timestamp: string): string {
      if (!timestamp) return '';
      const d = new Date(timestamp);
      const today = new Date();

      // Check if same day
      if (d.toDateString() === today.toDateString()) {
        return 'Today';
      }

      // Check if yesterday
      const yesterday = new Date(today);
      yesterday.setDate(yesterday.getDate() - 1);
      if (d.toDateString() === yesterday.toDateString()) {
        return 'Yesterday';
      }

      // Otherwise show date
      return d.toLocaleDateString('en-US', {
        month: 'short',
        day: 'numeric'
      });
    },

    scrollToBottom(this: ChatPanelState) {
      const container = this.$el.querySelector('.messages-container');
      if (container) {
        container.scrollTop = container.scrollHeight;
      }
    },

    destroy(this: ChatPanelState) {
      // Clean up pollers
      if (threadPoller) {
        threadPoller.stop();
        threadPoller = null;
      }
      if (messagePoller) {
        messagePoller.stop();
        messagePoller = null;
      }

      // Remove event listener
      window.removeEventListener('run-selected', this.handleRunChange.bind(this) as EventListener);
    },

    getRunName(this: ChatPanelState): string | null {
      // Access parent Alpine scope to get selected run
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const win = window as any;
      return win.Alpine?.store?.('app')?.selectedRun ||
             (this.$data as Record<string, unknown>).selectedRun as string ||
             null;
    },

    handleRunChange(this: ChatPanelState, event: CustomEvent) {
      const runName = event.detail as string;
      if (runName) {
        this.fetchThreads(runName).then(() => {
          if (this.threads.length > 0) {
            this.selectThread(this.threads[0].name);
          }
        });
      } else {
        this.threads = [];
        this.messages = [];
      }
    },

    updateUnreadCount(this: ChatPanelState) {
      const total = this.threads.reduce((sum: number, t: ThreadSummary) => sum + t.unreadCount, 0);
      // Dispatch event for header badge
      dispatchEvent('unread-count-changed', { count: total });
    },
  };

  return component;
}

/**
 * Enhanced message display with grouping by date
 */
export interface MessageGroup {
  date: string;
  messages: Message[];
}

/**
 * Group messages by date for display
 */
export function groupMessagesByDate(messages: Message[]): MessageGroup[] {
  const groups: Map<string, Message[]> = new Map();

  for (const msg of messages) {
    const date = new Date(msg.timestamp).toDateString();
    if (!groups.has(date)) {
      groups.set(date, []);
    }
    groups.get(date)!.push(msg);
  }

  return Array.from(groups.entries()).map(([date, msgs]) => ({
    date,
    messages: msgs,
  }));
}

/**
 * Get sender display color based on name
 */
export function getSenderColor(sender: string): string {
  // Hash the sender name to get a consistent color
  const colors = [
    'text-amber-500',   // User/primary
    'text-sage',        // Success/green
    'text-sky-400',     // Info/blue
    'text-pink-400',    // Accent
    'text-purple-400',  // Secondary
    'text-golden',      // Warning
  ];

  // Special cases
  if (sender === 'user' || sender === 'admin') {
    return colors[0];
  }
  if (sender === 'system') {
    return 'text-wool-500';
  }

  // Hash for workers and other senders
  let hash = 0;
  for (let i = 0; i < sender.length; i++) {
    hash = sender.charCodeAt(i) + ((hash << 5) - hash);
  }
  return colors[Math.abs(hash) % colors.length];
}

/**
 * Format message content with markdown-like styling
 */
export function formatMessageContent(content: string): string {
  // Simple inline code formatting
  let formatted = content.replace(/`([^`]+)`/g, '<code class="bg-pasture-700 px-1 rounded text-xs">$1</code>');

  // Bold text
  formatted = formatted.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');

  // Italic text
  formatted = formatted.replace(/\*([^*]+)\*/g, '<em>$1</em>');

  return formatted;
}

// Export the component function for Alpine.js registration
export default chatPanel;
