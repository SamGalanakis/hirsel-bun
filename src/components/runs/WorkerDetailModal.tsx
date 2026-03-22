/**
 * WorkerDetailModal - Full worker detail popup
 *
 * Uses Basecoat dialog patterns with the Hirsel design language.
 * Compact, information-dense layout respecting the Alchemical Ledger aesthetic.
 */
import { type Component, Show } from 'solid-js';
import { useElapsedTime } from '../../hooks';
import type { WorkerDisplay } from '../../lib/types';
import { getContextBarClass, getContextClass, getContextStatus } from '../../lib/utils/context-class';
import { formatTokens } from '../../lib/utils/formatters';
import { getWorkerStatusConfig } from '../../lib/utils/status';
import { BaseModal, Icon, StatusDot, WorkerAvatar } from '../shared';

interface WorkerDetailModalProps {
  worker: WorkerDisplay;
  metricsAvailable: boolean;
  runName: string;
  onClose: () => void;
  onAttach: () => void;
  onOpenDM?: () => void;
}

export const WorkerDetailModal: Component<WorkerDetailModalProps> = (props) => {
  const elapsedTime = useElapsedTime(() => props.worker.sessionStartedAt);

  return (
    <BaseModal
      onClose={props.onClose}
      overlayClass="p-4 bg-black/70 backdrop-blur-sm"
      class="bg-pasture-800 border border-pasture-600 shadow-2xl w-full max-w-sm flex flex-col animate-in fade-in zoom-in-95 duration-200"
      style={{ 'max-height': 'min(90vh, 600px)' }}
    >
      {/* Header with worker identity */}
        <header class="relative px-5 pt-5 pb-4">
          {/* Close button */}
          <button
            class="absolute top-3 right-3 p-1.5 rounded-none text-wool-500 hover:text-wool-200 hover:bg-pasture-700 transition-colors"
            onClick={props.onClose}
            aria-label="Close"
          >
            <Icon name="x" class="w-4 h-4" />
          </button>

          {/* Avatar and identity */}
          <div class="flex items-center gap-4">
            <WorkerAvatar name={props.worker.name} size={56} />
            <div class="min-w-0 flex-1">
              <h2 class="text-lg font-semibold text-wool-100 truncate">{props.worker.name}</h2>
              <div class="flex items-center gap-2 mt-1">
                <StatusDot status={props.worker.status} />
                <span class={`text-sm font-medium ${getWorkerStatusConfig(props.worker.status).color}`}>
                  {getWorkerStatusConfig(props.worker.status).label}
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
            <div class="bg-pasture-700/50 rounded-none p-3 border-l-2 border-amber-500/60">
              <p class="text-xs text-wool-500 uppercase tracking-wide mb-1">Current Task</p>
              <p class="text-sm text-wool-100 font-medium">{props.worker.currentTask}</p>
            </div>
          </Show>

          {/* HITL Waiting alert */}
          <Show when={props.worker.hitlWaiting}>
            <div class="flex items-center gap-3 bg-golden/10 border border-golden/20 rounded-none p-3">
              <Icon name="user" class="w-4 h-4 text-golden flex-shrink-0" />
              <span class="text-sm text-golden font-medium">Awaiting human input</span>
            </div>
          </Show>

          {/* Waiting Thread */}
          <Show when={props.worker.waitingThread && !props.worker.hitlWaiting}>
            <div class="flex items-center gap-3 bg-sky-500/10 border border-sky-500/20 rounded-none p-3">
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
                  <div class="h-1.5 bg-pasture-600 rounded-none overflow-hidden">
                    <div
                      class={`h-full rounded-none transition-all duration-300 ${getContextBarClass(props.worker.contextUtilization)}`}
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
              <p class="text-xs font-mono text-wool-400 break-all bg-pasture-900/50 rounded-none px-2 py-1.5">
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
          <Show when={props.onOpenDM}>
            <button class="btn-ghost btn-sm" onClick={props.onOpenDM}>
              <Icon name="message-circle" class="w-4 h-4" />
              Message
            </button>
          </Show>
          <button class="btn btn-sm" onClick={props.onAttach}>
            <Icon name="eye" class="w-4 h-4" />
            Spectate
          </button>
        </footer>
    </BaseModal>
  );
};
