/**
 * ProjectSelector - Combobox-style project switcher
 *
 * A refined dropdown for switching between projects with search,
 * keyboard navigation, and quick project creation.
 */
import {
  type Component,
  For,
  Show,
  createEffect,
  createSignal,
  onCleanup,
  onMount,
} from 'solid-js';
import { useProject } from '../../stores';
import { initLucideIcons } from '../../lib/icons';

export const ProjectSelector: Component = () => {
  const project = useProject();

  let triggerRef: HTMLButtonElement | undefined;
  let inputRef: HTMLInputElement | undefined;
  let listRef: HTMLDivElement | undefined;

  const [highlightedIndex, setHighlightedIndex] = createSignal(0);

  // Get items: filtered projects + "New Project" action
  const items = () => {
    const projects = project.filteredProjects();
    return [
      ...projects.map(p => ({ type: 'project' as const, project: p })),
      { type: 'action' as const, action: 'new-project' },
    ];
  };

  // Current project name
  const currentProjectName = () => {
    const selected = project.selectedProject();
    return selected?.name ?? 'Select project';
  };

  const hasProject = () => !!project.selectedProject();

  // Open/close handlers
  const open = () => {
    project.setProjectSelectorOpen(true);
    project.setProjectSearchQuery('');
    setHighlightedIndex(0);
    setTimeout(() => {
      inputRef?.focus();
      initLucideIcons();
    }, 10);
  };

  const close = () => {
    project.setProjectSelectorOpen(false);
    project.setProjectSearchQuery('');
    triggerRef?.focus();
  };

  const toggle = () => {
    if (project.projectSelectorOpen()) {
      close();
    } else {
      open();
    }
  };

  // Selection handlers
  const selectItem = (index: number) => {
    const item = items()[index];
    if (!item) return;

    if (item.type === 'project') {
      project.selectProject(item.project);
      // Also set as focused for the board
      project.setFocusedProjectId(item.project.id);
      close();
    } else if (item.type === 'action' && item.action === 'new-project') {
      project.openProjectSetup();
      close();
    }
  };

  // Keyboard navigation
  const handleKeyDown = (e: KeyboardEvent) => {
    const itemCount = items().length;

    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault();
        setHighlightedIndex(i => (i + 1) % itemCount);
        scrollToHighlighted();
        break;
      case 'ArrowUp':
        e.preventDefault();
        setHighlightedIndex(i => (i - 1 + itemCount) % itemCount);
        scrollToHighlighted();
        break;
      case 'Enter':
        e.preventDefault();
        selectItem(highlightedIndex());
        break;
      case 'Escape':
        e.preventDefault();
        close();
        break;
      case 'Tab':
        close();
        break;
    }
  };

  const scrollToHighlighted = () => {
    if (!listRef) return;
    const highlighted = listRef.querySelector('[data-highlighted="true"]');
    highlighted?.scrollIntoView({ block: 'nearest' });
  };

  // Reset highlight when search changes
  createEffect(() => {
    project.projectSearchQuery();
    setHighlightedIndex(0);
  });

  // Global keyboard shortcut: Cmd+K to open project selector
  createEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault();
        if (project.projectSelectorOpen()) {
          close();
        } else {
          open();
        }
      }
    };

    document.addEventListener('keydown', handler);
    onCleanup(() => document.removeEventListener('keydown', handler));
  });

  // Click outside to close
  createEffect(() => {
    if (!project.projectSelectorOpen()) return;

    const handler = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      if (!target.closest('[data-project-selector]')) {
        close();
      }
    };

    // Delay to avoid immediate close
    setTimeout(() => {
      document.addEventListener('click', handler);
    }, 10);

    onCleanup(() => document.removeEventListener('click', handler));
  });

  onMount(() => initLucideIcons());

  return (
    <div class="relative" data-project-selector>
      {/* Trigger button */}
      <button
        ref={triggerRef}
        type="button"
        onClick={toggle}
        class="flex items-center gap-2 px-2.5 py-1.5 rounded-lg transition-all group"
        classList={{
          'bg-pasture-700/60 text-wool-100': project.projectSelectorOpen(),
          'text-wool-200 hover:text-wool-100 hover:bg-pasture-800/80': !project.projectSelectorOpen(),
        }}
        aria-haspopup="listbox"
        aria-expanded={project.projectSelectorOpen()}
      >
        <Show
          when={hasProject()}
          fallback={
            <span class="text-wool-500 italic text-sm">No project</span>
          }
        >
          <i data-lucide="folder" class="w-4 h-4 text-amber-500/80" />
          <span class="text-sm font-medium max-w-[180px] truncate">
            {currentProjectName()}
          </span>
        </Show>
        <i
          data-lucide="chevron-down"
          class="w-3.5 h-3.5 text-wool-500 transition-transform"
          classList={{ 'rotate-180': project.projectSelectorOpen() }}
        />
      </button>

      {/* Dropdown panel */}
      <Show when={project.projectSelectorOpen()}>
        <div
          class="absolute top-full left-0 mt-2 w-72 rounded-xl overflow-hidden shadow-2xl z-50"
          style={{
            background: 'linear-gradient(180deg, rgba(28,28,30,0.98) 0%, rgba(20,20,22,0.98) 100%)',
            border: '1px solid rgba(255,255,255,0.08)',
            'box-shadow': '0 25px 50px -12px rgba(0,0,0,0.6), 0 0 0 1px rgba(255,255,255,0.05)',
            'backdrop-filter': 'blur(20px)',
          }}
        >
          {/* Search input */}
          <div class="p-2 border-b border-white/5">
            <div class="relative">
              <i
                data-lucide="search"
                class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-wool-600"
              />
              <input
                ref={inputRef}
                type="text"
                placeholder="Search projects..."
                value={project.projectSearchQuery()}
                onInput={(e) => project.setProjectSearchQuery(e.currentTarget.value)}
                onKeyDown={handleKeyDown}
                class="w-full pl-9 pr-3 py-2.5 text-sm rounded-lg bg-black/30 border border-white/5 text-wool-200 placeholder-wool-600 focus:outline-none focus:border-amber-500/30 focus:ring-1 focus:ring-amber-500/20 transition-all"
              />
              <Show when={!project.projectSearchQuery()}>
                <kbd class="absolute right-3 top-1/2 -translate-y-1/2 px-1.5 py-0.5 text-[10px] font-mono text-wool-600 bg-pasture-800 rounded border border-pasture-700">
                  {navigator.platform.includes('Mac') ? '⌘K' : 'Ctrl+K'}
                </kbd>
              </Show>
            </div>
          </div>

          {/* Project list */}
          <div
            ref={listRef}
            class="max-h-64 overflow-y-auto py-1 scrollbar-thin"
            role="listbox"
          >
            <Show
              when={items().length > 1 || items()[0]?.type === 'action'}
              fallback={
                <div class="px-4 py-8 text-center">
                  <i data-lucide="search-x" class="w-8 h-8 mx-auto mb-2 text-wool-700" />
                  <p class="text-sm text-wool-500">No projects found</p>
                </div>
              }
            >
              <For each={items()}>
                {(item, index) => (
                  <Show
                    when={item.type === 'project'}
                    fallback={
                      /* New Project action */
                      <>
                        <div class="mx-2 my-1 border-t border-white/5" />
                        <button
                          type="button"
                          class="w-full flex items-center gap-3 px-3 py-2.5 mx-1 rounded-lg text-left transition-colors"
                          classList={{
                            'bg-amber-500/10 text-amber-400': highlightedIndex() === index(),
                            'text-wool-400 hover:bg-white/5': highlightedIndex() !== index(),
                          }}
                          data-highlighted={highlightedIndex() === index()}
                          onClick={() => selectItem(index())}
                          onMouseEnter={() => setHighlightedIndex(index())}
                          role="option"
                        >
                          <div
                            class="w-8 h-8 rounded-lg flex items-center justify-center"
                            style={{
                              background: highlightedIndex() === index()
                                ? 'rgba(251,191,36,0.15)'
                                : 'rgba(255,255,255,0.05)',
                              border: '1px solid rgba(251,191,36,0.2)',
                            }}
                          >
                            <i data-lucide="plus" class="w-4 h-4" />
                          </div>
                          <div>
                            <div class="text-sm font-medium">New Project</div>
                            <div class="text-xs text-wool-600">Create a new project</div>
                          </div>
                        </button>
                      </>
                    }
                  >
                    {/* Project item */}
                    <button
                      type="button"
                      class="w-full flex items-center gap-3 px-3 py-2 mx-1 rounded-lg text-left transition-colors"
                      classList={{
                        'bg-white/8': highlightedIndex() === index(),
                        'hover:bg-white/5': highlightedIndex() !== index(),
                      }}
                      data-highlighted={highlightedIndex() === index()}
                      onClick={() => selectItem(index())}
                      onMouseEnter={() => setHighlightedIndex(index())}
                      role="option"
                      aria-selected={project.selectedProjectId() === (item as { type: 'project'; project: { id: number } }).project.id}
                    >
                      <div
                        class="w-8 h-8 rounded-lg flex items-center justify-center shrink-0"
                        style={{
                          background: 'linear-gradient(135deg, rgba(251,191,36,0.1) 0%, rgba(212,165,116,0.05) 100%)',
                          border: '1px solid rgba(251,191,36,0.15)',
                        }}
                      >
                        <i data-lucide="folder" class="w-4 h-4 text-amber-500/70" />
                      </div>
                      <div class="flex-1 min-w-0">
                        <div class="text-sm font-medium text-wool-200 truncate">
                          {(item as { type: 'project'; project: { name: string } }).project.name}
                        </div>
                        <Show when={(item as { type: 'project'; project: { description?: string } }).project.description}>
                          <div class="text-xs text-wool-600 truncate">
                            {(item as { type: 'project'; project: { description?: string } }).project.description}
                          </div>
                        </Show>
                      </div>
                      <Show when={project.selectedProjectId() === (item as { type: 'project'; project: { id: number } }).project.id}>
                        <i data-lucide="check" class="w-4 h-4 text-amber-500 shrink-0" />
                      </Show>
                    </button>
                  </Show>
                )}
              </For>
            </Show>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default ProjectSelector;
