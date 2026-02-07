/**
 * LeftDrawer - Collapsible left sidebar for navigation
 *
 * Contains:
 * - PROJECT section: Current project, click to switch
 * - ROUTES section: List of routes for the project
 * - Collapse toggle to minimize to icon rail
 */
import { type Component, Show, For, createSignal, createEffect, onCleanup } from 'solid-js';
import { emit } from '../../lib/events';
import { useApp, useProject, useRoute } from '../../stores';
import { Icon } from '../shared';
import type { Route } from '../../lib/types';

export const LeftDrawer: Component = () => {
  const app = useApp();
  const project = useProject();
  const route = useRoute();

  const isCollapsed = () => app.sidebarCollapsed();

  // Track which project selector is hovered for expansion
  const [projectHovered, setProjectHovered] = createSignal(false);

  const handleRouteSelect = async (routeItem: Route) => {
    if (routeItem.id === route.activeRoute()?.id) return;
    await route.setActiveRoute(routeItem.id);
  };

  const handleFork = () => {
    // Dispatch event to open fork dialog
    emit('open-fork-dialog');
  };

  const handleOpenChat = () => {
    project.setActiveThread('chat');
    project.setSheepfoldOpen(true);
  };

  return (
    <aside
      class="left-drawer flex flex-col border-r border-pasture-600/50 bg-pasture-800 transition-all duration-200 ease-out select-none"
      style={{
        width: isCollapsed() ? '48px' : '200px',
        'min-width': isCollapsed() ? '48px' : '200px',
      }}
    >
      {/* PROJECT Section */}
      <Show when={project.selectedProject()}>
        <section class="drawer-section p-2 border-b border-pasture-600/30">
          <Show when={!isCollapsed()}>
            <h3 class="drawer-section-label text-[10px] uppercase tracking-wider text-wool-600 font-medium px-2 mb-1">
              Project
            </h3>
          </Show>
          <button
            onClick={() => project.setProjectSelectorOpen(true)}
            onMouseEnter={() => setProjectHovered(true)}
            onMouseLeave={() => setProjectHovered(false)}
            class="w-full flex items-center gap-2 px-2 py-1.5 rounded hover:bg-pasture-700 transition-colors"
            title={project.selectedProject()?.name}
          >
            <Icon name="folder" class="w-4 h-4 text-amber-500 flex-shrink-0" />
            <Show when={!isCollapsed()}>
              <span class="text-[12px] text-wool-200 truncate flex-1 text-left">
                {project.selectedProject()?.name}
              </span>
              <Icon name="chevron-down" class="w-3 h-3 text-wool-500" />
            </Show>
          </button>
        </section>
      </Show>

      {/* ROUTES Section */}
      <Show when={project.selectedProject()}>
        <section class="drawer-section flex-1 overflow-y-auto p-2">
          <Show when={!isCollapsed()}>
            <div class="flex items-center justify-between px-2 mb-1">
              <h3 class="drawer-section-label text-[10px] uppercase tracking-wider text-wool-600 font-medium">
                Routes
              </h3>
              <button
                onClick={handleFork}
                class="p-1 rounded hover:bg-pasture-700 transition-colors"
                title="Fork current route"
              >
                <Icon name="copy-plus" class="w-3 h-3 text-wool-500" />
              </button>
            </div>
          </Show>

          <div class="space-y-0.5">
            <For each={route.routes()}>
              {(routeItem) => {
                const isActive = () => routeItem.id === route.activeRoute()?.id;
                return (
                  <button
                    onClick={() => handleRouteSelect(routeItem)}
                    class="route-item w-full flex items-center gap-2 px-2 py-1.5 rounded text-left transition-colors"
                    classList={{
                      'bg-pasture-700/50': isActive(),
                      'hover:bg-pasture-700/30': !isActive(),
                    }}
                    style={{
                      'border-left': isActive() ? '2px solid var(--amber-500)' : '2px solid transparent',
                    }}
                    title={routeItem.name}
                  >
                    <Icon
                      name="git-branch"
                      class={`w-3.5 h-3.5 flex-shrink-0 ${isActive() ? 'text-amber-500' : 'text-wool-500'}`}
                    />
                    <Show when={!isCollapsed()}>
                      <span
                        class="text-[12px] truncate flex-1"
                        classList={{
                          'text-amber-400': isActive(),
                          'text-wool-300': !isActive(),
                        }}
                      >
                        {routeItem.name}
                      </span>
                      <Show when={isActive()}>
                        <Icon name="check" class="w-3 h-3 text-amber-500 flex-shrink-0" />
                      </Show>
                    </Show>
                  </button>
                );
              }}
            </For>
          </div>
        </section>
      </Show>

      {/* Bottom Actions */}
      <div class="mt-auto border-t border-pasture-600/30 p-2 space-y-0.5">
        {/* Chat */}
        <button
          onClick={handleOpenChat}
          class="w-full flex items-center gap-2 px-2 py-1.5 rounded hover:bg-pasture-700 transition-colors relative"
          title="Chat"
        >
          <Icon name="message-circle" class="w-4 h-4 text-wool-500 flex-shrink-0" />
          <Show when={!isCollapsed()}>
            <span class="text-[12px] text-wool-400">Chat</span>
          </Show>
          {/* Unread badge */}
          <Show when={project.projectUnreadCount() > 0}>
            <span
              class="absolute min-w-[16px] h-4 px-1 rounded-full text-[9px] font-bold flex items-center justify-center"
              style={{
                background: 'var(--amber-500)',
                color: 'var(--pasture-900)',
                right: isCollapsed() ? '2px' : '8px',
                top: '2px',
              }}
            >
              {project.projectUnreadCount() > 99 ? '99+' : project.projectUnreadCount()}
            </span>
          </Show>
        </button>

        {/* Docs */}
        <Show when={project.selectedProject()}>
          <button
            onClick={() => project.setDocsOpen(!project.docsOpen())}
            class="w-full flex items-center gap-2 px-2 py-1.5 rounded transition-colors"
            classList={{
              'bg-pasture-700/50 text-sage-400': project.docsOpen(),
              'hover:bg-pasture-700 text-wool-500': !project.docsOpen(),
            }}
            title="Project documentation"
          >
            <Icon name="book-open" class="w-4 h-4 flex-shrink-0" />
            <Show when={!isCollapsed()}>
              <span class="text-[12px]">Docs</span>
            </Show>
          </button>
        </Show>

        {/* Project Settings */}
        <Show when={project.selectedProject()}>
          <button
            onClick={() => project.setShowProjectSettings(true)}
            class="w-full flex items-center gap-2 px-2 py-1.5 rounded hover:bg-pasture-700 transition-colors"
            title={`${project.selectedProject()?.name} settings`}
          >
            <Icon name="folder-cog" class="w-4 h-4 text-wool-500 flex-shrink-0" />
            <Show when={!isCollapsed()}>
              <span class="text-[12px] text-wool-400">Project Settings</span>
            </Show>
          </button>
        </Show>
      </div>
    </aside>
  );
};

export default LeftDrawer;
