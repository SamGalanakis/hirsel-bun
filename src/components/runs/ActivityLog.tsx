/**
 * ActivityLog - Activity log with sorting and fullscreen support
 */
import {
  type Component,
  For,
  Show,
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
} from 'solid-js';
import type { HistoryEntry, SheepConfig, WorkerDisplay } from '../../lib/types';
import { SheepAvatar } from '../shared';
import {
  extractWorkerName,
  formatActionLabel,
  getActionBgClass,
  getActionColor,
  getActionIcon,
} from '../../lib/utils/activity';
import { formatTime } from '../../lib/utils/formatters';
import { Icon } from '../shared';

interface ActivityLogProps {
  history: HistoryEntry[];
  workers: WorkerDisplay[];
  class?: string;
}

export const ActivityLog: Component<ActivityLogProps> = (props) => {
  const [sortDirection, setSortDirection] = createSignal<'asc' | 'desc'>('desc');
  const [isFullscreen, setIsFullscreen] = createSignal(false);
  const [autoScroll, setAutoScroll] = createSignal(true);
  let containerRef: HTMLDivElement | undefined;

  // Sort history based on direction
  const sortedHistory = createMemo(() => {
    const entries = [...props.history];
    if (sortDirection() === 'desc') {
      return entries.sort(
        (a, b) => new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime()
      );
    }
    return entries.sort(
      (a, b) => new Date(a.timestamp).getTime() - new Date(b.timestamp).getTime()
    );
  });

  // Get worker config by name
  const getWorkerConfig = (workerName: string): SheepConfig | null => {
    const worker = props.workers.find((w) => w.name === workerName);
    return worker?.sheepConfig || null;
  };

  // Auto-scroll to bottom when new entries arrive (if enabled and sorting by newest first)
  createEffect(() => {
    if (autoScroll() && containerRef && sortDirection() === 'desc') {
      // For desc order, scroll to top since newest are at top
      containerRef.scrollTop = 0;
    } else if (autoScroll() && containerRef && sortDirection() === 'asc') {
      // For asc order, scroll to bottom since newest are at bottom
      containerRef.scrollTop = containerRef.scrollHeight;
    }
  });

  // Detect manual scroll to disable auto-scroll
  const handleScroll = () => {
    if (!containerRef) return;
    if (sortDirection() === 'desc') {
      // For desc, if not at top, disable auto-scroll
      if (containerRef.scrollTop > 20) {
        setAutoScroll(false);
      }
    } else {
      // For asc, if not at bottom, disable auto-scroll
      const isAtBottom =
        containerRef.scrollHeight - containerRef.scrollTop - containerRef.clientHeight < 20;
      if (!isAtBottom) {
        setAutoScroll(false);
      }
    }
  };

  // Close fullscreen on escape
  createEffect(() => {
    if (!isFullscreen()) return;
    const handleKeydown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setIsFullscreen(false);
      }
    };
    window.addEventListener('keydown', handleKeydown);
    onCleanup(() => window.removeEventListener('keydown', handleKeydown));
  });

  const toggleSort = () => {
    setSortDirection((d) => (d === 'desc' ? 'asc' : 'desc'));
    setAutoScroll(true); // Re-enable auto-scroll when toggling
  };

  // Fullscreen modal wrapper
  const FullscreenWrapper: Component<{ children: any }> = (wrapperProps) => {
    if (!isFullscreen()) return null;
    return (
      <div
        class="fixed inset-0 z-50 flex items-center justify-center bg-black/90 p-8"
        onClick={(e) => {
          if (e.target === e.currentTarget) setIsFullscreen(false);
        }}
      >
        <div class="w-full h-full max-w-4xl bg-pasture-800 border border-pasture-600 rounded-lg flex flex-col overflow-hidden">
          {/* Fullscreen header */}
          <div class="flex items-center justify-between p-3 border-b border-pasture-600">
            <h3 class="text-sm font-medium text-wool-200">Activity Log</h3>
            <div class="flex items-center gap-2">
              <button
                class="p-1.5 rounded hover:bg-pasture-700 text-wool-500 hover:text-wool-300"
                onClick={toggleSort}
                title={sortDirection() === 'desc' ? 'Showing newest first' : 'Showing oldest first'}
              >
                <Icon
                  name={sortDirection() === 'desc' ? 'arrow-down' : 'arrow-up'}
                  class="w-4 h-4"
                />
              </button>
              <button
                class="p-1.5 rounded hover:bg-pasture-700 text-wool-500 hover:text-wool-300"
                onClick={() => setIsFullscreen(false)}
                title="Exit fullscreen"
              >
                <Icon name="minimize-2" class="w-4 h-4" />
              </button>
            </div>
          </div>
          {/* Fullscreen content */}
          <div class="flex-1 overflow-auto" ref={(el) => (containerRef = el)} onScroll={handleScroll}>
            {wrapperProps.children}
          </div>
        </div>
      </div>
    );
  };

  // Activity entry component
  const ActivityEntry: Component<{ entry: HistoryEntry }> = (entryProps) => {
    const workerName = () => extractWorkerName(entryProps.entry.detail);
    const workerConfig = () => (workerName() ? getWorkerConfig(workerName()!) : null);

    return (
      <div
        class={`px-3 py-2 border-b border-pasture-700 last:border-0 activity-entry-hover ${getActionBgClass(
          entryProps.entry.action
        )}`}
      >
        <div class="flex items-start gap-2">
          {/* Worker avatar or action icon */}
          <div class="flex-shrink-0 mt-0.5">
            <Show
              when={workerConfig()}
              fallback={
                <div class={`w-5 h-5 ${getActionColor(entryProps.entry.action)}`}>
                  <Icon name={getActionIcon(entryProps.entry.action)} class="w-5 h-5" />
                </div>
              }
            >
              <SheepAvatar config={workerConfig()!} size={20} class="w-5 h-5" />
            </Show>
          </div>

          {/* Content */}
          <div class="flex-1 min-w-0">
            <div class="flex items-center justify-between gap-2">
              <span class={`text-sm ${getActionColor(entryProps.entry.action)}`}>
                {formatActionLabel(entryProps.entry.action)}
              </span>
              <span class="text-xs text-wool-600 flex-shrink-0">
                {formatTime(entryProps.entry.timestamp)}
              </span>
            </div>
            <Show when={entryProps.entry.detail}>
              <p class="text-xs text-wool-500 mt-0.5 truncate" title={entryProps.entry.detail || ''}>
                {entryProps.entry.detail}
              </p>
            </Show>
          </div>
        </div>
      </div>
    );
  };

  // Empty state
  const EmptyState: Component = () => (
    <div class="flex flex-col items-center justify-center py-8 text-wool-500">
      {/* Sleeping sheep placeholder */}
      <svg
        class="w-16 h-16 mb-3 opacity-40"
        viewBox="0 0 128 128"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
      >
        <ellipse cx="64" cy="80" rx="45" ry="30" fill="currentColor" opacity="0.3" />
        <ellipse cx="64" cy="70" rx="35" ry="25" fill="currentColor" opacity="0.5" />
        <circle cx="50" cy="60" r="3" fill="currentColor" />
        <circle cx="78" cy="60" r="3" fill="currentColor" />
        <path
          d="M58 68 Q64 72 70 68"
          stroke="currentColor"
          stroke-width="2"
          fill="none"
          stroke-linecap="round"
        />
        <text x="85" y="50" font-size="16" fill="currentColor" opacity="0.7">
          z
        </text>
        <text x="95" y="40" font-size="12" fill="currentColor" opacity="0.5">
          z
        </text>
        <text x="102" y="32" font-size="10" fill="currentColor" opacity="0.3">
          z
        </text>
      </svg>
      <p class="text-sm">No activity yet</p>
    </div>
  );

  // Content to render (shared between inline and fullscreen)
  const LogContent = () => (
    <>
      <Show when={sortedHistory().length === 0}>
        <EmptyState />
      </Show>
      <For each={sortedHistory()}>
        {(entry, index) => (
          <div class="stagger-item" style={{ 'animation-delay': `${Math.min(index(), 8) * 30}ms` }}>
            <ActivityEntry entry={entry} />
          </div>
        )}
      </For>
    </>
  );

  return (
    <>
      {/* Fullscreen modal */}
      <Show when={isFullscreen()}>
        <FullscreenWrapper>
          <LogContent />
        </FullscreenWrapper>
      </Show>

      {/* Inline view */}
      <div class={`card p-0 flex flex-col ${props.class || ''}`}>
        {/* Header */}
        <div class="flex items-center justify-between px-3 py-2 border-b border-pasture-700">
          <h3 class="text-sm font-medium text-wool-300">Activity</h3>
          <div class="flex items-center gap-1">
            <button
              class="p-1 rounded hover:bg-pasture-700 text-wool-500 hover:text-wool-300"
              onClick={toggleSort}
              title={sortDirection() === 'desc' ? 'Showing newest first' : 'Showing oldest first'}
            >
              <Icon
                name={sortDirection() === 'desc' ? 'arrow-down' : 'arrow-up'}
                class="w-3.5 h-3.5"
              />
            </button>
            <button
              class="p-1 rounded hover:bg-pasture-700 text-wool-500 hover:text-wool-300"
              onClick={() => setIsFullscreen(true)}
              title="Fullscreen"
            >
              <Icon name="maximize-2" class="w-3.5 h-3.5" />
            </button>
          </div>
        </div>

        {/* Content */}
        <div
          class="flex-1 overflow-auto"
          ref={(el) => !isFullscreen() && (containerRef = el)}
          onScroll={!isFullscreen() ? handleScroll : undefined}
        >
          <LogContent />
        </div>
      </div>
    </>
  );
};
