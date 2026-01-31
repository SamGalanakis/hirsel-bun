/**
 * DeliveryModal - Three-tier delivery workflow for publishing run changes
 *
 * Delivery tiers:
 * 1. Push branch only - push to remote, no PR
 * 2. Create pull request - push + open PR
 * 3. Auto-merge - push + merge directly (only if clean)
 */

import { type Component, Show, createSignal, createEffect, onCleanup } from 'solid-js';
import { invoke } from '@tauri-apps/api/core';
import type { DeliveryState, PrInfo, MergeInfo, PushResult } from '../../lib/types';
import { MERGE_STATE_ICONS, MERGE_STATE_COLORS } from '../../lib/types';

export interface DeliveryModalProps {
  runName: string;
  targetBranch: string;
  summary?: string;
  taskIds: string[];
  evalIds: string[];
  onClose: () => void;
  onDelivered: () => void;
}

type DeliveryTier = 'push' | 'pr' | 'merge';

export const DeliveryModal: Component<DeliveryModalProps> = (props) => {
  const [deliveryState, setDeliveryState] = createSignal<DeliveryState | null>(null);
  const [loading, setLoading] = createSignal(true);
  const [delivering, setDelivering] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [selectedTier, setSelectedTier] = createSignal<DeliveryTier>('pr');

  // Load delivery state on mount
  createEffect(async () => {
    try {
      const state = await invoke<DeliveryState>('get_delivery_state', {
        runName: props.runName,
        targetBranch: props.targetBranch,
      });
      setDeliveryState(state);

      // Auto-select appropriate tier based on state
      if (state.mergeState === 'conflicts') {
        setSelectedTier('push');
      } else {
        setSelectedTier('pr');
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  });

  // Close on escape
  createEffect(() => {
    const handleKeydown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !delivering()) {
        props.onClose();
      }
    };
    window.addEventListener('keydown', handleKeydown);
    onCleanup(() => window.removeEventListener('keydown', handleKeydown));
  });

  // Generate PR content
  const prTitle = () => props.summary || `Changes from ${props.runName}`;
  const prBody = async () => {
    try {
      return await invoke<string>('generate_pr_body', {
        runName: props.runName,
        taskIds: props.taskIds,
        evalIds: props.evalIds,
      });
    } catch {
      return `Changes from ${props.runName}`;
    }
  };

  // Delivery actions
  const handleDeliver = async () => {
    setDelivering(true);
    setError(null);

    try {
      const tier = selectedTier();

      if (tier === 'push') {
        const result = await invoke<PushResult>('push_run_branch', {
          runName: props.runName,
        });
        window.toast?.success(`Branch pushed: ${result.branch}`);
      } else if (tier === 'pr') {
        const body = await prBody();
        const result = await invoke<PrInfo>('create_run_pr', {
          runName: props.runName,
          targetBranch: props.targetBranch,
          title: prTitle(),
          body,
        });
        window.toast?.success(`PR #${result.number} created`);
        // Open PR in browser
        if (result.url) {
          window.open(result.url, '_blank');
        }
      } else if (tier === 'merge') {
        const body = await prBody();
        const result = await invoke<MergeInfo>('auto_merge_run', {
          runName: props.runName,
          targetBranch: props.targetBranch,
          title: prTitle(),
          body,
        });
        if (result.merged) {
          window.toast?.success('Changes merged successfully');
        } else {
          throw new Error(result.message || 'Merge failed');
        }
      }

      props.onDelivered();
      props.onClose();
    } catch (e) {
      setError(String(e));
      window.toast?.error(`Delivery failed: ${e}`);
    } finally {
      setDelivering(false);
    }
  };

  // Handle Gyp merge (conflict resolution)
  const handleGypMerge = async () => {
    window.toast?.info('Gyp merge coming soon...');
  };

  // Mark as manually delivered
  const handleMarkDelivered = async () => {
    try {
      // Backend command not yet available - just notify for now
      window.toast?.success('Marked as delivered');
      props.onDelivered();
      props.onClose();
    } catch (e) {
      setError(String(e));
    }
  };

  const state = () => deliveryState();
  const hasConflicts = () => state()?.mergeState === 'conflicts';
  const isClean = () => state()?.mergeState === 'clean';
  const mergeIcon = () => state() ? MERGE_STATE_ICONS[state()!.mergeState] : '?';
  const mergeColorClass = () => state() ? `text-${MERGE_STATE_COLORS[state()!.mergeState]}` : '';

  return (
    <div
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/80"
      onClick={(e) => {
        if (e.target === e.currentTarget && !delivering()) props.onClose();
      }}
    >
      <div
        class="bg-zinc-900 border border-zinc-700 rounded-xl shadow-xl w-full max-w-md"
        style={{ 'box-shadow': '0 8px 32px rgba(0,0,0,0.5)' }}
      >
        {/* Header */}
        <div class="flex items-center justify-between px-5 py-4 border-b border-zinc-700">
          <h2 class="text-lg font-semibold text-zinc-100">
            Deliver: {props.runName}
          </h2>
          <button
            onClick={props.onClose}
            disabled={delivering()}
            class="text-zinc-500 hover:text-zinc-300 transition-colors disabled:opacity-50"
          >
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        {/* Content */}
        <div class="p-5">
          {/* Loading state */}
          <Show when={loading()}>
            <div class="flex items-center justify-center py-8">
              <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-amber-500" />
            </div>
          </Show>

          {/* Loaded state */}
          <Show when={!loading() && state()}>
            {/* Branch info */}
            <div class="mb-4 text-sm">
              <div class="flex items-center gap-2 text-zinc-400 mb-1">
                <span class="font-medium">Branch:</span>
                <span class="text-zinc-200 font-mono">{state()?.deliveryBranch || props.runName}</span>
              </div>
              <div class="flex items-center gap-2 text-zinc-400">
                <span class="font-medium">Target:</span>
                <span class="text-zinc-200 font-mono">{props.targetBranch}</span>
              </div>
            </div>

            {/* Merge state box */}
            <div
              class="rounded-lg p-3 mb-5"
              classList={{
                'bg-sage/10 border border-sage/30': isClean(),
                'bg-terra/10 border border-terra/30': hasConflicts(),
                'bg-zinc-800 border border-zinc-700': !isClean() && !hasConflicts(),
              }}
            >
              <div class="flex items-center gap-2 mb-1">
                <span class={`text-lg ${mergeColorClass()}`}>{mergeIcon()}</span>
                <span class="font-medium text-zinc-200">
                  Merge: {state()?.mergeState === 'clean' ? 'Clean' : state()?.mergeState === 'conflicts' ? 'Conflicts detected' : 'Unknown'}
                </span>
              </div>
              <Show when={state()!.stalenessCommits > 0}>
                <div class="text-sm text-amber-500">
                  {state()!.stalenessCommits} commit{state()!.stalenessCommits !== 1 ? 's' : ''} behind target
                </div>
              </Show>
              <Show when={hasConflicts() && state()!.conflictingFiles.length > 0}>
                <div class="text-sm text-terra mt-1">
                  {state()!.conflictingFiles.length} file{state()!.conflictingFiles.length !== 1 ? 's' : ''} with conflicts
                </div>
              </Show>
            </div>

            {/* Conflict resolution options */}
            <Show when={hasConflicts()}>
              <div class="mb-5 p-3 rounded-lg bg-terra/10 border border-terra/30">
                <div class="text-sm text-terra mb-3">
                  Conflicts must be resolved before merging
                </div>
                <div class="flex gap-2">
                  <button
                    onClick={handleGypMerge}
                    disabled={delivering()}
                    class="flex-1 px-3 py-2 rounded-lg text-sm font-medium bg-amber-500/20 text-amber-400 hover:bg-amber-500/30 transition-colors disabled:opacity-50"
                  >
                    Gyp Merge
                  </button>
                  <button
                    onClick={handleMarkDelivered}
                    disabled={delivering()}
                    class="flex-1 px-3 py-2 rounded-lg text-sm font-medium bg-zinc-700 text-zinc-300 hover:bg-zinc-600 transition-colors disabled:opacity-50"
                  >
                    I'll handle it
                  </button>
                </div>
              </div>
            </Show>

            {/* Delivery tier selection */}
            <Show when={!hasConflicts()}>
              <div class="space-y-2 mb-5">
                {/* Push only */}
                <label
                  class="flex items-start gap-3 p-3 rounded-lg cursor-pointer transition-colors"
                  classList={{
                    'bg-zinc-800 border border-zinc-600': selectedTier() === 'push',
                    'hover:bg-zinc-800/50': selectedTier() !== 'push',
                  }}
                >
                  <input
                    type="radio"
                    name="tier"
                    checked={selectedTier() === 'push'}
                    onChange={() => setSelectedTier('push')}
                    class="mt-1"
                  />
                  <div>
                    <div class="font-medium text-zinc-200">Push branch only</div>
                    <div class="text-sm text-zinc-500">Push to remote, no PR or merge</div>
                  </div>
                </label>

                {/* Create PR */}
                <label
                  class="flex items-start gap-3 p-3 rounded-lg cursor-pointer transition-colors"
                  classList={{
                    'bg-amber-500/10 border border-amber-500/30': selectedTier() === 'pr',
                    'hover:bg-zinc-800/50': selectedTier() !== 'pr',
                  }}
                >
                  <input
                    type="radio"
                    name="tier"
                    checked={selectedTier() === 'pr'}
                    onChange={() => setSelectedTier('pr')}
                    class="mt-1"
                  />
                  <div class="flex-1">
                    <div class="flex items-center gap-2">
                      <span class="font-medium text-zinc-200">Create pull request</span>
                      <span class="text-[10px] px-1.5 py-0.5 rounded bg-amber-500/20 text-amber-400 uppercase font-medium">
                        Recommended
                      </span>
                    </div>
                    <div class="text-sm text-zinc-500">Push + open PR to {props.targetBranch}</div>
                  </div>
                </label>

                {/* Auto-merge */}
                <label
                  class="flex items-start gap-3 p-3 rounded-lg cursor-pointer transition-colors"
                  classList={{
                    'bg-zinc-800 border border-zinc-600': selectedTier() === 'merge',
                    'hover:bg-zinc-800/50': selectedTier() !== 'merge',
                    'opacity-50 cursor-not-allowed': !isClean(),
                  }}
                >
                  <input
                    type="radio"
                    name="tier"
                    checked={selectedTier() === 'merge'}
                    onChange={() => isClean() && setSelectedTier('merge')}
                    disabled={!isClean()}
                    class="mt-1"
                  />
                  <div>
                    <div class="font-medium text-zinc-200">Auto-merge</div>
                    <div class="text-sm text-zinc-500">
                      Push + merge directly to {props.targetBranch}
                      {!isClean() && ' (requires clean merge)'}
                    </div>
                  </div>
                </label>
              </div>
            </Show>

            {/* Error display */}
            <Show when={error()}>
              <div class="mb-4 p-3 rounded-lg bg-terra/10 border border-terra/30 text-sm text-terra">
                {error()}
              </div>
            </Show>
          </Show>
        </div>

        {/* Footer */}
        <div class="flex items-center justify-end gap-3 px-5 py-4 border-t border-zinc-700">
          <button
            onClick={props.onClose}
            disabled={delivering()}
            class="px-4 py-2 rounded-lg text-sm font-medium text-zinc-400 hover:text-zinc-200 transition-colors disabled:opacity-50"
          >
            Cancel
          </button>
          <Show when={!loading() && !hasConflicts()}>
            <button
              onClick={handleDeliver}
              disabled={delivering()}
              class="px-4 py-2 rounded-lg text-sm font-medium bg-amber-500 text-zinc-900 hover:bg-amber-400 transition-colors disabled:opacity-50 flex items-center gap-2"
            >
              <Show when={delivering()}>
                <div class="animate-spin rounded-full h-4 w-4 border-b-2 border-zinc-900" />
              </Show>
              {delivering() ? 'Delivering...' : 'Deliver'}
            </button>
          </Show>
        </div>
      </div>
    </div>
  );
};

export default DeliveryModal;
