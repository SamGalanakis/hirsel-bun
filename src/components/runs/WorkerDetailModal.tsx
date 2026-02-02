/**
 * WorkerDetailModal - Full worker detail popup
 *
 * Uses Basecoat dialog patterns with the Hirsel design language.
 * Compact, information-dense layout respecting the "Highland Craft" aesthetic.
 */
import { type Component, Show, createEffect, createSignal, onCleanup } from 'solid-js';
import { useEscapeKey } from '../../hooks';
import { generateSheepSvg } from '../../lib/sheep-avatar';
import type { WorkerDisplay } from '../../lib/types';
import { getContextBarClass, getContextClass, getContextStatus } from '../../lib/utils/context-class';
import { formatElapsedTime, formatTokens } from '../../lib/utils/formatters';
import { Icon } from '../shared';

interface WorkerDetailModalProps {
  worker: WorkerDisplay;
  metricsAvailable: boolean;
  runName: string;
  onClose: () => void;
  onAttach: () => void;
}

const STATUS_LABELS: Record<string, string> = {
  idle: 'Idle',
  working: 'Working',
  waiting: 'Waiting',
  awaiting: 'Awaiting',
  paused: 'Paused',
  error: 'Error',
};

const STATUS_COLORS: Record<string, string> = {
  idle: 'text-wool-400',
  working: 'text-amber-500',
  waiting: 'text-golden',
  awaiting: 'text-sky-500',
  paused: 'text-golden',
  error: 'text-terra',
};

const STATUS_DOT_CLASSES: Record<string, string> = {
  idle: 'bg-wool-500',
  working: 'bg-amber-500 animate-pulse',
  waiting: 'bg-golden',
  awaiting: 'bg-sky-500',
  paused: 'bg-golden',
  error: 'bg-terra animate-pulse',
};

