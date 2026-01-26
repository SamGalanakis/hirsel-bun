/**
 * RunSatellite - Small badge showing a dispatched run attached to a task
 *
 * Displays run status and merge state as compact indicators:
 * - Status dot (working/done/failed)
 * - Merge state indicator (clean/conflicts)
 * - Staleness indicator (+N commits)
 */

import type { Component } from 'solid-js';
import { Show } from 'solid-js';
import type { DispatchedRun, DeliveryStatus, MergeState } from '../../lib/types';
import {
  DELIVERY_STATUS_COLORS,
  DELIVERY_STATUS_ICONS,
  MERGE_STATE_COLORS,
  MERGE_STATE_ICONS,
} from '../../lib/types';

export interface RunSatelliteProps {
  run: DispatchedRun;
  onClick: () => void;
  compact?: boolean;
}

/** Get status color class */
const getStatusColor = (status: string): string => {
  switch (status) {
    case 'working':
      return 'bg-amber-500';
    case 'done':
      return 'bg-sage';
    case 'failed':
      return 'bg-terra';
    case 'eval':
      return 'bg-amber-400';
    default:
      return 'bg-wool-500';
  }
};

/** Get delivery status color */
const getDeliveryColor = (status: DeliveryStatus): string => {
  return `bg-${DELIVERY_STATUS_COLORS[status]}`;
};

/** Get merge state color */
const getMergeColor = (state: MergeState): string => {
  return `text-${MERGE_STATE_COLORS[state]}`;
};

/** Compact satellite - just a dot with tooltip */
const SatelliteCompact: Component<RunSatelliteProps> = (props) => {
  const statusColor = () => getStatusColor(props.run.status);
  const deliveryIcon = () => DELIVERY_STATUS_ICONS[props.run.deliveryStatus];
  const mergeIcon = () => MERGE_STATE_ICONS[props.run.mergeState];

  const tooltip = () => {
    let tip = `${props.run.runName}\nStatus: ${props.run.status}`;
    if (props.run.deliveryStatus !== 'pending') {
      tip += `\nDelivery: ${props.run.deliveryStatus}`;
    }
    if (props.run.mergeState !== 'unknown') {
      tip += `\nMerge: ${props.run.mergeState}`;
    }
    if (props.run.stalenessCommits > 0) {
      tip += `\n+${props.run.stalenessCommits} commits behind`;
    }
    return tip;
  };

  return (
    <button
      onClick={(e) => {
        e.stopPropagation();
        props.onClick();
      }}
      class="w-4 h-4 rounded-full flex items-center justify-center transition-all hover:scale-110"
      classList={{
        [statusColor()]: true,
      }}
      style={{
        'box-shadow': '0 1px 3px rgba(0,0,0,0.3)',
      }}
      title={tooltip()}
    >
      <Show when={props.run.mergeState === 'conflicts'}>
        <span class="text-[8px] text-white font-bold">!</span>
      </Show>
    </button>
  );
};

/** Full satellite - shows run name and indicators */
const SatelliteFull: Component<RunSatelliteProps> = (props) => {
  const statusColor = () => getStatusColor(props.run.status);
  const mergeIcon = () => MERGE_STATE_ICONS[props.run.mergeState];
  const mergeColor = () => getMergeColor(props.run.mergeState);

  return (
    <button
      onClick={(e) => {
        e.stopPropagation();
        props.onClick();
      }}
      class="flex items-center gap-1.5 px-2 py-1 rounded-full transition-all hover:bg-wool-800/50 group"
      style={{
        background: 'rgba(39,39,42,0.8)',
        border: '1px solid rgba(63,63,70,0.5)',
        'box-shadow': '0 2px 6px rgba(0,0,0,0.2)',
      }}
    >
      {/* Status dot */}
      <span
        class={`w-2 h-2 rounded-full ${statusColor()}`}
        style={{
          'box-shadow': props.run.status === 'working'
            ? '0 0 6px rgba(251,191,36,0.5)'
            : 'none',
        }}
      />

      {/* Run name (truncated) */}
      <span class="text-[10px] text-wool-300 truncate max-w-[80px] group-hover:text-wool-100">
        {props.run.runName}
      </span>

      {/* Merge state indicator */}
      <Show when={props.run.mergeState !== 'unknown'}>
        <span class={`text-[10px] ${mergeColor()}`} title={`Merge: ${props.run.mergeState}`}>
          {mergeIcon()}
        </span>
      </Show>

      {/* Staleness indicator */}
      <Show when={props.run.stalenessCommits > 0}>
        <span
          class="text-[9px] text-amber-500"
          title={`${props.run.stalenessCommits} commits behind target`}
        >
          +{props.run.stalenessCommits}
        </span>
      </Show>

      {/* PR indicator */}
      <Show when={props.run.prUrl}>
        <span class="text-[10px] text-sky-400" title="PR open">
          PR
        </span>
      </Show>
    </button>
  );
};

/** Main RunSatellite component */
export const RunSatellite: Component<RunSatelliteProps> = (props) => {
  return (
    <Show when={props.compact} fallback={<SatelliteFull {...props} />}>
      <SatelliteCompact {...props} />
    </Show>
  );
};

/** Container for multiple satellites attached to a task */
export interface RunSatelliteGroupProps {
  runs: DispatchedRun[];
  onRunClick: (runName: string) => void;
  maxVisible?: number;
  compact?: boolean;
}

export const RunSatelliteGroup: Component<RunSatelliteGroupProps> = (props) => {
  const maxVisible = () => props.maxVisible ?? 3;
  const visibleRuns = () => props.runs.slice(0, maxVisible());
  const hiddenCount = () => Math.max(0, props.runs.length - maxVisible());

  return (
    <div class="flex items-center gap-1 flex-wrap">
      {visibleRuns().map((run) => (
        <RunSatellite
          run={run}
          onClick={() => props.onRunClick(run.runName)}
          compact={props.compact}
        />
      ))}
      <Show when={hiddenCount() > 0}>
        <span
          class="text-[9px] text-wool-500 px-1"
          title={`${hiddenCount()} more runs`}
        >
          +{hiddenCount()}
        </span>
      </Show>
    </div>
  );
};

export default RunSatellite;
