/**
 * RunSelector - Dropdown multiselect for filtering visible runs on the board
 *
 * Groups runs by status category:
 * - Active: working, paused
 * - Completed: done, eval
 * - Delivered: merged, pushed, pr_open
 * - Failed: failed, abandoned
 */

import type { Component } from 'solid-js';
import { createSignal, Show, For, createMemo } from 'solid-js';
import type { DispatchedRun, RunStatus, DeliveryStatus } from '../../lib/types';
import { MERGE_STATE_ICONS, DELIVERY_STATUS_ICONS } from '../../lib/types';

export interface RunSelectorProps {
  runs: DispatchedRun[];
  selectedRuns: Set<string>;
  onSelectionChange: (selected: Set<string>) => void;
}

type RunCategory = 'active' | 'completed' | 'delivered' | 'failed';

interface CategorizedRuns {
  active: DispatchedRun[];
  completed: DispatchedRun[];
  delivered: DispatchedRun[];
  failed: DispatchedRun[];
}

/** Categorize a run based on its status */
const getCategory = (run: DispatchedRun): RunCategory => {
  // Check delivery status first
  if (run.deliveryStatus === 'merged' || run.deliveryStatus === 'pushed' || run.deliveryStatus === 'pr_open') {
    return 'delivered';
  }
  if (run.deliveryStatus === 'abandoned') {
    return 'failed';
  }

  // Check run status
  switch (run.status) {
    case 'working':
    case 'paused':
    case 'waiting':
      return 'active';
    case 'done':
    case 'eval':
      return 'completed';
    case 'failed':
      return 'failed';
    default:
      return 'completed';
  }
};

/** Get status icon */
const getStatusIcon = (run: DispatchedRun): string => {
  if (run.deliveryStatus !== 'pending') {
    return DELIVERY_STATUS_ICONS[run.deliveryStatus];
  }
  switch (run.status) {
    case 'working':
      return '\u25cf'; // ●
    case 'paused':
      return '\u23f8'; // ⏸
    case 'done':
      return '\u2713'; // ✓
    case 'failed':
      return '\u2717'; // ✗
    default:
      return '\u25cb'; // ○
  }
};

/** Get status color class */
const getStatusColorClass = (run: DispatchedRun): string => {
  if (run.deliveryStatus === 'merged') return 'text-sage';
  if (run.deliveryStatus === 'pr_open') return 'text-sky-400';
  if (run.deliveryStatus === 'abandoned') return 'text-wool-600';

  switch (run.status) {
    case 'working':
      return 'text-amber-500';
    case 'paused':
      return 'text-golden';
    case 'done':
      return 'text-sage';
    case 'failed':
      return 'text-terra';
    default:
      return 'text-wool-500';
  }
};

