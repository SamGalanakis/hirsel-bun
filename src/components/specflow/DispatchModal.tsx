import { createSignal, Show } from 'solid-js';
import { invoke } from '@tauri-apps/api/core';
import { useProject } from '../../stores/project-context';

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
  const [workerScale, setWorkerScale] = createSignal(1);
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
      const result = await invoke<{ runName: string }>('prepare_multi_dispatch', {
        projectId: p.id,
        rootTaskIds: props.rootTaskIds,
        runName: runName() || null,
        targetBranch: null, // TODO: Get from project settings
        workerScale: workerScale().toString(),
        timeLimitMinutes: timeLimit(),
      });

      // Record dispatch for each root task
      for (const taskId of props.rootTaskIds) {
        await invoke('record_dispatch', {
          projectId: p.id,
          taskId,
          runName: result.runName,
        });
      }

      props.onDispatch(result.runName);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setIsLoading(false);
    }
  };

  return (
    <div class="fixed inset-0 z-50 flex items-center justify-center">
      {/* Backdrop */}
      <div
        class="absolute inset-0 bg-black/60 backdrop-blur-sm"
        onClick={props.onClose}
      />

      {/* Modal */}
      <div class="relative z-10 w-full max-w-md mx-4 bg-wool-900 rounded-2xl border border-wool-700 shadow-2xl">
        {/* Header */}
        <div class="flex items-center justify-between px-6 py-4 border-b border-wool-700">
          <h2 class="text-lg font-semibold text-wool-50">Dispatch Run</h2>
          <button
            onClick={props.onClose}
            class="p-1.5 rounded-lg hover:bg-wool-800 text-wool-400 hover:text-wool-200 transition-colors"
          >
            <i data-lucide="x" class="w-5 h-5" />
          </button>
        </div>

        {/* Content */}
        <form onSubmit={handleSubmit} class="p-6 space-y-5">
          {/* Scope summary */}
          <div class="flex items-center gap-4 p-3 bg-wool-800/50 rounded-xl text-sm">
            <div class="flex items-center gap-2">
              <span class="text-wool-400">Tasks:</span>
              <span class="text-amber-400 font-semibold">{props.taskCount}</span>
            </div>
            <Show when={props.evalCount > 0}>
              <div class="flex items-center gap-2">
                <span class="text-wool-400">Evals:</span>
                <span class="text-sage font-semibold">{props.evalCount}</span>
              </div>
            </Show>
            <div class="flex items-center gap-2">
              <span class="text-wool-400">Roots:</span>
              <span class="text-wool-200">{props.rootTaskIds.length}</span>
            </div>
          </div>

          {/* Run name */}
          <div class="space-y-2">
            <label class="text-sm text-wool-300">Run Name (optional)</label>
            <input
              type="text"
              value={runName()}
              onInput={(e) => setRunName(e.currentTarget.value)}
              placeholder="Auto-generated if empty"
              class="w-full px-4 py-2.5 bg-wool-800 border border-wool-600 rounded-xl text-wool-100 placeholder:text-wool-500 focus:outline-none focus:border-sage/50 focus:ring-1 focus:ring-sage/25"
            />
          </div>

          {/* Worker count */}
          <div class="space-y-2">
            <label class="text-sm text-wool-300">Workers</label>
            <div class="flex items-center gap-3">
              <input
                type="range"
                min="1"
                max="5"
                value={workerScale()}
                onInput={(e) => setWorkerScale(parseInt(e.currentTarget.value))}
                class="flex-1 accent-sage"
              />
              <span class="w-8 text-center text-wool-200 font-medium tabular-nums">
                {workerScale()}
              </span>
            </div>
          </div>

          {/* Time limit */}
          <div class="space-y-2">
            <label class="text-sm text-wool-300">Time Limit (minutes)</label>
            <input
              type="number"
              min="5"
              max="480"
              value={timeLimit()}
              onInput={(e) => setTimeLimit(parseInt(e.currentTarget.value) || 60)}
              class="w-full px-4 py-2.5 bg-wool-800 border border-wool-600 rounded-xl text-wool-100 focus:outline-none focus:border-sage/50 focus:ring-1 focus:ring-sage/25"
            />
          </div>

          {/* Error */}
          <Show when={error()}>
            <div class="p-3 bg-red-500/10 border border-red-500/30 rounded-xl text-red-400 text-sm">
              {error()}
            </div>
          </Show>

          {/* Actions */}
          <div class="flex items-center justify-end gap-3 pt-2">
            <button
              type="button"
              onClick={props.onClose}
              class="px-4 py-2 text-sm text-wool-300 hover:text-wool-100 transition-colors"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={isLoading()}
              class="px-5 py-2.5 bg-sage hover:bg-sage/90 text-wool-900 font-medium rounded-xl transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2"
            >
              <Show when={isLoading()}>
                <i data-lucide="loader-2" class="w-4 h-4 animate-spin" />
              </Show>
              <span>Dispatch</span>
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
