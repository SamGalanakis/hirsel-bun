/**
 * DeliveryDialog - Dialog for delivering board changes to a branch
 *
 * Shows:
 * - Target branch input (pre-dispatch)
 * - Delivery status and progress
 * - Action buttons based on status
 */

import { type Component, Show, createSignal, createEffect } from 'solid-js';
import { useEscapeKey } from '../../hooks';
import { useDelta } from '../../stores/delta-context';
import { Icon } from '../shared';
import type { BoardDeliveryStatus } from '../../lib/types';

interface DeliveryDialogProps {
  onClose: () => void;
  defaultBranch?: string;
}

export const DeliveryDialog: Component<DeliveryDialogProps> = (props) => {
  const delta = useDelta();

  // Form state
  const [targetBranch, setTargetBranch] = createSignal(props.defaultBranch || 'main');
  const [error, setError] = createSignal<string | null>(null);

  useEscapeKey(() => {
    if (!delta.deliveryPending()) {
      props.onClose();
    }
  });

  // Handle starting delivery
  const handleStartDelivery = async () => {
    setError(null);
    const branch = targetBranch().trim();
    if (!branch) {
      setError('Target branch is required');
      return;
    }

    const result = await delta.startDelivery(branch);
    if (!result) {
      setError('Failed to start delivery');
    }
  };

  // Handle completing delivery with PR creation
  const handleCreatePR = async () => {
    setError(null);
    const result = await delta.completeDelivery('pr');
    if (!result) {
      setError('Failed to create PR');
    }
  };

  // Handle abandoning delivery
  const handleAbandon = async () => {
    setError(null);
    const result = await delta.abandonDelivery();
    if (result) {
      props.onClose();
    } else {
      setError('Failed to abandon delivery');
    }
  };

  // Handle retry
  const handleRetry = async () => {
    setError(null);
    await delta.retryDelivery();
  };

  // Get current delivery status
  const delivery = () => delta.currentDelivery();
  const isActive = () => {
    const d = delivery();
    return d && !['abandoned', 'merged'].includes(d.status);
  };

  // Status display helpers
  const statusColor = (status: BoardDeliveryStatus): string => {
    switch (status) {
      case 'pending':
        return 'var(--wool-500)';
      case 'in_progress':
        return 'var(--amber-500)';
      case 'resolving_conflicts':
        return 'var(--amber-400)';
      case 'pushed':
        return 'var(--sage)';
      case 'pr_open':
        return 'var(--sky-400)';
      case 'merged':
        return 'var(--sage)';
      case 'failed':
        return 'var(--terra)';
      case 'abandoned':
        return 'var(--wool-600)';
      default:
        return 'var(--wool-500)';
    }
  };

  const statusLabel = (status: BoardDeliveryStatus): string => {
    switch (status) {
      case 'pending':
        return 'Pending';
      case 'in_progress':
        return 'Pushing...';
      case 'resolving_conflicts':
        return 'Resolving Conflicts';
      case 'pushed':
        return 'Pushed';
      case 'pr_open':
        return 'PR Open';
      case 'merged':
        return 'Merged';
      case 'failed':
        return 'Failed';
      case 'abandoned':
        return 'Abandoned';
      default:
        return status;
    }
  };

  return (
    <div
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={(e) => {
        if (e.target === e.currentTarget && !delta.deliveryPending()) {
          props.onClose();
        }
      }}
    >
      <div
        class="w-full max-w-md rounded-lg shadow-xl"
        style={{
          background: 'linear-gradient(180deg, #2a2a2a 0%, #242424 100%)',
          border: '1px solid rgba(64, 64, 64, 0.6)',
        }}
      >
        {/* Header */}
        <div
          class="flex items-center justify-between px-4 py-3"
          style={{ 'border-bottom': '1px solid rgba(64, 64, 64, 0.4)' }}
        >
          <h2 class="text-sm font-semibold text-wool-200">
            {isActive() ? 'Delivery Status' : 'Deliver Changes'}
          </h2>
          <button
            onClick={props.onClose}
            disabled={delta.deliveryPending()}
            class="p-1 rounded text-wool-500 hover:text-wool-300 hover:bg-pasture-700 disabled:opacity-30"
          >
            <Icon name="x" class="w-4 h-4" />
          </button>
        </div>

        {/* Content */}
        <div class="px-4 py-4">
          {/* Pre-delivery form */}
          <Show when={!isActive()}>
            <div class="space-y-4">
              <div>
                <label class="block text-xs font-medium text-wool-400 mb-1.5">
                  Target branch
                </label>
                <input
                  type="text"
                  value={targetBranch()}
                  onInput={(e) => setTargetBranch(e.currentTarget.value)}
                  placeholder="main"
                  class="w-full px-3 py-2 rounded text-sm text-wool-200 placeholder:text-wool-600"
                  style={{
                    background: 'rgba(36, 36, 36, 0.8)',
                    border: '1px solid rgba(64, 64, 64, 0.5)',
                  }}
                />
              </div>

              <div
                class="flex items-center gap-2 px-3 py-2 rounded text-xs text-wool-400"
                style={{ background: 'rgba(125, 153, 112, 0.08)' }}
              >
                <Icon name="git-branch" class="w-3.5 h-3.5 text-sage" />
                <span>
                  Creates branch: <span class="text-wool-300 font-medium">hirsel/...</span>
                </span>
              </div>
            </div>
          </Show>

          {/* Active delivery status */}
          <Show when={isActive()}>
            <div class="space-y-4">
              {/* Status indicator */}
              <div class="flex items-center gap-3">
                <div
                  class={`w-2.5 h-2.5 rounded-full ${
                    delivery()?.status === 'in_progress' ? 'animate-pulse' : ''
                  }`}
                  style={{ background: statusColor(delivery()!.status) }}
                />
                <span class="text-sm font-medium" style={{ color: statusColor(delivery()!.status) }}>
                  {statusLabel(delivery()!.status)}
                </span>
              </div>

              {/* Branch info */}
              <div class="space-y-2 text-xs">
                <div class="flex items-center justify-between">
                  <span class="text-wool-500">Target:</span>
                  <span class="text-wool-300 font-mono">{delivery()?.targetBranch}</span>
                </div>
                <Show when={delivery()?.deliveryBranch}>
                  <div class="flex items-center justify-between">
                    <span class="text-wool-500">Branch:</span>
                    <span class="text-wool-300 font-mono">{delivery()?.deliveryBranch}</span>
                  </div>
                </Show>
                <Show when={delivery()?.prNumber}>
                  <div class="flex items-center justify-between">
                    <span class="text-wool-500">PR:</span>
                    <a
                      href={delivery()?.prUrl || '#'}
                      target="_blank"
                      rel="noopener noreferrer"
                      class="text-sky-400 hover:underline font-mono"
                    >
                      #{delivery()?.prNumber}
                    </a>
                  </div>
                </Show>
              </div>

              {/* Failure message */}
              <Show when={delivery()?.status === 'failed' && delivery()?.failureReason}>
                <div
                  class="px-3 py-2 rounded text-xs text-terra"
                  style={{ background: 'rgba(196, 92, 74, 0.1)' }}
                >
                  {delivery()?.failureReason}
                </div>
              </Show>
            </div>
          </Show>

          {/* Error message */}
          <Show when={error()}>
            <div
              class="mt-3 px-3 py-2 rounded text-xs text-terra"
              style={{ background: 'rgba(196, 92, 74, 0.1)' }}
            >
              {error()}
            </div>
          </Show>
        </div>

        {/* Footer with actions */}
        <div
          class="flex items-center justify-end gap-2 px-4 py-3"
          style={{ 'border-top': '1px solid rgba(64, 64, 64, 0.4)' }}
        >
          {/* Pre-delivery actions */}
          <Show when={!isActive()}>
            <button
              onClick={props.onClose}
              class="px-3 py-1.5 rounded text-xs font-medium text-wool-400 hover:text-wool-200 hover:bg-pasture-700"
            >
              Cancel
            </button>
            <button
              onClick={handleStartDelivery}
              disabled={delta.deliveryPending()}
              class="px-3 py-1.5 rounded text-xs font-medium disabled:opacity-30"
              style={{
                background: 'rgba(125, 153, 112, 0.2)',
                border: '1px solid rgba(125, 153, 112, 0.35)',
                color: 'var(--sage)',
              }}
            >
              {delta.deliveryPending() ? 'Starting...' : 'Deliver'}
            </button>
          </Show>

          {/* Pushed - can create PR or close */}
          <Show when={delivery()?.status === 'pushed'}>
            <button
              onClick={handleAbandon}
              disabled={delta.deliveryPending()}
              class="px-3 py-1.5 rounded text-xs font-medium text-wool-400 hover:text-wool-200 hover:bg-pasture-700 disabled:opacity-30"
            >
              Close
            </button>
            <button
              onClick={handleCreatePR}
              disabled={delta.deliveryPending()}
              class="px-3 py-1.5 rounded text-xs font-medium disabled:opacity-30"
              style={{
                background: 'rgba(56, 189, 248, 0.15)',
                border: '1px solid rgba(56, 189, 248, 0.3)',
                color: 'var(--sky-400)',
              }}
            >
              <Icon name="git-pull-request" class="w-3 h-3 mr-1.5 inline" />
              {delta.deliveryPending() ? 'Creating...' : 'Create PR'}
            </button>
          </Show>

          {/* PR Open - done button */}
          <Show when={delivery()?.status === 'pr_open'}>
            <button
              onClick={props.onClose}
              class="px-3 py-1.5 rounded text-xs font-medium"
              style={{
                background: 'rgba(125, 153, 112, 0.2)',
                border: '1px solid rgba(125, 153, 112, 0.35)',
                color: 'var(--sage)',
              }}
            >
              Done
            </button>
          </Show>

          {/* Failed - retry or abandon */}
          <Show when={delivery()?.status === 'failed'}>
            <button
              onClick={handleAbandon}
              disabled={delta.deliveryPending()}
              class="px-3 py-1.5 rounded text-xs font-medium text-wool-400 hover:text-wool-200 hover:bg-pasture-700 disabled:opacity-30"
            >
              Abandon
            </button>
            <button
              onClick={handleRetry}
              disabled={delta.deliveryPending()}
              class="px-3 py-1.5 rounded text-xs font-medium disabled:opacity-30"
              style={{
                background: 'rgba(212, 165, 116, 0.15)',
                border: '1px solid rgba(212, 165, 116, 0.3)',
                color: 'var(--amber-400)',
              }}
            >
              {delta.deliveryPending() ? 'Retrying...' : 'Retry'}
            </button>
          </Show>

          {/* In progress - just show cancel */}
          <Show when={delivery()?.status === 'in_progress' || delivery()?.status === 'pending'}>
            <button
              onClick={handleAbandon}
              disabled={delta.deliveryPending()}
              class="px-3 py-1.5 rounded text-xs font-medium text-wool-400 hover:text-wool-200 hover:bg-pasture-700 disabled:opacity-30"
            >
              Cancel
            </button>
          </Show>
        </div>
      </div>
    </div>
  );
};
