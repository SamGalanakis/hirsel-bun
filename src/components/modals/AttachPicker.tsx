/**
 * Attach picker modal for selecting workers/evals to attach to
 */
import { type Component, For, Show } from 'solid-js';
import { useSelection } from '../../stores';

export const AttachPicker: Component = () => {
  const selection = useSelection();

  const picker = () => selection.attachPicker();

  return (
    <Show when={picker().open}>
      <div
        class="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
        onClick={(e) => {
          if (e.target === e.currentTarget) selection.closeAttachPicker();
        }}
      >
        <div class="bg-pasture-800 border border-pasture-600 rounded-lg shadow-xl w-[400px] max-h-[500px] overflow-hidden">
          <div class="p-4 border-b border-pasture-600 flex items-center justify-between">
            <h2 class="text-lg font-medium text-wool-100">Attach to...</h2>
            <button
              onClick={() => selection.closeAttachPicker()}
              class="p-1 rounded hover:bg-pasture-700 text-wool-500"
            >
              <i data-lucide="x" class="w-4 h-4" />
            </button>
          </div>

          <div class="p-4 overflow-y-auto max-h-[400px]">
            {/* Workers section */}
            <Show when={picker().workers.length > 0}>
              <div class="mb-4">
                <h3 class="text-xs font-medium text-wool-500 uppercase tracking-wide mb-2">
                  Workers
                </h3>
                <div class="space-y-1">
                  <For each={picker().workers}>
                    {(worker) => (
                      <button
                        onClick={() =>
                          selection.attachToTarget('worker', worker.name)
                        }
                        class="w-full px-3 py-2 text-left text-sm rounded hover:bg-pasture-700 flex items-center justify-between gap-2"
                      >
                        <span class="flex items-center gap-2">
                          <i data-lucide="terminal" class="w-4 h-4 text-wool-500" />
                          <span class="text-wool-200">{worker.name}</span>
                        </span>
                        <span
                          class="text-xs px-1.5 py-0.5 rounded"
                          classList={{
                            'bg-sage/20 text-sage': worker.status === 'working',
                            'bg-wool-700 text-wool-400': worker.status === 'idle',
                            'bg-amber-500/20 text-amber-400':
                              worker.status === 'waiting',
                          }}
                        >
                          {worker.status}
                        </span>
                      </button>
                    )}
                  </For>
                </div>
              </div>
            </Show>

            {/* Evals section */}
            <Show when={picker().evals.length > 0}>
              <div>
                <h3 class="text-xs font-medium text-wool-500 uppercase tracking-wide mb-2">
                  Evals
                </h3>
                <div class="space-y-1">
                  <For each={picker().evals}>
                    {(evalItem) => (
                      <button
                        onClick={() =>
                          selection.attachToTarget('eval', evalItem.evalName)
                        }
                        class="w-full px-3 py-2 text-left text-sm rounded hover:bg-pasture-700 flex items-center justify-between gap-2"
                      >
                        <span class="flex items-center gap-2">
                          <i data-lucide="flask-conical" class="w-4 h-4 text-wool-500" />
                          <span class="text-wool-200">{evalItem.evalName}</span>
                        </span>
                        <span
                          class="text-xs px-1.5 py-0.5 rounded"
                          classList={{
                            'bg-sage/20 text-sage': evalItem.status === 'passed',
                            'bg-terra/20 text-terra': evalItem.status === 'failed',
                            'bg-amber-500/20 text-amber-400':
                              evalItem.status === 'running',
                          }}
                        >
                          {evalItem.status}
                        </span>
                      </button>
                    )}
                  </For>
                </div>
              </div>
            </Show>

            {/* Empty state */}
            <Show
              when={picker().workers.length === 0 && picker().evals.length === 0}
            >
              <div class="text-center py-8 text-wool-500">
                <i data-lucide="inbox" class="w-8 h-8 mx-auto mb-2 text-wool-600" />
                <p class="text-sm">No workers or evals available</p>
              </div>
            </Show>
          </div>
        </div>
      </div>
    </Show>
  );
};