export const RunSelector: Component<RunSelectorProps> = (props) => {
  const [isOpen, setIsOpen] = createSignal(false);

  // Categorize runs
  const categorizedRuns = createMemo<CategorizedRuns>(() => {
    const result: CategorizedRuns = {
      active: [],
      completed: [],
      delivered: [],
      failed: [],
    };

    for (const run of props.runs) {
      const category = getCategory(run);
      result[category].push(run);
    }

    return result;
  });

  // Selection helpers
  const selectedCount = () => props.selectedRuns.size;
  const totalCount = () => props.runs.length;

  const isSelected = (runName: string) => props.selectedRuns.has(runName);

  const toggleRun = (runName: string) => {
    const newSet = new Set(props.selectedRuns);
    if (newSet.has(runName)) {
      newSet.delete(runName);
    } else {
      newSet.add(runName);
    }
    props.onSelectionChange(newSet);
  };

  const selectAll = () => {
    props.onSelectionChange(new Set(props.runs.map((r) => r.runName)));
  };

  const clearAll = () => {
    props.onSelectionChange(new Set());
  };

  const selectCategory = (category: RunCategory) => {
    const newSet = new Set(props.selectedRuns);
    for (const run of categorizedRuns()[category]) {
      newSet.add(run.runName);
    }
    props.onSelectionChange(newSet);
  };

  // Close on click outside
  let containerRef: HTMLDivElement | undefined;

  const handleClickOutside = (e: MouseEvent) => {
    if (containerRef && !containerRef.contains(e.target as Node)) {
      setIsOpen(false);
    }
  };

  // Category labels
  const categoryLabels: Record<RunCategory, string> = {
    active: 'Active',
    completed: 'Completed',
    delivered: 'Delivered',
    failed: 'Failed',
  };

  return (
    <div
      ref={containerRef}
      class="relative"
      onMouseLeave={() => setIsOpen(false)}
    >
      {/* Trigger button */}
      <button
        onClick={() => setIsOpen(!isOpen())}
        class="flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm transition-all"
        classList={{
          'bg-wool-800 text-wool-300 hover:bg-wool-700': !isOpen(),
          'bg-wool-700 text-wool-100': isOpen(),
        }}
      >
        <span class="text-wool-500">Runs:</span>
        <span class="font-medium">
          {selectedCount() === totalCount()
            ? 'All'
            : selectedCount() === 0
              ? 'None'
              : `${selectedCount()} selected`}
        </span>
        <svg
          class="w-4 h-4 transition-transform"
          classList={{ 'rotate-180': isOpen() }}
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
        </svg>
      </button>

      {/* Dropdown */}
      <Show when={isOpen()}>
        <div
          class="absolute top-full left-0 mt-1 w-64 rounded-lg shadow-xl z-50 overflow-hidden"
          style={{
            background: 'rgba(24,24,27,0.98)',
            border: '1px solid rgba(63,63,70,0.6)',
            'box-shadow': '0 8px 32px rgba(0,0,0,0.5)',
          }}
        >
          {/* Header actions */}
          <div
            class="flex items-center justify-between px-3 py-2"
            style={{ 'border-bottom': '1px solid rgba(63,63,70,0.4)' }}
          >
            <button
              onClick={selectAll}
              class="text-xs text-wool-400 hover:text-wool-200 transition-colors"
            >
              Select All
            </button>
            <button
              onClick={clearAll}
              class="text-xs text-wool-400 hover:text-wool-200 transition-colors"
            >
              Clear
            </button>
          </div>

          {/* Scrollable content */}
          <div class="max-h-80 overflow-y-auto">
            {/* Categories */}
            <For each={(['active', 'completed', 'delivered', 'failed'] as RunCategory[])}>
              {(category) => (
                <Show when={categorizedRuns()[category].length > 0}>
                  <div class="px-2 py-1">
                    {/* Category header */}
                    <button
                      onClick={() => selectCategory(category)}
                      class="w-full flex items-center justify-between px-2 py-1 text-[10px] font-bold uppercase tracking-widest text-wool-500 hover:text-wool-300 transition-colors"
                    >
                      <span>{categoryLabels[category]}</span>
                      <span class="text-wool-600">{categorizedRuns()[category].length}</span>
                    </button>

                    {/* Runs in category */}
                    <For each={categorizedRuns()[category]}>
                      {(run) => (
                        <button
                          onClick={() => toggleRun(run.runName)}
                          class="w-full flex items-center gap-2 px-2 py-1.5 rounded transition-all hover:bg-wool-800/50"
                        >
                          {/* Checkbox */}
                          <span
                            class="w-4 h-4 rounded border flex items-center justify-center text-[10px]"
                            classList={{
                              'bg-amber-500/20 border-amber-500 text-amber-400': isSelected(run.runName),
                              'border-wool-600': !isSelected(run.runName),
                            }}
                          >
                            <Show when={isSelected(run.runName)}>
                              <svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="3" d="M5 13l4 4L19 7" />
                              </svg>
                            </Show>
                          </span>

                          {/* Run name */}
                          <span class="flex-1 text-xs text-wool-300 truncate text-left">
                            {run.runName}
                          </span>

                          {/* Status icon */}
                          <span class={`text-xs ${getStatusColorClass(run)}`}>
                            {getStatusIcon(run)}
                          </span>

                          {/* Merge state */}
                          <Show when={run.mergeState !== 'unknown'}>
                            <span
                              class="text-[10px]"
                              classList={{
                                'text-sage': run.mergeState === 'clean',
                                'text-terra': run.mergeState === 'conflicts',
                              }}
                            >
                              {MERGE_STATE_ICONS[run.mergeState]}
                            </span>
                          </Show>

                          {/* Staleness */}
                          <Show when={run.stalenessCommits > 0}>
                            <span class="text-[9px] text-amber-500">
                              +{run.stalenessCommits}
                            </span>
                          </Show>
                        </button>
                      )}
                    </For>
                  </div>
                </Show>
              )}
            </For>

            {/* Empty state */}
            <Show when={props.runs.length === 0}>
              <div class="px-4 py-6 text-center text-sm text-wool-500">
                No runs dispatched yet
              </div>
            </Show>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default RunSelector;
