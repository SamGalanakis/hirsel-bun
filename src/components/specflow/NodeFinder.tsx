/**
 * NodeFinder - lightweight search + jump for SpecBoard nodes
 *
 * Opened via `/` on the SpecBoard canvas.
 */
import {
  type Component,
  For,
  Show,
  createEffect,
  createSignal,
  onCleanup,
} from 'solid-js';
import { Icon } from '../shared';

export interface NodeFinderItem {
  id: string;
  name: string;
  kind: 'feature' | 'task' | 'check';
  status: string;
  claimedBy: string | null;
}

export const NodeFinder: Component<{
  open: boolean;
  query: string;
  setQuery: (v: string) => void;
  items: NodeFinderItem[];
  onSelect: (id: string) => void;
  onClose: () => void;
}> = (props) => {
  let inputRef: HTMLInputElement | undefined;
  let listRef: HTMLDivElement | undefined;
  const [highlightedIndex, setHighlightedIndex] = createSignal(0);

  const platformHint = () =>
    navigator.platform.includes('Mac') ? 'Esc' : 'Esc';

  const kindLabel = (k: NodeFinderItem['kind']) =>
    k === 'feature' ? 'Feature' : k === 'check' ? 'Check' : 'Task';

  const statusColor = (status: string) => {
    switch (status) {
      case 'working': return 'var(--amber-500)';
      case 'done':
      case 'validated': return 'var(--sage)';
      case 'awaiting_check': return 'var(--amber-400)';
      case 'needs_repair': return 'var(--golden)';
      case 'failed': return 'var(--terra)';
      default: return 'var(--wool-600)';
    }
  };

  const scrollToHighlighted = () => {
    if (!listRef) return;
    const el = listRef.querySelector('[data-highlighted="true"]');
    el?.scrollIntoView({ block: 'nearest' });
  };

  createEffect(() => {
    if (!props.open) return;
    setHighlightedIndex(0);
    props.setQuery('');
    setTimeout(() => inputRef?.focus(), 10);
  });

  createEffect(() => {
    // Reset highlight on query change.
    props.query;
    setHighlightedIndex(0);
  });

  createEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (!props.open) return;

      const target = e.target as HTMLElement;
      if (target?.tagName === 'INPUT' || target?.tagName === 'TEXTAREA') {
        // We'll still handle navigation keys when the finder input is focused.
      }

      const count = props.items.length;
      if (e.key === 'Escape') {
        e.preventDefault();
        props.onClose();
        return;
      }

      if (count === 0) return;

      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setHighlightedIndex(i => (i + 1) % count);
        scrollToHighlighted();
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        setHighlightedIndex(i => (i - 1 + count) % count);
        scrollToHighlighted();
        return;
      }
      if (e.key === 'Enter') {
        e.preventDefault();
        const item = props.items[highlightedIndex()];
        if (item) props.onSelect(item.id);
      }
    };

    document.addEventListener('keydown', handler);
    onCleanup(() => document.removeEventListener('keydown', handler));
  });

  return (
    <Show when={props.open}>
      <div
        class="fixed inset-0 z-[120] bg-black/40"
        onClick={(e) => {
          if (e.target === e.currentTarget) props.onClose();
        }}
      >
        <div
          class="fixed left-1/2 top-[90px] -translate-x-1/2 w-[560px] rounded-xl overflow-hidden"
          style={{
            background: 'linear-gradient(180deg, rgba(28,28,30,0.98) 0%, rgba(20,20,22,0.98) 100%)',
            border: '1px solid rgba(255,255,255,0.08)',
            'box-shadow': '0 25px 50px -12px rgba(0,0,0,0.7), 0 0 0 1px rgba(255,255,255,0.05)',
            'backdrop-filter': 'blur(20px)',
          }}
        >
          <div class="p-2 border-b border-white/5">
            <div class="relative">
              <Icon name="search" class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-wool-600" />
              <input
                ref={inputRef}
                value={props.query}
                onInput={(e) => props.setQuery(e.currentTarget.value)}
                placeholder="Search nodes..."
                class="w-full pl-9 pr-14 py-2.5 text-sm rounded-lg bg-black/30 border border-white/5 text-wool-200 placeholder-wool-600 focus:outline-none focus:border-amber-500/30 focus:ring-1 focus:ring-amber-500/20 transition-all"
              />
              <kbd class="absolute right-3 top-1/2 -translate-y-1/2 px-1.5 py-0.5 text-[10px] font-mono text-wool-600 bg-pasture-800 rounded border border-pasture-700">
                {platformHint()}
              </kbd>
            </div>
          </div>

          <div ref={listRef} class="max-h-[380px] overflow-y-auto py-1 scrollbar-thin">
            <Show
              when={props.items.length > 0}
              fallback={
                <div class="px-4 py-10 text-center">
                  <Icon name="search-x" class="w-8 h-8 mx-auto mb-2 text-wool-700" />
                  <p class="text-sm text-wool-500">No matches</p>
                </div>
              }
            >
              <For each={props.items}>
                {(item, index) => (
                  <button
                    type="button"
                    class="w-full flex items-center gap-3 px-3 py-2 mx-1 rounded-lg text-left transition-colors"
                    classList={{
                      'bg-white/8': highlightedIndex() === index(),
                      'hover:bg-white/5': highlightedIndex() !== index(),
                    }}
                    data-highlighted={highlightedIndex() === index()}
                    onMouseEnter={() => setHighlightedIndex(index())}
                    onClick={() => props.onSelect(item.id)}
                  >
                    <div
                      class="w-8 h-8 rounded-lg flex items-center justify-center shrink-0"
                      style={{
                        background: 'rgba(255,255,255,0.05)',
                        border: '1px solid rgba(255,255,255,0.08)',
                      }}
                    >
                      <span class="text-[9px] font-semibold uppercase tracking-wider text-wool-500">
                        {item.kind === 'feature' ? 'F' : item.kind === 'check' ? 'C' : 'T'}
                      </span>
                    </div>

                    <div class="flex-1 min-w-0">
                      <div class="text-sm font-medium text-wool-200 truncate">
                        {item.name || 'Untitled'}
                      </div>
                      <div class="flex items-center gap-2 text-[11px] text-wool-600">
                        <span>{kindLabel(item.kind)}</span>
                        <span class="inline-flex items-center gap-1">
                          <span class="w-1.5 h-1.5 rounded-full" style={{ background: statusColor(item.status) }} />
                          <span>{item.status.replace('_', ' ')}</span>
                        </span>
                        <Show when={item.claimedBy}>
                          <span class="truncate">by {item.claimedBy}</span>
                        </Show>
                      </div>
                    </div>
                  </button>
                )}
              </For>
            </Show>
          </div>
        </div>
      </div>
    </Show>
  );
};

export default NodeFinder;
