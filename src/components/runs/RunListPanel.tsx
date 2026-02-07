/**
 * Run list sidebar panel
 */
import { invoke } from '../../lib/invoke';
import { emit, on } from '../../lib/events';
import {
  type Component,
  For,
  Show,
  createEffect,
  createSignal,
  onCleanup,
} from 'solid-js';
import { cloneRun, createDraft, safeInvokeWithToast } from '../../lib/api';
import type { RunSummary } from '../../lib/types';
import {
  formatElapsed,
  formatProgress,
  formatRelativeTime,
} from '../../lib/utils/formatters';
import {
  getProgressBarClass,
  getStatusBadgeClass,
  getStatusLabel,
} from '../../lib/utils/status';
import { useApp, useRuns } from '../../stores';
import { Icon } from '../shared';
import { RunListItem } from './RunListItem';

export const RunListPanel: Component = () => {
  const app = useApp();
  const runs = useRuns();

  // Context menu state
  const [contextMenu, setContextMenu] = createSignal<{
    visible: boolean;
    x: number;
    y: number;
    run: RunSummary | null;
  }>({ visible: false, x: 0, y: 0, run: null });

  // Clone dialog state
  const [cloneDialog, setCloneDialog] = createSignal<{
    visible: boolean;
    sourceRun: string;
    newName: string;
    loading: boolean;
  }>({ visible: false, sourceRun: '', newName: '', loading: false });

  // Close context menu on click outside
  createEffect(() => {
    const handler = () => setContextMenu((s) => ({ ...s, visible: false }));
    document.addEventListener('click', handler);
    onCleanup(() => document.removeEventListener('click', handler));
  });

  // Listen for create-draft events
  createEffect(() => {
    const cleanup = on('create-draft', () => createNewDraft());
    onCleanup(cleanup);
  });

  const showContextMenu = (e: MouseEvent, run: RunSummary) => {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({
      visible: true,
      x: e.clientX,
      y: e.clientY,
      run,
    });
  };

  const hideContextMenu = () => {
    setContextMenu((s) => ({ ...s, visible: false, run: null }));
  };

  const canPause = () => {
    const status = contextMenu().run?.status;
    return ['working', 'idle', 'waiting', 'eval'].includes(status || '');
  };

  const canResume = () => contextMenu().run?.status === 'paused';

  const canDeliver = () => {
    const status = contextMenu().run?.status;
    return ['done', 'paused', 'working', 'idle', 'waiting', 'timed_out'].includes(
      status || '',
    );
  };

  const contextPause = async () => {
    const run = contextMenu().run;
    if (!run) return;
    const result = await safeInvokeWithToast('pause_run', { runName: run.name }, {
      errorPrefix: 'Failed to pause run',
    });
    if (result.success) await runs.invalidateRuns();
    hideContextMenu();
  };

  const contextResume = async () => {
    const run = contextMenu().run;
    if (!run) return;
    const result = await safeInvokeWithToast('resume_run', { runName: run.name }, {
      errorPrefix: 'Failed to resume run',
    });
    if (result.success) await runs.invalidateRuns();
    hideContextMenu();
  };

  const contextDelete = async () => {
    const run = contextMenu().run;
    if (!run) return;
    hideContextMenu();

    const confirmed = await window.confirmDialog?.delete(run.name, 'run');
    if (!confirmed) return;

    const result = await safeInvokeWithToast('delete_run', { runName: run.name }, {
      errorPrefix: 'Failed to delete run',
    });
    if (result.success) {
      if (runs.selectedRun() === run.name) {
        runs.setSelectedRun(null);
      }
      await runs.invalidateRuns();
    }
  };

  const contextDeliver = async () => {
    const run = contextMenu().run;
    if (!run) return;
    const result = await safeInvokeWithToast<string>('deliver_run', { runName: run.name }, {
      errorPrefix: 'Failed to deliver',
    });
    if (result.success && result.data) {
      window.toast?.success(`Delivered to ${result.data}`);
      await runs.invalidateRuns();
    }
    hideContextMenu();
  };

  const showCloneDialog = () => {
    const run = contextMenu().run;
    if (!run) return;
    setCloneDialog({
      visible: true,
      sourceRun: run.name,
      newName: `${run.name}-copy`,
      loading: false,
    });
    hideContextMenu();
  };

  const hideCloneDialog = () => {
    setCloneDialog({ visible: false, sourceRun: '', newName: '', loading: false });
  };

  const executeClone = async () => {
    const dialog = cloneDialog();
    if (!dialog.sourceRun || !dialog.newName.trim()) return;

    setCloneDialog((s) => ({ ...s, loading: true }));
    try {
      const detail = await cloneRun(dialog.sourceRun, dialog.newName.trim());
      window.toast?.success(`Cloned to "${detail.name}"`);
      await runs.invalidateRuns();
      selectRun(detail.name, 'draft');
      hideCloneDialog();
    } catch (err) {
      const message =
        typeof err === 'string' ? err : (err as Error).message || 'Failed to clone';
      window.toast?.error(message);
      setCloneDialog((s) => ({ ...s, loading: false }));
    }
  };

  const createNewDraft = async () => {
    try {
      const detail = await createDraft();
      window.toast?.success(`Draft "${detail.name}" created`);
      await runs.invalidateRuns();
      selectRun(detail.name, 'draft');
      emit('draft-created');
    } catch (err) {
      window.toast?.error('Failed to create draft');
    }
  };

  const selectRun = (name: string, status: string) => {
    runs.setSelectedRun(name);
    if (status === 'draft') {
      emit('draft-selected', name);
    }
  };

  const getTimeDisplay = (run: RunSummary): string => {
    const completedStatuses = [
      'done',
      'delivered',
      'merged',
      'timed_out',
      'eval_failed',
      'runaway',
    ];

    if (run.status === 'draft') {
      return formatRelativeTime(run.createdAt);
    }
    return formatElapsed(run.elapsedMinutes);
  };

  const formatTimeRemaining = (
    limit: number | null,
    elapsed: number | null,
  ): string => {
    if (!limit) return '';
    const remaining = limit - (elapsed || 0);
    if (remaining <= 0) return '\u26a0 Time up!';
    if (remaining < 60) return `${remaining}m left`;
    const h = Math.floor(remaining / 60);
    const m = remaining % 60;
    return `${h}h${m > 0 ? ` ${m}m` : ''} left`;
  };

  return (
    <aside
      class="run-list-panel flex-shrink-0 border-r border-pasture-600 flex flex-col transition-all duration-200"
      classList={{
        'w-10 collapsed': app.sidebarCollapsed(),
        'w-56': !app.sidebarCollapsed(),
      }}
    >
      {/* Collapsed state */}
      <Show when={app.sidebarCollapsed()}>
        <div class="flex flex-col items-center py-3 h-full">
          <button
            onClick={() => app.toggleSidebar()}
            class="p-2 rounded hover:bg-pasture-700 text-wool-400 hover:text-wool-200"
            data-tooltip="Expand runs panel"
            data-side="right"
          >
            <Icon name="chevrons-right" class="w-4 h-4" />
          </button>
          <div class="w-6 border-t border-pasture-600 my-2" />
          <div class="flex-1 flex flex-col gap-2 overflow-y-auto px-2">
            <For each={runs.runs()}>
              {(run) => (
                <button
                  onClick={() => {
                    selectRun(run.name, run.status);
                    app.toggleSidebar();
                  }}
                  data-tooltip={`${run.name} (${run.status})`}
                  data-side="right"
                  class="w-6 h-6 rounded-md flex items-center justify-center transition-colors shrink-0 border-2 border-transparent"
                  classList={{
                    'bg-pasture-600 border-amber-500':
                      runs.selectedRun() === run.name,
                    'hover:bg-pasture-700': runs.selectedRun() !== run.name,
                  }}
                >
                  <span
                    class="w-2.5 h-2.5 rounded-full"
                    classList={{
                      'status-draft': run.status === 'draft',
                      'status-idle': run.status === 'idle',
                      'status-working':
                        run.status === 'working' || run.status === 'eval',
                      'status-waiting':
                        run.status === 'paused' || run.status === 'waiting',
                      'status-done':
                        run.status === 'done' ||
                        run.status === 'delivered' ||
                        run.status === 'merged',
                      'status-error':
                        run.status === 'runaway' ||
                        run.status === 'timed_out' ||
                        run.status === 'eval_failed',
                    }}
                  />
                </button>
              )}
            </For>
          </div>
        </div>
      </Show>

      {/* Expanded state */}
      <Show when={!app.sidebarCollapsed()}>
        <div class="flex flex-col h-full">
          {/* Runs section header */}
          <div class="p-3 border-b border-pasture-600 flex items-center justify-between">
            <h2 class="text-sm font-medium text-wool-300 uppercase tracking-wide">
              Runs
            </h2>
            <div class="flex items-center gap-1">
              <button
                onClick={createNewDraft}
                class="p-1 rounded hover:bg-pasture-700 text-wool-500 hover:text-wool-300"
                data-tooltip="New draft run"
                data-side="bottom"
                data-test="new-run-btn"
              >
                <Icon name="plus" class="w-3.5 h-3.5" />
              </button>
              <button
                onClick={() => app.toggleSidebar()}
                class="p-1 rounded hover:bg-pasture-700 text-wool-500 hover:text-wool-300"
                data-tooltip="Collapse runs panel"
                data-side="bottom"
              >
                <Icon name="chevrons-left" class="w-3.5 h-3.5" />
              </button>
            </div>
          </div>

          {/* Run list */}
          <div class="flex-1 overflow-y-auto">
            {/* Empty state */}
            <Show when={runs.runs().length === 0 && !runs.loading()}>
              <div class="empty-state p-6 text-center text-wool-500">
                <svg class="w-24 h-12 mx-auto mb-3 text-wool-600">
                  <use href="#sheep-flock" />
                </svg>
                <p class="text-sm font-medium text-wool-400">No runs yet</p>
                <p class="text-xs mt-2">
                  Use <code class="bg-pasture-700 px-1.5 py-0.5 rounded text-wool-300">hirsel go</code> to start
                </p>
              </div>
            </Show>

            {/* Loading state */}
            <Show when={runs.loading()}>
              <div class="space-y-1 p-2">
                <For each={[1, 2, 3, 4]}>
                  {() => (
                    <div class="px-3 py-2.5 border-b border-pasture-800">
                      <div class="flex items-center justify-between gap-2">
                        <div class="flex items-center gap-2">
                          <div class="skeleton w-2 h-2 rounded-full" />
                          <div class="skeleton h-4 w-24" />
                        </div>
                        <div class="skeleton h-4 w-12 rounded" />
                      </div>
                      <div class="flex items-center justify-between mt-1.5">
                        <div class="skeleton h-3 w-32" />
                        <div class="skeleton h-3 w-8" />
                      </div>
                    </div>
                  )}
                </For>
              </div>
            </Show>

            {/* Run items */}
            <For each={runs.runs()}>
              {(run, index) => (
                <div class="stagger-item" style={{ 'animation-delay': `${index() * 50}ms` }}>
                  <RunListItem
                    run={run}
                    selected={runs.selectedRun() === run.name}
                    onSelect={() => selectRun(run.name, run.status)}
                    onContextMenu={(e) => showContextMenu(e, run)}
                    getTimeDisplay={getTimeDisplay}
                    formatTimeRemaining={formatTimeRemaining}
                  />
                </div>
              )}
            </For>
          </div>
        </div>
      </Show>

      {/* Context menu */}
      <Show when={contextMenu().visible}>
        <div
          class="fixed z-50 bg-pasture-700 border border-pasture-600 rounded shadow-lg py-1 min-w-[160px]"
          style={{
            left: `${contextMenu().x}px`,
            top: `${contextMenu().y}px`,
          }}
        >
          <Show when={canPause()}>
            <button
              onClick={contextPause}
              class="w-full px-3 py-1.5 text-left text-sm text-wool-200 hover:bg-pasture-600 flex items-center gap-2"
            >
              <Icon name="pause" class="w-3.5 h-3.5" /> Pause
            </button>
          </Show>
          <Show when={canResume()}>
            <button
              onClick={contextResume}
              class="w-full px-3 py-1.5 text-left text-sm text-wool-200 hover:bg-pasture-600 flex items-center gap-2"
            >
              <Icon name="play" class="w-3.5 h-3.5" /> Resume
            </button>
          </Show>
          <Show when={canPause() || canResume()}>
            <div class="border-t border-pasture-600 my-1" />
          </Show>
          <button
            onClick={showCloneDialog}
            class="w-full px-3 py-1.5 text-left text-sm text-wool-200 hover:bg-pasture-600 flex items-center gap-2"
          >
            <Icon name="copy" class="w-3.5 h-3.5" /> Clone
          </button>
          <Show when={canDeliver()}>
            <button
              onClick={contextDeliver}
              class="w-full px-3 py-1.5 text-left text-sm text-wool-200 hover:bg-pasture-600 flex items-center gap-2"
            >
              <Icon name="git-branch" class="w-3.5 h-3.5" /> Deliver
            </button>
          </Show>
          <div class="border-t border-pasture-600 my-1" />
          <button
            onClick={contextDelete}
            class="w-full px-3 py-1.5 text-left text-sm text-terra hover:bg-pasture-600 flex items-center gap-2"
          >
            <Icon name="trash-2" class="w-3.5 h-3.5" /> Delete
          </button>
        </div>
      </Show>

      {/* Clone dialog */}
      <Show when={cloneDialog().visible}>
        <div
          class="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
          onClick={(e) => {
            if (e.target === e.currentTarget) hideCloneDialog();
          }}
        >
          <div class="bg-pasture-800 border border-pasture-600 rounded-lg shadow-xl p-6 w-[400px]">
            <h3 class="text-lg font-medium text-wool-100 mb-4">Clone Run</h3>
            <p class="text-sm text-wool-400 mb-4">
              Create a new draft from "
              <span class="text-wool-200">{cloneDialog().sourceRun}</span>"
            </p>
            <div class="mb-4">
              <label class="block text-sm text-wool-300 mb-1.5">
                New run name
              </label>
              <input
                type="text"
                value={cloneDialog().newName}
                onInput={(e) =>
                  setCloneDialog((s) => ({ ...s, newName: e.currentTarget.value }))
                }
                onKeyDown={(e) => {
                  if (e.key === 'Enter') executeClone();
                }}
                class="input w-full"
                placeholder="my-new-run"
              />
            </div>
            <div class="flex justify-end gap-2">
              <button
                onClick={hideCloneDialog}
                class="btn btn-ghost"
                disabled={cloneDialog().loading}
              >
                Cancel
              </button>
              <button
                onClick={executeClone}
                class="btn"
                disabled={!cloneDialog().newName.trim() || cloneDialog().loading}
              >
                <Show when={cloneDialog().loading}>
                  <span class="spinner mr-2" />
                </Show>
                Clone
              </button>
            </div>
          </div>
        </div>
      </Show>
    </aside>
  );
};
