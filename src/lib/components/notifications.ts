/**
 * Notifications dropdown component
 * Aggregates unread messages across all runs
 */

import type { RunSummary, Message } from '../types';
import { formatRelativeTime } from '../utils/formatters';

interface Notification {
  id: string;
  runName: string;
  thread: string;
  sender: string;
  content: string;
  timestamp: string;
  read: boolean;
}

/**
 * Notifications component for the header bell icon
 */
export function notifications() {
  return {
    open: false,
    notifications: [] as Notification[],
    totalUnread: 0,
    _pollInterval: null as ReturnType<typeof setInterval> | null,

    async init() {
      await this.fetchNotifications();
      // Poll every 5 seconds for new notifications
      this._pollInterval = setInterval(() => this.fetchNotifications(), 5000);
    },

    destroy() {
      if (this._pollInterval) {
        clearInterval(this._pollInterval);
        this._pollInterval = null;
      }
    },

    toggleNotifications() {
      this.open = !this.open;
      if (this.open) {
        this.fetchNotifications();
      }
    },

    async fetchNotifications() {
      if (!window.tauriInvoke) return;

      try {
        // Get all runs
        const runs = await window.tauriInvoke<RunSummary[]>('get_runs');

        // Collect notifications from runs with unread messages
        const newNotifications: Notification[] = [];
        let total = 0;

        for (const run of runs) {
          if (run.hasUnreadMessages) {
            total++;

            // Get threads for this run to find unread messages
            try {
              const threads = await window.tauriInvoke<Array<{ name: string; unreadCount: number; lastMessage: string | null; lastTimestamp: string | null }>>('get_threads', { runName: run.name });

              for (const thread of threads) {
                if (thread.unreadCount > 0 && thread.lastMessage) {
                  // Get the latest messages to show in notifications
                  const messages = await window.tauriInvoke<Message[]>('get_messages', {
                    runName: run.name,
                    threadName: thread.name,
                    limit: thread.unreadCount,
                  });

                  // Add recent unread messages as notifications
                  for (const msg of messages.slice(-3)) { // Show last 3 unread per thread
                    newNotifications.push({
                      id: `${run.name}-${thread.name}-${msg.id}`,
                      runName: run.name,
                      thread: thread.name,
                      sender: msg.sender,
                      content: msg.content,
                      timestamp: msg.timestamp,
                      read: false,
                    });
                  }
                }
              }
            } catch (e) {
              console.debug('Failed to get threads for', run.name, e);
            }
          }
        }

        // Sort by timestamp, newest first
        newNotifications.sort((a, b) =>
          new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime()
        );

        // Limit to 20 most recent
        this.notifications = newNotifications.slice(0, 20);
        this.totalUnread = total;

        // Update app state
        this.updateAppState(total);
      } catch (e) {
        console.error('Failed to fetch notifications:', e);
      }
    },

    updateAppState(count: number) {
      // @ts-expect-error Alpine.js $el magic property
      let el = this.$el as HTMLElement;
      while (el && el.parentElement) {
        el = el.parentElement;
        // @ts-expect-error Alpine.js internal property
        if (el._x_dataStack) {
          // @ts-expect-error Alpine.js internal property
          const app = el._x_dataStack[0];
          if (app && 'totalUnreadCount' in app) {
            app.totalUnreadCount = count;
          }
          break;
        }
      }
    },

    async goToRun(runName: string) {
      // Dispatch event to select the run
      window.dispatchEvent(new CustomEvent('run-selected', { detail: runName }));
    },

    async markAllRead() {
      if (!window.tauriInvoke) return;

      try {
        // Mark messages as read for each notification
        const runThreads = new Map<string, Set<string>>();

        for (const notif of this.notifications) {
          if (!runThreads.has(notif.runName)) {
            runThreads.set(notif.runName, new Set());
          }
          runThreads.get(notif.runName)!.add(notif.thread);
        }

        for (const [runName, threads] of runThreads) {
          for (const threadName of threads) {
            await window.tauriInvoke('mark_messages_read', {
              runName,
              threadName,
              reader: 'user',
            });
          }
        }

        // Clear notifications
        this.notifications = [];
        this.totalUnread = 0;
        this.updateAppState(0);
      } catch (e) {
        console.error('Failed to mark all read:', e);
      }
    },

    // Use shared formatter (removes "ago" suffix for compact display)
    formatTime(timestamp: string): string {
      const relative = formatRelativeTime(timestamp);
      // Remove " ago" for compact notification display
      return relative.replace(' ago', '').replace('just now', 'now');
    },
  };
}
