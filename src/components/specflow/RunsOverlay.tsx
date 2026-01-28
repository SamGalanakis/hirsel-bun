/**
 * RunsOverlay - Floating panel showing active runs for the current project
 *
 * Positioned in bottom-right corner of the board canvas.
 * - Collapsed: small button with run count + pulse animation
 * - Expanded: list of active runs with status/progress/actions
 */

import { invoke } from '@tauri-apps/api/core';
import {
  type Component,
  For,
  Show,
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
} from 'solid-js';
import { useRuns } from '../../stores';
import type { RunSummary, TaskRun } from '../../lib/types';

interface RunsOverlayProps {
  projectId: number | null;
  onViewRun?: (runName: string) => void;
}

/** Get status color for run status */
const getStatusColor = (status: string): string => {
  switch (status) {
    case 'working':
      return 'bg-amber-500';
    case 'eval':
      return 'bg-amber-400';
    case 'paused':
      return 'bg-wool-500';
    case 'done':
      return 'bg-sage';
    case 'delivered':
      return 'bg-sage';
    case 'merged':
      return 'bg-sage';
    case 'failed':
      return 'bg-terra';
    case 'idle':
      return 'bg-wool-500';
    case 'waiting':
      return 'bg-golden';
    default:
      return 'bg-wool-600';
  }
};

/** Format elapsed time */
const formatElapsed = (minutes: number): string => {
  const rounded = Math.round(minutes);
  if (rounded < 60) return `${rounded}m`;
  const hours = Math.floor(rounded / 60);
  const mins = rounded % 60;
  return mins > 0 ? `${hours}h ${mins}m` : `${hours}h`;
};

/** Check if run is "active" (working/in-progress) */
const isActiveRun = (status: string): boolean => {
  return ['working', 'eval', 'paused', 'idle', 'waiting'].includes(status);
};

/** Check if run is "completed" (finished, one way or another) */
const isCompletedRun = (status: string): boolean => {
  return ['done', 'delivered', 'merged', 'failed'].includes(status);
};

