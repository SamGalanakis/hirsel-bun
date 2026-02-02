/**
 * WorkerCard - Displays a worker with sheep avatar and metrics
 */
import { type Component, Show, createEffect, createSignal, onCleanup } from 'solid-js';
import { generateSheepSvg } from '../../lib/sheep-avatar';
import type { WorkerDisplay } from '../../lib/types';
import { getContextClass } from '../../lib/utils/context-class';
import { formatElapsedTime, formatTokens } from '../../lib/utils/formatters';
import { Icon } from '../shared';

interface WorkerCardProps {
  worker: WorkerDisplay;
  metricsAvailable: boolean;
  selected?: boolean;
  compact?: boolean;
  onClick: () => void;
  onDoubleClick: () => void;
}

const STATUS_COLORS: Record<string, string> = {
  idle: 'border-wool-600',
  working: 'border-amber-500',
  waiting: 'border-golden',
  awaiting: 'border-sky-500',
  paused: 'border-golden',
  error: 'border-terra',
};

const STATUS_DOT_COLORS: Record<string, string> = {
  idle: 'bg-wool-500',
  working: 'bg-amber-500',
  waiting: 'bg-golden',
  awaiting: 'bg-sky-500',
  paused: 'bg-golden',
  error: 'bg-terra',
};

