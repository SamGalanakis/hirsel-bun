/**
 * RouteSelector - Compact dropdown for switching between routes
 *
 * Design: "Path Markers" - minimal, functional UI that fits Highland Craft aesthetic.
 * Routes appear as subtle navigation aids like trail markers on a hillside.
 */

import { type Component, For, Show, createSignal, createEffect, onCleanup } from 'solid-js';
import { Icon } from '../shared';
import { useRoute } from '../../stores/route-context';
import type { Route } from '../../lib/types';

interface RouteSelectorProps {
  onFork: () => void;
}

export const RouteSelector: Component<RouteSelectorProps> = (props) => {
  const route = useRoute();
  const [isOpen, setIsOpen] = createSignal(false);
  let triggerRef: HTMLButtonElement | undefined;
  let dropdownRef: HTMLDivElement | undefined;

  // Close dropdown when clicking outside
  createEffect(() => {
    if (!isOpen()) return;

    const handleClickOutside = (e: MouseEvent) => {
      if (
        triggerRef &&
        !triggerRef.contains(e.target as Node) &&
        dropdownRef &&
        !dropdownRef.contains(e.target as Node)
      ) {
        setIsOpen(false);
      }
    };

    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setIsOpen(false);
      }
    };

    document.addEventListener('mousedown', handleClickOutside);
    document.addEventListener('keydown', handleEscape);
    onCleanup(() => {
      document.removeEventListener('mousedown', handleClickOutside);
      document.removeEventListener('keydown', handleEscape);
    });
  });

  const handleSelectRoute = async (routeItem: Route) => {
    if (routeItem.id === route.activeRoute()?.id) {
      setIsOpen(false);
      return;
    }

    await route.setActiveRoute(routeItem.id);
    setIsOpen(false);
  };

  const handleFork = () => {
    setIsOpen(false);
    props.onFork();
  };

  return (
    <div class="relative">
      {/* Trigger button */}
      <button
        ref={triggerRef}
        onClick={() => setIsOpen(!isOpen())}
        class="flex items-center gap-1.5 px-2 py-1 rounded text-[11px] font-medium transition-colors"
        style={{
          background: 'rgba(36, 36, 36, 0.6)',
          border: '1px solid rgba(64, 64, 64, 0.5)',
          color: 'var(--wool-300)',
        }}
      >
        <Icon name="git-branch" class="w-3 h-3 text-wool-500" />
        <span>{route.activeRoute()?.name || 'main'}</span>
        <svg
          class={`w-3 h-3 text-wool-500 transition-transform ${isOpen() ? 'rotate-180' : ''}`}
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
        </svg>
      </button>

      {/* Dropdown */}
      <Show when={isOpen()}>
        <div
          ref={dropdownRef}
          class="absolute left-0 top-full mt-1 min-w-[160px] rounded-lg shadow-xl z-50 overflow-hidden"
          style={{
            background: 'linear-gradient(180deg, #2d2d2d 0%, #262626 100%)',
            border: '1px solid rgba(64, 64, 64, 0.6)',
            'box-shadow': '0 8px 24px rgba(0,0,0,0.4)',
          }}
        >
          {/* Routes list */}
          <div class="py-1">
            <For each={route.routes()}>
              {(routeItem) => {
                const isActive = () => routeItem.id === route.activeRoute()?.id;
                return (
                  <button
                    onClick={() => handleSelectRoute(routeItem)}
                    class="w-full flex items-center gap-2 px-3 py-1.5 text-left text-[11px] transition-colors hover:bg-pasture-700/50"
                    style={{
                      color: isActive() ? 'var(--amber-400)' : 'var(--wool-300)',
                    }}
                  >
                    {/* Active indicator dot */}
                    <div
                      class="w-1.5 h-1.5 rounded-full flex-shrink-0"
                      style={{
                        background: isActive() ? 'var(--amber-500)' : 'var(--wool-600)',
                      }}
                    />
                    <span class="truncate flex-1">{routeItem.name}</span>
                    <Show when={isActive()}>
                      <Icon name="check" class="w-3 h-3 text-amber-500 flex-shrink-0" />
                    </Show>
                  </button>
                );
              }}
            </For>
          </div>

          {/* Divider */}
          <div class="h-px bg-pasture-600/50" />

          {/* Fork action */}
          <div class="py-1">
            <button
              onClick={handleFork}
              class="w-full flex items-center gap-2 px-3 py-1.5 text-left text-[11px] transition-colors hover:bg-pasture-700/50"
              style={{ color: 'var(--wool-400)' }}
            >
              <Icon name="copy-plus" class="w-3.5 h-3.5" />
              <span>Fork from current...</span>
            </button>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default RouteSelector;
