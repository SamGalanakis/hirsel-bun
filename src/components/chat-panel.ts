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
 * Chat panel component state and methods
 */
export interface ChatPanelState {
  threads: ThreadSummary[];
  selectedThread: string;
  messages: Message[];
  newMessage: string;
  loading: boolean;
  error: string | null;

  // Methods
  init(): Promise<void>;
  selectThread(name: string): Promise<void>;
  sendMessage(): Promise<void>;
  formatTime(timestamp: string): string;
  formatDate(timestamp: string): string;
  scrollToBottom(): void;
  destroy(): void;
}

/**
 * Create the chat panel Alpine.js component
 */
export function chatPanel(): ChatPanelState {
  let threadPoller: ReturnType<typeof createPoller> | null = null;
  let messagePoller: ReturnType<typeof createPoller> | null = null;

  return {
    threads: [],
    selectedThread: 'user',
    messages: [],
    newMessage: '',
    loading: false,
    error: null,

    async init() {
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
        const userThread = this.threads.find(t => t.name === 'user');
        if (userThread) {
          await this.selectThread('user');
        } else {
          await this.selectThread(this.threads[0].name);
        }
      }

      // Start polling for thread updates
      threadPoller = createPoller(
        () => getThreads(runName),
        (threads) => {
          this.threads = threads;
          this.updateUnreadCount();
        },
        MESSAGE_POLL_INTERVAL * 2 // Poll threads less frequently
      );
      threadPoller.start();

      // Listen for run selection changes
      window.addEventListener('run-selected', this.handleRunChange.bind(this) as EventListener);
    },

    async fetchThreads(runName: string) {
      try {
        this.threads = await getThreads(runName);
        this.updateUnreadCount();
      } catch (e) {
        console.error('Failed to fetch threads:', e);
        this.threads = [];
      }
    },

    async selectThread(name: string) {
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
        const thread = this.threads.find(t => t.name === name);
        if (thread) {
          thread.unreadCount = 0;
        }
        this.updateUnreadCount();

        // Scroll to bottom after messages load
        this.$nextTick(() => this.scrollToBottom());

        // Start polling for new messages in this thread
        messagePoller = createPoller(
          () => getMessages(runName, name),
          async (messages) => {
            const hadNewMessages = messages.length > this.messages.length;
            this.messages = messages;

            if (hadNewMessages) {
              // Mark new messages as read
              await markMessagesRead(runName, name);
              this.$nextTick(() => this.scrollToBottom());
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

    async sendMessage() {
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

    scrollToBottom() {
      const container = this.$el.querySelector('.messages-container');
      if (container) {
        container.scrollTop = container.scrollHeight;
      }
    },

    destroy() {
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

    // Private helper methods
    getRunName(): string | null {
      // Access parent Alpine scope to get selected run
      // This depends on how the parent appState exposes selectedRun
      return (window as any).Alpine?.store?.('app')?.selectedRun ||
             (this.$data as any).selectedRun ||
             null;
    },

    handleRunChange(event: CustomEvent) {
      const runName = event.detail;
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

    updateUnreadCount() {
      const total = this.threads.reduce((sum, t) => sum + t.unreadCount, 0);
      // Dispatch event for header badge
      dispatchEvent('unread-count-changed', { count: total });
    },

    // Alpine.js lifecycle hooks
    $nextTick(fn: () => void) {
      // This will be provided by Alpine.js
      setTimeout(fn, 0);
    },

    $el: null as unknown as HTMLElement,
    $data: null as unknown as Record<string, unknown>,
  } as unknown as ChatPanelState;
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
