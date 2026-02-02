/**
 * Shared StatusBadge component - Displays run/task status with consistent styling
 */
import type { Component } from 'solid-js';
import { getStatusBadgeClass, getStatusLabel } from '../../lib/utils/status';
import type { RunStatus } from '../../lib/types';

export interface StatusBadgeProps {
  status: RunStatus | string | null | undefined;
  class?: string;
}

export const StatusBadge: Component<StatusBadgeProps> = (props) => (
  <span
    class={`px-2 py-0.5 text-xs font-medium rounded ${getStatusBadgeClass(props.status as RunStatus)} ${props.class || ''}`}
  >
    {getStatusLabel(props.status as RunStatus)}
  </span>
);
