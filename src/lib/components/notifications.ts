/**
 * Notifications dropdown component
 * Aggregates unread messages across all runs
 */

import { DATA_EVENTS, dataCache } from '../data-cache';
import type { UnreadNotification, UnreadNotificationsResponse } from '../types';
import { formatRelativeTime } from '../utils/formatters';

declare const lucide: { createIcons(options?: { inTemplates?: boolean }): void } | undefined;

interface Notification extends UnreadNotification {
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
    _eventCleanups: [] as (() => void)[],
    _cacheUnsubscribe: null as (() => void) | null,
    _observer: null as IntersectionObserver | null,
    _visibleTimers: new Map<string, ReturnType<typeof setTimeout>>(),
    _pendingObserve: [] as HTMLElement[], // Queue for elements waiting to be observed

    async init() {
      // Subscribe to cache (for subscriber count)
      this._cacheUnsubscribe = dataCache.subscribe();

      // Fetch initial notifications
      await this.fetchNotifications();

      // Poll less frequently since this is a summary view (10 seconds)
      this._pollInterval = setInterval(() => this.fetchNotifications(), 10000);

      // Observer will be set up when dropdown opens with proper root
    },

    // Called from x-ref when dropdown opens to set the scroll container
    setScrollRoot(el: HTMLElement | null) {
      this._setupObserver(el);
    },

    destroy() {
      if (this._pollInterval) {
        clearInterval(this._pollInterval);
        this._pollInterval = null;
      }
      this._eventCleanups.forEach((fn) => fn());
      this._eventCleanups = [];
      if (this._cacheUnsubscribe) {
        this._cacheUnsubscribe();
        this._cacheUnsubscribe = null;
      }
      if (this._observer) {
        this._observer.disconnect();
        this._observer = null;
      }
      // Clear any pending timers and queued elements
      this._visibleTimers.forEach((timer) => clearTimeout(timer));
      this._visibleTimers.clear();
      this._pendingObserve = [];
    },

    _setupObserver(root: HTMLElement | null = null) {
      // Disconnect existing observer if any
      if (this._observer) {
        this._observer.disconnect();
      }

      // Clear any pending timers
      this._visibleTimers.forEach((timer) => clearTimeout(timer));
      this._visibleTimers.clear();

      // Create intersection observer that marks notifications as read when visible
      this._observer = new IntersectionObserver(
        (entries) => {
          entries.forEach((entry) => {
            const notifId = (entry.target as HTMLElement).dataset.notifId;
            if (!notifId) return;

            if (entry.isIntersecting) {
              // Start timer - mark as read after 800ms of being visible
              if (!this._visibleTimers.has(notifId)) {
                const timer = setTimeout(() => {
                  this.markOneRead(notifId);
                  this._visibleTimers.delete(notifId);
                }, 800);
                this._visibleTimers.set(notifId, timer);
              }
            } else {
              // Scrolled out of view - cancel timer
              const timer = this._visibleTimers.get(notifId);
              if (timer) {
                clearTimeout(timer);
                this._visibleTimers.delete(notifId);
              }
            }
          });
        },
        {
          root: root, // Use the scrollable container as root
          threshold: 0.5, // 50% visible
        },
      );

      // Process any elements that were queued before observer was ready
      for (const el of this._pendingObserve) {
        this._observer.observe(el);
      }
      this._pendingObserve = [];
    },

    observeNotification(el: HTMLElement) {
      if (!el) return;

      if (this._observer) {
        this._observer.observe(el);
      } else {
        // Queue for later when observer is set up
        this._pendingObserve.push(el);
      }
    },

    toggleNotifications() {
      this.open = !this.open;
      if (this.open) {
        this.fetchNotifications();
        // Re-render icons for dismiss buttons after a tick
        setTimeout(() => {
          if (typeof lucide !== 'undefined') {
            lucide.createIcons({ inTemplates: true });
          }
        }, 0);
      }
    },

    async fetchNotifications() {
      if (!window.tauriInvoke) return;

      try {
        // Single API call to get all unread notifications
        const response = await window.tauriInvoke<UnreadNotificationsResponse>(
          'get_all_unread_notifications',
        );

        // Keep existing read notifications
        const existingRead = this.notifications.filter((n) => n.read);

        // Convert new unread to internal format
        const newUnread: Notification[] = response.notifications.map((n) => ({
          ...n,
          read: false,
        }));

        // Merge: new unread first, then existing read (avoid duplicates)
        const unreadIds = new Set(newUnread.map((n) => n.id));
        const filteredRead = existingRead.filter((n) => !unreadIds.has(n.id));

        // Combine and cap at 100
        this.notifications = [...newUnread, ...filteredRead].slice(0, 100);
        this.totalUnread = newUnread.length;

        // Update app state
        this.updateAppState(response.totalRunsWithUnread);

        // Re-render icons for dismiss buttons
        setTimeout(() => {
          if (typeof lucide !== 'undefined') {
            lucide.createIcons({ inTemplates: true });
          }
        }, 0);
      } catch (e) {
        console.error('Failed to fetch notifications:', e);
      }
    },

    updateAppState(count: number) {
      // @ts-expect-error Alpine.js $el magic property
      let el = this.$el as HTMLElement;
      while (el?.parentElement) {
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

    async goToMessage(runName: string, thread: string) {
      // 1. Select the run
      window.dispatchEvent(new CustomEvent('run-selected', { detail: runName }));

      // 2. Switch to chat tab
      window.dispatchEvent(new CustomEvent('switch-tab', { detail: 'chat' }));

      // 3. Select the thread (with small delay to allow run to load)
      setTimeout(() => {
        window.dispatchEvent(
          new CustomEvent('select-chat-thread', {
            detail: { runName, thread },
          }),
        );
      }, 300);
    },

    async markAllRead() {
      if (!window.tauriInvoke) return;

      // Mark all as read locally first for immediate feedback
      const unreadNotifs = this.notifications.filter((n) => !n.read);
      if (unreadNotifs.length === 0) return;

      // Optimistic update - mark all as read
      for (const notif of unreadNotifs) {
        notif.read = true;
      }
      this.totalUnread = 0;
      this.updateAppState(0);

      try {
        // Collect unique run/thread pairs to mark as read in backend
        const runThreads = new Map<string, Set<string>>();

        for (const notif of unreadNotifs) {
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
      } catch (e) {
        console.error('Failed to mark all read:', e);
        // Revert on error
        for (const notif of unreadNotifs) {
          notif.read = false;
        }
        this.totalUnread = unreadNotifs.length;
        this.updateAppState(unreadNotifs.length);
      }
    },

    async markOneRead(notifId: string) {
      const notif = this.notifications.find((n) => n.id === notifId);
      if (!notif || notif.read) return;

      // Mark as read locally first for immediate feedback
      notif.read = true;

      // Update count immediately
      const unreadCount = this.notifications.filter((n) => !n.read).length;
      this.totalUnread = unreadCount;
      this.updateAppState(unreadCount);

      try {
        // Mark in backend
        await window.tauriInvoke('mark_messages_read', {
          runName: notif.runName,
          threadName: notif.thread,
          reader: 'user',
        });
      } catch (e) {
        console.error('Failed to mark notification read:', e);
        // Revert on error
        notif.read = false;
        // Restore count
        const revertCount = this.notifications.filter((n) => !n.read).length;
        this.totalUnread = revertCount;
        this.updateAppState(revertCount);
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
