import { type Component, For, Match, Show, Switch } from 'solid-js';
import { invoke } from '../../lib/invoke';
import type { WorkItemTree } from '../../lib/types';
import { useRouteWorkTree } from '../../hooks';
import { useProject, useRoute } from '../../stores';
import { Icon } from '../shared';

const statusClass = (status: string) => {
  switch (status) {
    case 'working':
      return 'border-sky-500/40 bg-sky-500/10 text-sky-300';
    case 'done':
    case 'validated':
      return 'border-sage/40 bg-sage/10 text-sage';
    case 'failed':
      return 'border-terra/40 bg-terra/10 text-terra';
    case 'awaiting_check':
      return 'border-amber-500/40 bg-amber-500/10 text-amber-300';
    default:
      return 'border-pasture-700/60 bg-pasture-800/60 text-wool-400';
  }
};

const countItems = (nodes: WorkItemTree[]): number =>
  nodes.reduce((sum, node) => sum + 1 + countItems(node.children), 0);

export const WorkTreePane: Component = () => {
  const project = useProject();
  const route = useRoute();
  const { snapshot, refreshWorkTree } = useRouteWorkTree();

  const createItem = async (parentId?: string) => {
    const projectId = project.selectedProjectId();
    const routeId = route.currentRouteId();
    if (!projectId || !routeId) return;

    const title = window.prompt(parentId ? 'Child work item title' : 'New work item title');
    if (!title || !title.trim()) return;
    const description = window.prompt('Optional description') ?? '';

    try {
      await invoke('create_work_item', {
        projectId,
        routeId,
        parentId: parentId ?? null,
        title: title.trim(),
        description: description.trim() || null,
      });
      await refreshWorkTree();
    } catch (error) {
      console.error('Failed to create work item:', error);
      window.toast?.error(`Failed to create work item: ${error}`);
    }
  };

  const reopenItem = async (itemId: string) => {
    const projectId = project.selectedProjectId();
    const routeId = route.currentRouteId();
    if (!projectId || !routeId) return;
    try {
      await invoke('reopen_work_item', { projectId, routeId, itemId });
      await refreshWorkTree();
    } catch (error) {
      console.error('Failed to reopen work item:', error);
      window.toast?.error(`Failed to reopen work item: ${error}`);
    }
  };

  const archiveItem = async (itemId: string) => {
    const projectId = project.selectedProjectId();
    const routeId = route.currentRouteId();
    if (!projectId || !routeId) return;
    try {
      await invoke('archive_work_item', { projectId, routeId, itemId });
      await refreshWorkTree();
    } catch (error) {
      console.error('Failed to archive work item:', error);
      window.toast?.error(`Failed to archive work item: ${error}`);
    }
  };

  const ItemNode: Component<{ item: WorkItemTree; depth: number }> = (props) => (
    <div class="space-y-2">
      <article
        class="rounded-none border border-pasture-700/60 bg-pasture-800/45 px-4 py-3"
        style={{ 'margin-left': `${props.depth * 16}px` }}
      >
        <div class="flex flex-wrap items-start gap-2">
          <span class={`rounded-none border px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] ${statusClass(props.item.status)}`}>
            {props.item.status.replaceAll('_', ' ')}
          </span>
          <Show when={props.item.assignee}>
            {(assignee) => (
              <span class="rounded-none border border-pasture-700/60 bg-pasture-900/60 px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-wool-500">
                {assignee().kind}: {assignee().id}
              </span>
            )}
          </Show>
          <Show when={props.item.claimedBy}>
            {(worker) => (
              <span class="rounded-none border border-sky-500/40 bg-sky-500/10 px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-sky-300">
                worker: {worker()}
              </span>
            )}
          </Show>
        </div>

        <div class="mt-3 flex items-start gap-3">
          <div class="mt-0.5 text-wool-500">
            <Icon name="blocks" class="h-4 w-4" />
          </div>
          <div class="min-w-0 flex-1">
            <p class="text-sm font-medium text-wool-100">{props.item.title}</p>
            <Show when={props.item.description}>
              <p class="mt-2 whitespace-pre-wrap text-sm leading-6 text-wool-400">
                {props.item.description}
              </p>
            </Show>
            <Show when={props.item.blockedBy.length > 0}>
              <p class="mt-2 text-xs text-wool-500">
                Blocked by: {props.item.blockedBy.join(', ')}
              </p>
            </Show>
          </div>
        </div>

        <div class="mt-3 flex flex-wrap items-center gap-2">
          <button
            type="button"
            onClick={() => void createItem(props.item.id)}
            class="rounded-none border border-pasture-700/60 bg-pasture-900/60 px-2.5 py-1 text-xs text-wool-300 hover:bg-pasture-700"
          >
            Add child
          </button>
          <Show when={['done', 'validated', 'failed', 'awaiting_check'].includes(props.item.status)}>
            <button
              type="button"
              onClick={() => void reopenItem(props.item.id)}
              class="rounded-none border border-pasture-700/60 bg-pasture-900/60 px-2.5 py-1 text-xs text-wool-300 hover:bg-pasture-700"
            >
              Reopen
            </button>
          </Show>
          <button
            type="button"
            onClick={() => void archiveItem(props.item.id)}
            class="rounded-none border border-pasture-700/60 bg-pasture-900/60 px-2.5 py-1 text-xs text-wool-400 hover:bg-pasture-700 hover:text-wool-200"
          >
            Archive
          </button>
          <Show when={props.item.completedAt}>
            <span class="ml-auto text-xs text-wool-500">
              {new Date(props.item.completedAt!).toLocaleString()}
            </span>
          </Show>
        </div>
        <Show when={props.item.capabilityProfile}>
          {(profile) => (
            <div class="mt-2 text-[11px] uppercase tracking-[0.16em] text-wool-600">
              capability: {profile().replaceAll('_', ' ')}
            </div>
          )}
        </Show>
      </article>

      <For each={props.item.children}>{(child) => <ItemNode item={child} depth={props.depth + 1} />}</For>
    </div>
  );

  return (
    <div class="h-full min-h-0 flex flex-col bg-pasture-900/85">
      <div class="flex items-center gap-3 border-b border-pasture-700/60 px-5 py-3">
        <div>
          <p class="text-[11px] uppercase tracking-[0.18em] text-amber-400">Work Tree</p>
          <p class="mt-1 text-sm text-wool-500">
            Route-scoped execution tree for the selected route.
          </p>
        </div>
        <div class="ml-auto flex items-center gap-3 text-xs text-wool-500">
          <Show when={snapshot()}>
            {(state) => (
              <>
                <span>{countItems(state().tree)} items</span>
              </>
            )}
          </Show>
          <button
            type="button"
            onClick={() => void createItem()}
            class="rounded-none border border-pasture-700/60 bg-pasture-800/70 px-3 py-2 text-sm text-wool-300 hover:bg-pasture-700"
          >
            New item
          </button>
        </div>
      </div>

      <div class="min-h-0 flex-1 overflow-y-auto p-4">
        <Switch>
          <Match when={!snapshot()}>
            <div class="flex h-full items-center justify-center text-sm text-wool-500">
              Loading route work tree…
            </div>
          </Match>
          <Match when={snapshot() && snapshot()!.tree.length === 0}>
            <div class="flex h-full items-center justify-center rounded-none border border-pasture-700/60 bg-pasture-800/30 text-sm text-wool-500">
              No work items on this route yet.
            </div>
          </Match>
          <Match when={snapshot()}>
            <div class="space-y-3">
              <For each={snapshot()!.tree}>{(item) => <ItemNode item={item} depth={0} />}</For>
            </div>
          </Match>
        </Switch>
      </div>
    </div>
  );
};