export const WorkerCard: Component<WorkerCardProps> = (props) => {
  const [elapsedTime, setElapsedTime] = createSignal('');

  // Update elapsed time every second
  createEffect(() => {
    const sessionStart = props.worker.sessionStartedAt;
    if (!sessionStart) {
      setElapsedTime('');
      return;
    }

    // Initial update
    setElapsedTime(formatElapsedTime(sessionStart));

    // Update every second
    const interval = setInterval(() => {
      setElapsedTime(formatElapsedTime(sessionStart));
    }, 1000);

    onCleanup(() => clearInterval(interval));
  });

  const sheepSvg = () => generateSheepSvg(props.worker.sheepConfig, props.compact ? 28 : 40, props.worker.status);

  // Compact mode for horizontal strip
  if (props.compact) {
    return (
      <button
        class={`relative flex items-center gap-2 px-3 py-2 rounded-lg border bg-pasture-800 hover:bg-pasture-700 transition-all text-left flex-shrink-0 ${
          STATUS_COLORS[props.worker.status] || 'border-pasture-600'
        } ${props.selected ? 'ring-2 ring-amber-500' : ''}`}
        onClick={props.onClick}
        onDblClick={props.onDoubleClick}
      >
        {/* Sheep avatar */}
        <div class="relative flex-shrink-0">
          <div innerHTML={sheepSvg()} />
          <Show when={props.worker.isLeader}>
            <div
              class="absolute -top-0.5 -right-0.5 w-3 h-3 rounded-full bg-golden flex items-center justify-center"
              title="Leader"
            >
              <Icon name="star" class="w-2 h-2 text-pasture-900" />
            </div>
          </Show>
        </div>

        <div class="min-w-0">
          <p class="text-xs font-medium text-wool-200 truncate max-w-[100px]">{props.worker.name}</p>
          <div class="flex items-center gap-1.5 text-[10px] text-wool-500">
            <Show when={elapsedTime()}>
              <span>{elapsedTime()}</span>
            </Show>
            <Show when={props.metricsAvailable && props.worker.contextUtilization != null}>
              <span class={getContextClass(props.worker.contextUtilization)}>
                {Math.round(props.worker.contextUtilization || 0)}%
              </span>
            </Show>
          </div>
          <Show when={props.worker.currentTask}>
            <p class="text-[10px] text-wool-400 truncate max-w-[100px]" title={props.worker.currentTask || ''}>
              {props.worker.currentTask}
            </p>
          </Show>
        </div>

        {/* Status dot */}
        <div
          class={`absolute top-1.5 right-1.5 w-1.5 h-1.5 rounded-full ${
            STATUS_DOT_COLORS[props.worker.status] || 'bg-wool-500'
          }`}
          title={props.worker.status}
        />
      </button>
    );
  }

  // Full mode for modals/detail views
  return (
    <button
      class={`relative p-3 rounded-lg border-2 bg-pasture-800 hover:bg-pasture-700 transition-all text-left ${
        STATUS_COLORS[props.worker.status] || 'border-pasture-600'
      } ${props.selected ? 'ring-2 ring-amber-500 ring-offset-2 ring-offset-pasture-900' : ''}`}
      onClick={props.onClick}
      onDblClick={props.onDoubleClick}
    >
      {/* Status dot */}
      <div
        class={`absolute top-2 right-2 w-2 h-2 rounded-full ${
          STATUS_DOT_COLORS[props.worker.status] || 'bg-wool-500'
        }`}
        title={props.worker.status}
      />

      <div class="flex items-start gap-3">
        {/* Sheep avatar */}
        <div class="relative flex-shrink-0">
          <div innerHTML={sheepSvg()} />
          {/* Leader badge */}
          <Show when={props.worker.isLeader}>
            <div
              class="absolute -top-1 -right-1 w-4 h-4 rounded-full bg-golden flex items-center justify-center"
              title="Leader"
            >
              <Icon name="star" class="w-2.5 h-2.5 text-pasture-900" />
            </div>
          </Show>
        </div>

        <div class="flex-1 min-w-0">
          {/* Worker name and status */}
          <div class="flex items-center gap-2">
            <p class="text-sm font-medium text-wool-200 truncate">{props.worker.name}</p>
          </div>

          {/* Session elapsed time */}
          <Show when={elapsedTime()}>
            <p class="text-xs text-wool-500 mt-0.5">{elapsedTime()}</p>
          </Show>

          {/* Token counts */}
          <Show when={props.metricsAvailable && (props.worker.inputTokens || props.worker.outputTokens)}>
            <div class="flex items-center gap-2 mt-1 text-xs text-wool-500">
              <span title="Input tokens">
                <Icon name="arrow-down" class="w-3 h-3 inline" />
                {formatTokens(props.worker.inputTokens)}
              </span>
              <span title="Output tokens">
                <Icon name="arrow-up" class="w-3 h-3 inline" />
                {formatTokens(props.worker.outputTokens)}
              </span>
            </div>
          </Show>

          {/* Context utilization */}
          <Show when={props.metricsAvailable && props.worker.contextUtilization != null}>
            <div class="mt-1.5">
              <div class="flex items-center justify-between text-xs mb-0.5">
                <span class="text-wool-600">Context</span>
                <span class={getContextClass(props.worker.contextUtilization)}>
                  {Math.round(props.worker.contextUtilization || 0)}%
                </span>
              </div>
              <div class="h-1 bg-pasture-600 rounded-full overflow-hidden">
                <div
                  class={`h-full rounded-full transition-all ${
                    (props.worker.contextUtilization || 0) >= 90
                      ? 'bg-terra'
                      : (props.worker.contextUtilization || 0) >= 75
                        ? 'bg-amber-500'
                        : 'bg-sage'
                  }`}
                  style={{ width: `${props.worker.contextUtilization || 0}%` }}
                />
              </div>
            </div>
          </Show>

          {/* Current task */}
          <Show when={props.worker.currentTask}>
            <p class="text-xs text-wool-400 truncate mt-1.5" title={props.worker.currentTask || ''}>
              {props.worker.currentTask}
            </p>
          </Show>

          {/* HITL waiting indicator */}
          <Show when={props.worker.hitlWaiting}>
            <div class="flex items-center gap-1 mt-1.5 text-xs text-golden">
              <Icon name="user" class="w-3 h-3" />
              <span>Awaiting input</span>
            </div>
          </Show>
        </div>
      </div>
    </button>
  );
};