export const WorkerDetailModal: Component<WorkerDetailModalProps> = (props) => {
  const [elapsedTime, setElapsedTime] = createSignal('');

  // Update elapsed time every second
  createEffect(() => {
    const sessionStart = props.worker.sessionStartedAt;
    if (!sessionStart) {
      setElapsedTime('');
      return;
    }

    setElapsedTime(formatElapsedTime(sessionStart));

    const interval = setInterval(() => {
      setElapsedTime(formatElapsedTime(sessionStart));
    }, 1000);

    onCleanup(() => clearInterval(interval));
  });

  useEscapeKey(() => props.onClose());

  const sheepSvg = () => generateSheepSvg(props.worker.sheepConfig, 56, props.worker.status);

  return (
    <div
      class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/70 backdrop-blur-sm"
      onClick={(e) => {
        if (e.target === e.currentTarget) props.onClose();
      }}
    >
      <div
        class="bg-pasture-800 border border-pasture-600 rounded-lg shadow-2xl w-full max-w-sm flex flex-col animate-in fade-in zoom-in-95 duration-200"
        style={{ 'max-height': 'min(90vh, 600px)' }}
      >
        {/* Header with sheep avatar and name */}
        <header class="relative px-5 pt-5 pb-4">
          {/* Close button */}
          <button
            class="absolute top-3 right-3 p-1.5 rounded-md text-wool-500 hover:text-wool-200 hover:bg-pasture-700 transition-colors"
            onClick={props.onClose}
            aria-label="Close"
          >
            <Icon name="x" class="w-4 h-4" />
          </button>

          {/* Avatar and identity */}
          <div class="flex items-center gap-4">
            <div
              class="flex-shrink-0 p-2 bg-pasture-700/50 rounded-lg border border-pasture-600/50"
              innerHTML={sheepSvg()}
            />
            <div class="min-w-0 flex-1">
              <h2 class="text-lg font-semibold text-wool-100 truncate">{props.worker.name}</h2>
              <div class="flex items-center gap-2 mt-1">
                <span class={`w-2 h-2 rounded-full ${STATUS_DOT_CLASSES[props.worker.status]}`} />
                <span class={`text-sm font-medium ${STATUS_COLORS[props.worker.status]}`}>
                  {STATUS_LABELS[props.worker.status] || props.worker.status}
                </span>
                <Show when={elapsedTime()}>
                  <span class="text-wool-500 text-sm">· {elapsedTime()}</span>
                </Show>
              </div>
            </div>
          </div>
        </header>

        {/* Content */}
        <section class="flex-1 overflow-y-auto px-5 pb-5 space-y-4">
          {/* Current Task - highlighted if present */}
          <Show when={props.worker.currentTask}>
            <div class="bg-pasture-700/50 rounded-md p-3 border-l-2 border-amber-500/60">
              <p class="text-xs text-wool-500 uppercase tracking-wide mb-1">Current Task</p>
              <p class="text-sm text-wool-100 font-medium">{props.worker.currentTask}</p>
            </div>
          </Show>

          {/* HITL Waiting alert */}
          <Show when={props.worker.hitlWaiting}>
            <div class="flex items-center gap-3 bg-golden/10 border border-golden/20 rounded-md p-3">
              <Icon name="user" class="w-4 h-4 text-golden flex-shrink-0" />
              <span class="text-sm text-golden font-medium">Awaiting human input</span>
            </div>
          </Show>

          {/* Waiting Thread */}
          <Show when={props.worker.waitingThread && !props.worker.hitlWaiting}>
            <div class="flex items-center gap-3 bg-sky-500/10 border border-sky-500/20 rounded-md p-3">
              <Icon name="message-circle" class="w-4 h-4 text-sky-400 flex-shrink-0" />
              <div class="min-w-0">
                <p class="text-xs text-sky-400">Waiting on</p>
                <p class="text-sm text-wool-200 truncate">{props.worker.waitingThread}</p>
              </div>
            </div>
          </Show>

          {/* Metrics Section */}
          <Show when={props.metricsAvailable}>
            <div class="space-y-3">
              {/* Token counts */}
              <div class="flex items-center gap-6">
                <div class="flex items-center gap-2">
                  <Icon name="arrow-down" class="w-3.5 h-3.5 text-wool-500" />
                  <span class="text-sm text-wool-300">
                    <span class="font-mono">{formatTokens(props.worker.inputTokens)}</span>
                    <span class="text-wool-500 ml-1">in</span>
                  </span>
                </div>
                <div class="flex items-center gap-2">
                  <Icon name="arrow-up" class="w-3.5 h-3.5 text-wool-500" />
                  <span class="text-sm text-wool-300">
                    <span class="font-mono">{formatTokens(props.worker.outputTokens)}</span>
                    <span class="text-wool-500 ml-1">out</span>
                  </span>
                </div>
              </div>

              {/* Context utilization bar */}
              <Show when={props.worker.contextUtilization != null}>
                <div>
                  <div class="flex items-center justify-between text-xs mb-1.5">
                    <span class="text-wool-500">Context</span>
                    <span class={getContextClass(props.worker.contextUtilization)}>
                      {Math.round(props.worker.contextUtilization || 0)}%
                      <span class="text-wool-600 ml-1">({getContextStatus(props.worker.contextUtilization)})</span>
                    </span>
                  </div>
                  <div class="h-1.5 bg-pasture-600 rounded-full overflow-hidden">
                    <div
                      class={`h-full rounded-full transition-all duration-300 ${getContextBarClass(props.worker.contextUtilization)}`}
                      style={{ width: `${props.worker.contextUtilization || 0}%` }}
                    />
                  </div>
                </div>
              </Show>
            </div>
          </Show>

          {/* Info grid */}
          <div class="border-t border-pasture-700 pt-4">
            <dl class="grid grid-cols-2 gap-x-4 gap-y-3 text-sm">
              <Show when={props.worker.location}>
                <div>
                  <dt class="text-xs text-wool-600 uppercase tracking-wide">Location</dt>
                  <dd class="text-wool-300 mt-0.5">{props.worker.location}</dd>
                </div>
              </Show>
              <Show when={props.worker.turns != null}>
                <div>
                  <dt class="text-xs text-wool-600 uppercase tracking-wide">Turns</dt>
                  <dd class="text-wool-300 mt-0.5 font-mono">{props.worker.turns}</dd>
                </div>
              </Show>
              <Show when={props.worker.pid}>
                <div>
                  <dt class="text-xs text-wool-600 uppercase tracking-wide">PID</dt>
                  <dd class="text-wool-300 mt-0.5 font-mono">{props.worker.pid}</dd>
                </div>
              </Show>
            </dl>
          </div>

          {/* Work Directory */}
          <Show when={props.worker.workDir}>
            <div class="border-t border-pasture-700 pt-4">
              <p class="text-xs text-wool-600 uppercase tracking-wide mb-1.5">Work Directory</p>
              <p class="text-xs font-mono text-wool-400 break-all bg-pasture-900/50 rounded px-2 py-1.5">
                {props.worker.workDir}
              </p>
            </div>
          </Show>
        </section>

        {/* Footer */}
        <footer class="px-5 py-4 border-t border-pasture-700 flex justify-end gap-3">
          <button class="btn-ghost btn-sm" onClick={props.onClose}>
            Close
          </button>
          <button class="btn btn-sm" onClick={props.onAttach}>
            <Icon name="eye" class="w-4 h-4" />
            Spectate
          </button>
        </footer>
      </div>
    </div>
  );
};