export const RunsOverlay: Component<RunsOverlayProps> = (props) => {
  const runs = useRuns();
  const [expanded, setExpanded] = createSignal(false);
  const [taskRuns, setTaskRuns] = createSignal<TaskRun[]>([]);

  // Subscribe to runs polling
  createEffect(() => {
    const unsubscribe = runs.subscribe();
    onCleanup(unsubscribe);
  });

  // Fetch task runs for project
  const fetchTaskRuns = async (pid: number) => {
    try {
      const result = await invoke<TaskRun[]>('get_all_task_runs', { projectId: pid });
      setTaskRuns(result);
    } catch (e) {
      console.error('Failed to fetch task runs:', e);
    }
  };

  // Load task runs for project when projectId changes, and poll periodically
  createEffect(() => {
    const pid = props.projectId;
    if (pid) {
      // Initial fetch
      fetchTaskRuns(pid);

      // Poll every 5 seconds to pick up new dispatches
      const interval = setInterval(() => fetchTaskRuns(pid), 5000);
      onCleanup(() => clearInterval(interval));
    } else {
      setTaskRuns([]);
    }
  });

  // Get run names associated with this project
  const projectRunNames = createMemo(() => {
    return new Set(taskRuns().map((tr) => tr.runName));
  });

  // Filter runs to project runs (active first, then completed)
  const projectRuns = createMemo(() => {
    const runNames = projectRunNames();
    if (runNames.size === 0) return [];

    const allRuns = runs.runs().filter((r) => runNames.has(r.name));
    // Sort: active runs first, then completed, each group sorted by most recent
    return allRuns.sort((a, b) => {
      const aActive = isActiveRun(a.status);
      const bActive = isActiveRun(b.status);
      if (aActive && !bActive) return -1;
      if (!aActive && bActive) return 1;
      return 0; // Keep original order within groups
    });
  });

  // Active runs only (for badge count)
  const activeRuns = createMemo(() =>
    projectRuns().filter((r) => isActiveRun(r.status))
  );

  // Count of active runs (for badge)
  const activeCount = () => activeRuns().length;

  // Total project runs count
  const totalCount = () => projectRuns().length;

  // Any working runs (for pulse animation)
  const hasWorkingRuns = () =>
    activeRuns().some((r) => r.status === 'working' || r.status === 'eval');

  // Keyboard handler
  createEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') return;

      if (e.key === 'r' && !e.metaKey && !e.ctrlKey && !e.altKey) {
        setExpanded((prev) => !prev);
      }
      if (e.key === 'Escape' && expanded()) {
        setExpanded(false);
      }
    };

    document.addEventListener('keydown', handler);
    onCleanup(() => document.removeEventListener('keydown', handler));
  });

  // Handle view run click
  const handleViewRun = (runName: string) => {
    if (props.onViewRun) {
      props.onViewRun(runName);
    } else {
      // Dispatch run-selected event to navigate
      window.dispatchEvent(new CustomEvent('run-selected', { detail: runName }));
    }
    setExpanded(false);
  };

  // Handle pause/resume
  const handlePauseResume = async (run: RunSummary) => {
    try {
      if (run.status === 'paused') {
        await invoke('resume_run', { runName: run.name });
      } else {
        await invoke('pause_run', { runName: run.name });
      }
      runs.invalidateRuns();
    } catch (e) {
      console.error('Failed to pause/resume run:', e);
      window.toast?.error(`Failed to ${run.status === 'paused' ? 'resume' : 'pause'} run`);
    }
  };

  return (
    <div
      class="absolute bottom-4 right-4"
      style={{ 'z-index': 50, 'pointer-events': 'auto' }}
    >
      {/* Collapsed button */}
      <Show when={!expanded()}>
        <button
          onClick={() => setExpanded(true)}
          class="relative flex items-center gap-2 px-3 py-2 rounded-lg transition-all hover:scale-105"
          classList={{
            'animate-pulse': hasWorkingRuns(),
          }}
          style={{
            background: activeCount() > 0
              ? 'linear-gradient(180deg, rgba(245, 158, 11, 0.2) 0%, rgba(30, 27, 24, 0.95) 100%)'
              : totalCount() > 0
                ? 'linear-gradient(180deg, rgba(125, 153, 112, 0.15) 0%, rgba(30, 27, 24, 0.95) 100%)'
                : 'rgba(39, 39, 42, 0.9)',
            border: activeCount() > 0
              ? '1px solid rgba(245, 158, 11, 0.4)'
              : totalCount() > 0
                ? '1px solid rgba(125, 153, 112, 0.3)'
                : '1px solid rgba(63, 63, 70, 0.5)',
            'box-shadow': hasWorkingRuns()
              ? '0 4px 16px rgba(0, 0, 0, 0.4), 0 0 16px rgba(245, 158, 11, 0.2)'
              : '0 4px 12px rgba(0, 0, 0, 0.3)',
          }}
          title={`${totalCount()} runs (${activeCount()} active) - R to toggle`}
        >
          <svg
            class="w-4 h-4"
            classList={{
              'text-amber-400': activeCount() > 0,
              'text-sage': activeCount() === 0 && totalCount() > 0,
              'text-wool-500': totalCount() === 0,
            }}
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M13 10V3L4 14h7v7l9-11h-7z"
            />
          </svg>
          <span
            class="text-sm font-semibold tabular-nums"
            classList={{
              'text-amber-300': activeCount() > 0,
              'text-sage/80': activeCount() === 0 && totalCount() > 0,
              'text-wool-500': totalCount() === 0,
            }}
          >
            {totalCount()}
          </span>
        </button>
      </Show>

      {/* Expanded panel */}
      <Show when={expanded()}>
        <div
          class="w-72 rounded-xl overflow-hidden"
          style={{
            background: 'linear-gradient(180deg, rgba(30, 27, 24, 0.98) 0%, rgba(26, 24, 21, 0.98) 100%)',
            border: '1px solid rgba(63, 63, 70, 0.6)',
            'box-shadow': '0 8px 32px rgba(0, 0, 0, 0.5), 0 0 0 1px rgba(0, 0, 0, 0.2)',
            'backdrop-filter': 'blur(12px)',
          }}
        >
          {/* Header */}
          <div
            class="flex items-center justify-between px-3 py-2.5"
            style={{
              background: 'linear-gradient(180deg, rgba(255, 255, 255, 0.03) 0%, transparent 100%)',
              'border-bottom': '1px solid rgba(63, 63, 70, 0.4)',
            }}
          >
            <div class="flex items-center gap-2">
              <svg class="w-4 h-4 text-amber-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  d="M13 10V3L4 14h7v7l9-11h-7z"
                />
              </svg>
              <span class="text-sm font-semibold text-wool-200">Runs</span>
              <Show when={activeCount() > 0}>
                <span class="text-xs text-amber-400 font-medium">
                  {activeCount()} active
                </span>
              </Show>
            </div>
            <button
              onClick={() => setExpanded(false)}
              class="p-1 rounded-md text-wool-500 hover:text-wool-200 hover:bg-white/5 transition-colors"
              title="Close (Esc)"
            >
              <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
              </svg>
            </button>
          </div>

          {/* Run list */}
          <div class="max-h-80 overflow-y-auto">
            <Show
              when={projectRuns().length > 0}
              fallback={
                <div class="px-4 py-6 text-center">
                  <svg class="w-8 h-8 mx-auto mb-2 text-wool-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      stroke-width="1.5"
                      d="M20 7l-8-4-8 4m16 0l-8 4m8-4v10l-8 4m0-10L4 7m8 4v10M4 7v10l8 4"
                    />
                  </svg>
                  <p class="text-sm text-wool-500">No runs yet</p>
                  <p class="text-xs text-wool-600 mt-1">Dispatch from the board to start</p>
                </div>
              }
            >
              <For each={projectRuns()}>
                {(run) => {
                  const isActive = () => isActiveRun(run.status);
                  return (
                    <div
                      class="group px-3 py-3 transition-all cursor-pointer"
                      classList={{
                        'hover:bg-amber-500/5': isActive(),
                        'hover:bg-white/[0.02]': !isActive(),
                      }}
                      style={{
                        'border-bottom': '1px solid rgba(63, 63, 70, 0.25)',
                        background: isActive() && (run.status === 'working' || run.status === 'eval')
                          ? 'linear-gradient(90deg, rgba(245, 158, 11, 0.05) 0%, transparent 100%)'
                          : undefined,
                      }}
                      onClick={() => handleViewRun(run.name)}
                    >
                      {/* Header row */}
                      <div class="flex items-center justify-between gap-3 mb-2">
                        <div class="flex items-center gap-2.5 min-w-0">
                          <span
                            class="w-2.5 h-2.5 rounded-full shrink-0"
                            classList={{
                              'bg-amber-500 shadow-[0_0_8px_rgba(245,158,11,0.5)]': run.status === 'working' || run.status === 'eval',
                              'bg-sage': run.status === 'done' || run.status === 'delivered' || run.status === 'merged',
                              'bg-terra': run.status === 'failed',
                              'bg-wool-500': run.status === 'paused' || run.status === 'idle',
                              'bg-golden': run.status === 'waiting',
                              'animate-pulse': run.status === 'working' || run.status === 'eval',
                            }}
                          />
                          <span
                            class="text-sm font-medium truncate"
                            classList={{
                              'text-wool-100': isActive(),
                              'text-wool-400': !isActive(),
                            }}
                          >
                            {run.name}
                          </span>
                        </div>
                        <span
                          class="text-[10px] uppercase font-semibold tracking-wide px-1.5 py-0.5 rounded"
                          classList={{
                            'text-amber-400 bg-amber-500/10': run.status === 'working' || run.status === 'eval',
                            'text-sage bg-sage/10': run.status === 'done' || run.status === 'delivered' || run.status === 'merged',
                            'text-terra bg-terra/10': run.status === 'failed',
                            'text-wool-400 bg-wool-700/50': run.status === 'paused' || run.status === 'idle',
                            'text-golden bg-golden/10': run.status === 'waiting',
                          }}
                        >
                          {run.status}
                        </span>
                      </div>

                      {/* Stats row */}
                      <div class="flex items-center gap-3 text-[11px] text-wool-500">
                        <div class="flex items-center gap-1">
                          <svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z" />
                          </svg>
                          <span class="tabular-nums">{run.workersActive}/{run.workersTotal}</span>
                        </div>
                        <div class="flex items-center gap-1">
                          <svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-6 9l2 2 4-4" />
                          </svg>
                          <span class="tabular-nums">{run.tasksDone}/{run.tasksTotal}</span>
                        </div>
                        <div class="flex items-center gap-1">
                          <svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
                          </svg>
                          <span class="tabular-nums">{formatElapsed(run.elapsedMinutes)}</span>
                        </div>

                        {/* Spacer */}
                        <div class="flex-1" />

                        {/* Actions */}
                        <Show when={run.status === 'paused'}>
                          <button
                            onClick={(e) => { e.stopPropagation(); handlePauseResume(run); }}
                            class="flex items-center gap-1 px-2 py-0.5 text-[10px] font-medium rounded bg-amber-500/10 text-amber-400 hover:bg-amber-500/20 transition-colors"
                            title="Resume run"
                          >
                            <svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M14.752 11.168l-3.197-2.132A1 1 0 0010 9.87v4.263a1 1 0 001.555.832l3.197-2.132a1 1 0 000-1.664z" />
                            </svg>
                            Resume
                          </button>
                        </Show>
                        <Show when={isActive() && run.status !== 'paused'}>
                          <button
                            onClick={(e) => { e.stopPropagation(); handlePauseResume(run); }}
                            class="p-1 rounded text-wool-500 hover:text-wool-300 hover:bg-white/5 transition-colors opacity-0 group-hover:opacity-100"
                            title="Pause run"
                          >
                            <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 9v6m4-6v6" />
                            </svg>
                          </button>
                        </Show>
                        <svg
                          class="w-4 h-4 text-wool-600 group-hover:text-wool-400 transition-colors"
                          fill="none"
                          stroke="currentColor"
                          viewBox="0 0 24 24"
                        >
                          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
                        </svg>
                      </div>
                    </div>
                  );
                }}
              </For>
            </Show>
          </div>

          {/* Footer hint */}
          <div
            class="px-3 py-2 text-[10px] text-wool-600 text-center"
            style={{ 'border-top': '1px solid rgba(63, 63, 70, 0.3)' }}
          >
            Press <kbd class="px-1 py-0.5 rounded bg-white/5 text-wool-500 font-mono">R</kbd> to toggle
          </div>
        </div>
      </Show>
    </div>
  );
};

export default RunsOverlay;
