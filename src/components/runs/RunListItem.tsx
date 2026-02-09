/**
 * Individual run list item component
 */
import { type Component, Show } from 'solid-js';
import type { RunSummary } from '../../lib/types';
import {
  formatElapsed,
  formatProgress,
} from '../../lib/utils/formatters';
import {
  getProgressBarClass,
  getStatusBadgeClass,
  getStatusLabel,
} from '../../lib/utils/status';

interface RunListItemProps {
  run: RunSummary;
  selected: boolean;
  onSelect: () => void;
  onContextMenu: (e: MouseEvent) => void;
  getTimeDisplay: (run: RunSummary) => string;
  formatTimeRemaining: (limit: number | null, elapsed: number | null) => string;
}

export const RunListItem: Component<RunListItemProps> = (props) => {
  const getStatusDotClass = (status: string) => {
    const classes: Record<string, string> = {
      draft: 'status-draft',
      idle: 'status-idle',
      working: 'status-working',
      paused: 'status-waiting',
      runaway: 'status-error',
      timed_out: 'status-error',
      eval: 'status-working',
      eval_failed: 'status-error',
      waiting: 'status-waiting',
      done: 'status-done',
      delivered: 'status-done',
      merged: 'status-done',
    };
    return classes[status] || 'status-idle';
  };

  const progressPct = () => formatProgress(props.run.tasksDone, props.run.tasksTotal);

  return (
    <div
      onClick={props.onSelect}
      onContextMenu={props.onContextMenu}
      class="run-item card-interactive px-3 py-2.5 cursor-pointer hover:bg-pasture-700 border-b border-pasture-800"
      classList={{ 'bg-pasture-700': props.selected }}
      data-test={`run-item-${props.run.name}`}
    >
      {/* Header: name + status badge */}
      <div class="flex items-center justify-between gap-2">
        <div class="flex items-center gap-2 min-w-0">
          <span
            class="status-dot w-2 h-2 rounded-full flex-shrink-0"
            classList={{
              [getStatusDotClass(props.run.status)]: true,
            }}
          />
          <span class="text-sm text-wool-100 truncate">{props.run.name}</span>
          <Show when={props.run.hasUnreadMessages}>
            <span class="w-1.5 h-1.5 bg-amber-500 rounded-full flex-shrink-0" />
          </Show>
        </div>
        <span
          class="text-[10px] px-1.5 py-0.5 rounded font-medium flex-shrink-0"
          classList={{
            [getStatusBadgeClass(props.run.status)]: true,
          }}
        >
          {getStatusLabel(props.run.status)}
        </span>
      </div>

      {/* Stats row: tasks, workers, time (hide for drafts) */}
      <Show when={props.run.status !== 'draft'}>
        <div class="flex items-center justify-between mt-1.5 text-xs text-wool-500">
          <span class="flex items-center gap-2">
            <span>
              {props.run.tasksDone}/{props.run.tasksTotal}
            </span>
            <span class="text-wool-600">\u2022</span>
            <span
              classList={{ 'text-sage': props.run.workersActive > 0 }}
              title={`${props.run.workersActive} active \u00b7 ${props.run.workersTotal} spawned \u00b7 ${props.run.workersDesired} desired`}
            >
              {props.run.workersActive}/{props.run.workersDesired} workers
            </span>
          </span>
          <span>{props.getTimeDisplay(props.run)}</span>
        </div>
      </Show>

      {/* Draft: just show creation time */}
      <Show when={props.run.status === 'draft'}>
        <div class="mt-1.5 text-xs text-wool-500">
          <span>{props.getTimeDisplay(props.run)}</span>
        </div>
      </Show>

      {/* Progress bar (hide for drafts) */}
      <Show when={props.run.status !== 'draft'}>
        <div class="mt-2 flex items-center gap-2">
          <div class="flex-1 h-2 bg-pasture-600/50 rounded-full overflow-hidden relative">
            <div
              class="h-full rounded-full transition-all duration-500 ease-out"
              classList={{
                [getProgressBarClass(
                  props.run.status,
                  props.run.tasksDone,
                  props.run.tasksTotal,
                )]: true,
                'animate-pulse-subtle': props.run.status === 'working',
              }}
              style={{ width: `${progressPct()}%` }}
            />
            <Show when={progressPct() === 100}>
              <div class="absolute inset-0 bg-gradient-to-r from-transparent via-white/10 to-transparent" />
            </Show>
          </div>
          <span
            class="text-[10px] font-medium w-8 text-right tabular-nums"
            classList={{
              'text-sage': progressPct() === 100,
              'text-wool-500': progressPct() !== 100,
            }}
          >
            {progressPct()}%
          </span>
        </div>
      </Show>

      {/* Time limit/remaining */}
      <Show when={props.run.timeLimitMinutes}>
        <div class="mt-1 text-[10px] text-wool-600 flex items-center gap-1">
          <Show when={props.run.status === 'draft'}>
            <span>{formatElapsed(props.run.timeLimitMinutes!)} limit</span>
          </Show>
          <Show when={props.run.status !== 'draft'}>
            <span>
              {props.formatTimeRemaining(
                props.run.timeLimitMinutes,
                props.run.elapsedMinutes,
              )}
            </span>
          </Show>
        </div>
      </Show>
    </div>
  );
};
