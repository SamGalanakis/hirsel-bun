/**
 * Notifications dropdown component
 */
import { invoke } from '@tauri-apps/api/core';
import {
  type Component,
  For,
  Show,
  createEffect,
  createSignal,
  onCleanup,
  onMount,
} from 'solid-js';
import type { UnreadNotification } from '../../lib/types';
import { initLucideIcons } from '../../lib/icons';
import { formatTimeShort } from '../../lib/utils/formatters';
import { useRuns } from '../../stores';

export const NotificationsDropdown: Component = () => {
  const runs = useRuns();
  const [open, setOpen] = createSignal(false);
  const [notifications, setNotifications] = createSignal<
    (UnreadNotification & { read: boolean })[]
  >([]);
  const [totalUnread, setTotalUnread] = createSignal(0);

  // Fetch notifications
  const fetchNotifications = async () => {
    try {
      const response = await invoke<{
        notifications: UnreadNotification[];
        totalRunsWithUnread: number;
      }>('get_all_unread_notifications');
      setNotifications(
        response.notifications.map((n) => ({ ...n, read: false })),
      );
      setTotalUnread(response.notifications.length);
    } catch (e) {
      console.error('Failed to fetch notifications:', e);
    }
  };

  // Poll for notifications
  createEffect(() => {
    fetchNotifications();
    const interval = setInterval(fetchNotifications, 5000);
    onCleanup(() => clearInterval(interval));
  });

  const markAllRead = () => {
    setNotifications((notifs) => notifs.map((n) => ({ ...n, read: true })));
    setTotalUnread(0);
  };

  const markOneRead = (id: string) => {
    setNotifications((notifs) =>
      notifs.map((n) => (n.id === id ? { ...n, read: true } : n)),
    );
    setTotalUnread((c) => Math.max(0, c - 1));
  };

  const goToMessage = (runName: string, thread: string) => {
    runs.setSelectedRun(runName);
    window.dispatchEvent(new CustomEvent('switch-tab', { detail: 'chat' }));
    window.dispatchEvent(
      new CustomEvent('select-thread', { detail: thread }),
    );
  };

  const toggleOpen = () => {
    setOpen(!open());
  };

  // Initialize icons
  onMount(() => initLucideIcons());

  createEffect(() => {
    if (open()) {
      queueMicrotask(initLucideIcons);
    }
  });

  // Close on click outside
  let containerRef: HTMLDivElement | undefined;

  const handleClickOutside = (e: MouseEvent) => {
    if (open() && containerRef && !containerRef.contains(e.target as Node)) {
      setOpen(false);
    }
  };

  createEffect(() => {
    document.addEventListener('mousedown', handleClickOutside);
    onCleanup(() => document.removeEventListener('mousedown', handleClickOutside));
  });

  return (
    <div class="relative" ref={(el) => (containerRef = el)}>
      <button
        type="button"
        onClick={toggleOpen}
        class="p-2 rounded-md text-wool-500 hover:text-wool-300 hover:bg-pasture-800 transition-colors relative"
        title="Notifications"
      >
        <i data-lucide="bell" class="w-4 h-4" />
        <Show when={totalUnread() > 0}>
          <span class="absolute -top-1 -right-1 min-w-[18px] h-[18px] bg-terra rounded-full text-[10px] text-white font-bold flex items-center justify-center px-1">
            {totalUnread() > 99 ? '99+' : totalUnread()}
          </span>
        </Show>
      </button>

      {/* Dropdown */}
      <Show when={open()}>
        <div class="absolute right-0 mt-2 w-80 bg-pasture-800 border border-pasture-600 rounded-lg shadow-xl z-50 overflow-hidden">
          <div class="p-3 border-b border-pasture-600 flex items-center justify-between">
            <h3 class="text-sm font-medium text-wool-300">Notifications</h3>
            <Show when={notifications().length > 0}>
              <button
                type="button"
                onClick={markAllRead}
                class="text-xs text-wool-500 hover:text-wool-300"
              >
                Mark all read
              </button>
            </Show>
          </div>

          <div class="max-h-80 overflow-y-auto">
            <For each={notifications()}>
              {(notif) => (
                <div
                  onClick={() => {
                    markOneRead(notif.id);
                    goToMessage(notif.runName, notif.thread);
                    setOpen(false);
                  }}
                  class="p-3 hover:bg-pasture-700 cursor-pointer border-b border-pasture-700 last:border-0 transition-all duration-300 group relative"
                  classList={{
                    'bg-amber-500/10 border-l-2 border-l-amber-500': !notif.read,
                    'opacity-60 bg-pasture-800': notif.read,
                  }}
                >
                  <div class="flex items-center gap-2 mb-1">
                    <span
                      class="text-xs font-medium"
                      classList={{
                        'text-amber-500': !notif.read,
                        'text-wool-500': notif.read,
                      }}
                    >
                      {notif.runName}
                    </span>
                    <span class="text-xs text-wool-600">{notif.thread}</span>
                    <span class="text-xs text-wool-600 ml-auto">
                      {formatTimeShort(notif.timestamp)}
                    </span>
                    <Show when={!notif.read}>
                      <button
                        type="button"
                        onClick={(e) => {
                          e.stopPropagation();
                          markOneRead(notif.id);
                        }}
                        class="opacity-0 group-hover:opacity-100 p-0.5 rounded hover:bg-pasture-600 text-wool-500 hover:text-wool-300 transition-opacity"
                        title="Mark as read"
                      >
                        <i data-lucide="x" class="w-3 h-3" />
                      </button>
                    </Show>
                    <Show when={notif.read}>
                      <span class="text-[10px] text-wool-600 uppercase tracking-wide">
                        read
                      </span>
                    </Show>
                  </div>
                  <p
                    class="text-sm truncate"
                    classList={{
                      'text-wool-300': !notif.read,
                      'text-wool-500': notif.read,
                    }}
                  >
                    {notif.content}
                  </p>
                  <p class="text-xs text-wool-500 mt-0.5">{notif.sender}</p>
                </div>
              )}
            </For>

            <Show when={notifications().length === 0}>
              <div class="p-6 text-center text-wool-500">
                <i data-lucide="bell-off" class="w-8 h-8 mx-auto mb-2 text-wool-600" />
                <p class="text-sm">No notifications</p>
              </div>
            </Show>
          </div>
        </div>
      </Show>
    </div>
  );
};
