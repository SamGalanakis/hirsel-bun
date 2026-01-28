import { createSignal, Show } from 'solid-js';
import { invoke } from '@tauri-apps/api/core';
import { useProject } from '../../stores/project-context';
import type { RunDetail } from '../../lib/types';

interface DispatchModalProps {
  rootTaskIds: string[];
  taskCount: number;
  evalCount: number;
  onClose: () => void;
  onDispatch: (runName: string) => void;
}

export function DispatchModal(props: DispatchModalProps) {
  const { selectedProject } = useProject();
  const [runName, setRunName] = createSignal('');
  const [workerCount, setWorkerCount] = createSignal(3);
  const [useTimeLimit, setUseTimeLimit] = createSignal(false);
  const [timeLimit, setTimeLimit] = createSignal(60);
  const [isLoading, setIsLoading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    setIsLoading(true);
    setError(null);

    const p = selectedProject();
    if (!p) {
      setError('No project selected');
      setIsLoading(false);
      return;
    }

    try {
      const result = await invoke<RunDetail>('dispatch_board_run', {
        projectId: p.id,
        rootTaskIds: props.rootTaskIds,
        runName: runName() || null,
        workerScale: workerCount().toString(),
        timeLimitMinutes: useTimeLimit() ? timeLimit() : null,
      });

      props.onDispatch(result.name);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setIsLoading(false);
    }
  };

  return (
    <div class="fixed inset-0 z-50 flex items-center justify-center modal-backdrop">
      {/* Backdrop */}
      <div
        class="absolute inset-0 bg-pasture-900/80"
        style={{ 'backdrop-filter': 'blur(4px)' }}
        onClick={props.onClose}
      />

      {/* Modal */}
      <div
        class="relative z-10 w-full max-w-md mx-4 bg-pasture-800 rounded-lg border border-pasture-600 modal-content"
        style={{ 'box-shadow': '0 24px 64px rgba(0,0,0,0.5)' }}
      >
        {/* Header */}
        <div class="flex items-center justify-between px-5 py-4 border-b border-pasture-600">
          <h2 class="text-lg font-semibold text-wool-100">Dispatch Run</h2>
          <button
            onClick={props.onClose}
            class="p-1.5 rounded-md hover:bg-pasture-700 text-wool-500 hover:text-wool-300 transition-colors"
          >
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        {/* Content */}
        <form onSubmit={handleSubmit} class="form p-5 space-y-5">
          {/* Scope summary */}
          <div class="flex items-center gap-4 px-4 py-3 bg-pasture-700 rounded-lg text-sm">
            <div class="flex items-center gap-2">
              <span class="text-wool-500">Tasks</span>
              <span class="text-amber-500 font-semibold tabular-nums">{props.taskCount}</span>
            </div>
            <Show when={props.evalCount > 0}>
              <span class="text-wool-700">·</span>
              <div class="flex items-center gap-2">
                <span class="text-wool-500">Evals</span>
                <span class="text-sage font-semibold tabular-nums">{props.evalCount}</span>
              </div>
            </Show>
            <span class="text-wool-700">·</span>
            <div class="flex items-center gap-2">
              <span class="text-wool-500">Roots</span>
              <span class="text-wool-300 tabular-nums">{props.rootTaskIds.length}</span>
            </div>
          </div>

          {/* Run name */}
          <div class="space-y-1.5">
            <label class="text-sm text-wool-300">Run Name</label>
            <input
              type="text"
              value={runName()}
              onInput={(e) => setRunName(e.currentTarget.value)}
              placeholder="Auto-generated if empty"
              class="w-full px-3 py-2 bg-pasture-700 border border-pasture-600 rounded-md text-wool-100 placeholder:text-wool-700 focus:outline-none focus:border-amber-500/50 focus:ring-1 focus:ring-amber-500/25 transition-colors text-sm"
            />
          </div>

          {/* Workers */}
          <div class="space-y-1.5">
            <label class="text-sm text-wool-300">Workers</label>
            <input
              type="number"
              min="1"
              max="10"
              value={workerCount()}
              onInput={(e) => setWorkerCount(Math.max(1, Math.min(10, parseInt(e.currentTarget.value) || 1)))}
              class="w-24 px-3 py-2 bg-pasture-700 border border-pasture-600 rounded-md text-wool-100 focus:outline-none focus:border-amber-500/50 focus:ring-1 focus:ring-amber-500/25 transition-colors text-sm tabular-nums"
            />
          </div>

          {/* Time limit toggle + input */}
          <div class="space-y-2">
            <label class="flex items-center gap-3 cursor-pointer">
              <input
                type="checkbox"
                role="switch"
                checked={useTimeLimit()}
                onChange={() => setUseTimeLimit(!useTimeLimit())}
              />
              <span class="text-sm text-wool-300">Time limit</span>
            </label>
            <Show when={useTimeLimit()}>
              <div class="flex items-center gap-2 pl-12">
                <input
                  type="number"
                  min="5"
                  max="480"
                  value={timeLimit()}
                  onInput={(e) => setTimeLimit(parseInt(e.currentTarget.value) || 60)}
                  class="w-20 px-3 py-2 bg-pasture-700 border border-pasture-600 rounded-md text-wool-100 focus:outline-none focus:border-amber-500/50 focus:ring-1 focus:ring-amber-500/25 transition-colors text-sm tabular-nums"
                />
                <span class="text-sm text-wool-500">minutes</span>
              </div>
            </Show>
          </div>

          {/* Error */}
          <Show when={error()}>
            <div class="px-4 py-3 bg-terra/10 border border-terra/30 rounded-md text-terra text-sm">
              {error()}
            </div>
          </Show>

          {/* Actions */}
          <div class="flex items-center justify-end gap-3 pt-2">
            <button
              type="button"
              onClick={props.onClose}
              class="px-4 py-2 text-sm text-wool-500 hover:text-wool-100 transition-colors"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={isLoading()}
              class="btn-warning px-5 py-2 text-sm font-medium rounded-md transition-all disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2"
            >
              <Show when={isLoading()}>
                <svg class="w-4 h-4 animate-spin" fill="none" viewBox="0 0 24 24">
                  <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" />
                  <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
                </svg>
              </Show>
              <span>Dispatch</span>
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
