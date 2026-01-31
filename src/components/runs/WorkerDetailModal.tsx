/**
 * WorkerDetailModal - Full worker detail popup
 *
 * Uses Basecoat dialog patterns with scrollable content for small screens.
 */
import { type Component, Show, createEffect, createSignal, onCleanup } from 'solid-js';
import { generateSheepSvg } from '../../lib/sheep-avatar';
import type { WorkerDisplay } from '../../lib/types';
import { getContextBarClass, getContextClass, getContextStatus } from '../../lib/utils/context-class';
import { formatElapsedTime, formatTokens } from '../../lib/utils/formatters';

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

  // Close on escape
  createEffect(() => {
    const handleKeydown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        props.onClose();
      }
    };
    window.addEventListener('keydown', handleKeydown);
    onCleanup(() => window.removeEventListener('keydown', handleKeydown));
  });

  const sheepSvg = () => generateSheepSvg(props.worker.sheepConfig, 48, props.worker.status);

  return (
    <div
      class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/80"
      onClick={(e) => {
        if (e.target === e.currentTarget) props.onClose();
      }}
    >
      {/* Dialog container with max height for scrolling */}
      <div
        class="bg-pasture-800 border border-pasture-600 rounded-lg shadow-xl w-full max-w-md flex flex-col"
        style={{ 'max-height': 'min(90vh, 700px)' }}
      >
        {/* Header - fixed */}
        <header class="flex items-center justify-between p-4 border-b border-pasture-600 flex-shrink-0">
          <div class="flex items-center gap-3">
            <div class="flex-shrink-0" innerHTML={sheepSvg()} />
            <div class="min-w-0">
              <h2 class="text-lg font-medium text-wool-100 truncate">{props.worker.name}</h2>
              <p class={`text-sm ${STATUS_COLORS[props.worker.status]}`}>
                {STATUS_LABELS[props.worker.status] || props.worker.status}
              </p>
            </div>
          </div>
          <button
            class="btn-ghost btn-icon flex-shrink-0"
            onClick={props.onClose}
            aria-label="Close"
          >
            <svg class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M18 6L6 18M6 6l12 12" />
            </svg>
          </button>
        </header>

        {/* Content - scrollable */}
        <section class="flex-1 overflow-y-auto p-4 space-y-3">
          {/* Status & Session Time */}
          <div class="grid grid-cols-2 gap-3">
            <div class="card">
              <section class="p-3">
                <p class="text-xs text-muted-foreground mb-1">Status</p>
                <p class={`text-sm font-medium ${STATUS_COLORS[props.worker.status]}`}>
                  {STATUS_LABELS[props.worker.status] || props.worker.status}
                </p>
              </section>
            </div>
            <div class="card">
              <section class="p-3">
                <p class="text-xs text-muted-foreground mb-1">Session Time</p>
                <p class="text-sm font-medium text-wool-200">{elapsedTime() || 'N/A'}</p>
              </section>
            </div>
          </div>

          {/* Token Usage */}
          <Show when={props.metricsAvailable}>
            <div class="card">
              <section class="p-3">
                <p class="text-xs text-muted-foreground mb-2">Token Usage</p>
                <div class="flex items-center gap-4 text-sm">
                  <span class="text-wool-300 flex items-center gap-1">
                    <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <path d="M12 5v14M5 12l7 7 7-7" />
                    </svg>
                    {formatTokens(props.worker.inputTokens)} in
                  </span>
                  <span class="text-wool-300 flex items-center gap-1">
                    <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <path d="M12 19V5M5 12l7-7 7 7" />
                    </svg>
                    {formatTokens(props.worker.outputTokens)} out
                  </span>
                </div>

                {/* Context utilization bar */}
                <Show when={props.worker.contextUtilization != null}>
                  <div class="mt-3">
                    <div class="flex items-center justify-between text-xs mb-1">
                      <span class="text-muted-foreground">Context Utilization</span>
                      <span class={getContextClass(props.worker.contextUtilization)}>
                        {Math.round(props.worker.contextUtilization || 0)}% ({getContextStatus(props.worker.contextUtilization)})
                      </span>
                    </div>
                    <div class="h-1.5 bg-pasture-600 rounded-full overflow-hidden">
                      <div
                        class={`h-full rounded-full transition-all ${getContextBarClass(props.worker.contextUtilization)}`}
                        style={{ width: `${props.worker.contextUtilization || 0}%` }}
                      />
                    </div>
                  </div>
                </Show>
              </section>
            </div>
          </Show>

          {/* Current Task */}
          <Show when={props.worker.currentTask}>
            <div class="card">
              <section class="p-3">
                <p class="text-xs text-muted-foreground mb-1">Current Task</p>
                <p class="text-sm text-wool-200">{props.worker.currentTask}</p>
              </section>
            </div>
          </Show>

          {/* Waiting Thread */}
          <Show when={props.worker.waitingThread}>
            <div class="card border-golden/30 bg-golden/5">
              <section class="p-3">
                <p class="text-xs text-golden mb-1">Waiting on Thread</p>
                <p class="text-sm text-wool-200">{props.worker.waitingThread}</p>
              </section>
            </div>
          </Show>

          {/* HITL Waiting */}
          <Show when={props.worker.hitlWaiting}>
            <div class="alert border-golden/30 bg-golden/5">
              <svg class="w-4 h-4 text-golden" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
                <circle cx="12" cy="7" r="4" />
              </svg>
              <section>
                <span class="text-sm font-medium text-golden">Awaiting Human Input</span>
              </section>
            </div>
          </Show>

          {/* Work Directory */}
          <Show when={props.worker.workDir}>
            <div class="card">
              <section class="p-3">
                <p class="text-xs text-muted-foreground mb-1">Work Directory</p>
                <p class="text-xs font-mono text-wool-400 break-all">{props.worker.workDir}</p>
              </section>
            </div>
          </Show>

          {/* Technical Info */}
          <div class="card">
            <section class="p-3">
              <p class="text-xs text-muted-foreground mb-2">Technical Info</p>
              <div class="grid grid-cols-3 gap-3 text-sm">
                <div>
                  <p class="text-xs text-wool-600">PID</p>
                  <p class="text-wool-300 font-mono text-sm">{props.worker.pid || 'N/A'}</p>
                </div>
                <div>
                  <p class="text-xs text-wool-600">Location</p>
                  <p class="text-wool-300 text-sm">{props.worker.location}</p>
                </div>
                <div>
                  <p class="text-xs text-wool-600">Turns</p>
                  <p class="text-wool-300 text-sm">{props.worker.turns ?? 'N/A'}</p>
                </div>
              </div>
            </section>
          </div>
        </section>

        {/* Footer - fixed */}
        <footer class="p-4 border-t border-pasture-600 flex justify-end gap-2 flex-shrink-0">
          <button class="btn-ghost" onClick={props.onClose}>
            Close
          </button>
          <button class="btn" onClick={props.onAttach}>
            <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7Z" />
              <circle cx="12" cy="12" r="3" />
            </svg>
            Spectate
          </button>
        </footer>
      </div>
    </div>
  );
};
