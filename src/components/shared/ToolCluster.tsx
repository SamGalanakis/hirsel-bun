/**
 * Collapsed cluster view for multiple consecutive tools
 *
 * Shows count + status pips, expands to show individual tools.
 */
import { Component, For, Show, createSignal } from 'solid-js';
import { Icon } from './Icon';
import { ToolStatusPip } from './ToolStatusPip';
import type { WorkerEvent } from '../../lib/types';
import { getEffectiveToolStatus, getToolIcon, getToolShortLabel, getToolStatusIndicator } from '../../lib/tool-utils';

/** A single tool group within a cluster */
export interface ClusterToolGroup {
  events: WorkerEvent[];
}

export interface ToolClusterProps {
  /** Tool groups in this cluster */
  tools: ClusterToolGroup[];
}

/** Collapsed cluster view with status pips */
export const ToolCluster: Component<ToolClusterProps> = (props) => {
  const [expanded, setExpanded] = createSignal(false);

  const toolCount = () => props.tools.length;

  // Get effective status for each tool group
  // If tool has started but not completed/failed, treat as in_progress
  const toolStatuses = () =>
    props.tools.map((group) => {
      const hasStarted = group.events.some((e) => e.eventType === 'tool_start');
      const latestEvent = group.events[group.events.length - 1];
      return getEffectiveToolStatus(hasStarted, latestEvent?.toolStatus);
    });

  // Summary: how many done, failed, working, pending
  const statusSummary = () => {
    const statuses = toolStatuses();
    return {
      done: statuses.filter((s) => s === 'completed').length,
      failed: statuses.filter((s) => s === 'failed').length,
      working: statuses.filter((s) => s === 'in_progress').length,
      pending: statuses.filter((s) => s === 'pending' || !s).length,
    };
  };

  return (
    <div class="tool-cluster-container">
      {/* Collapsed view */}
      <button
        onClick={() => setExpanded(!expanded())}
        class="tool-cluster"
      >
        <Icon name="layers" class="w-3.5 h-3.5 text-wool-400" />
        <span class="text-wool-300">{toolCount()} tools</span>

        {/* Status pips */}
        <div class="tool-pips">
          <For each={toolStatuses()}>
            {(status) => <ToolStatusPip status={status} />}
          </For>
        </div>

        {/* Expand indicator */}
        <Icon
          name={expanded() ? 'chevron-up' : 'chevron-down'}
          class="w-3.5 h-3.5 text-wool-500"
        />
      </button>

      {/* Expanded view */}
      <Show when={expanded()}>
        <div class="tool-cluster-expanded">
          <For each={props.tools}>
            {(group) => {
              const titleEvent = () => group.events.find((e) => e.toolTitle) ?? group.events[0];
              const latestEvent = () => group.events[group.events.length - 1];
              const hasStarted = () => group.events.some((e) => e.eventType === 'tool_start');
              const status = () => getEffectiveToolStatus(hasStarted(), latestEvent()?.toolStatus);
              const statusIndicator = () => getToolStatusIndicator(status());

              return (
                <div class="tool-row">
                  {/* Status indicator */}
                  <Show when={statusIndicator().animate}>
                    <span class="w-3 h-3 border border-amber-500 border-t-transparent rounded-full animate-spin shrink-0" />
                  </Show>
                  <Show when={!statusIndicator().animate}>
                    <Icon
                      name={statusIndicator().icon}
                      class={`w-3 h-3 shrink-0 ${statusIndicator().color}`}
                    />
                  </Show>

                  {/* Tool icon */}
                  <Icon
                    name={getToolIcon(titleEvent()?.toolKind)}
                    class="w-3 h-3 text-wool-500 shrink-0"
                  />

                  {/* Tool name */}
                  <span class="text-wool-300 truncate">
                    {getToolShortLabel(titleEvent()?.toolTitle)}
                  </span>

                  {/* Status label for working tools */}
                  <Show when={status() === 'in_progress'}>
                    <span class="text-amber-500 text-[10px] ml-auto">running</span>
                  </Show>
                  <Show when={status() === 'pending'}>
                    <span class="text-wool-600 text-[10px] ml-auto">queued</span>
                  </Show>
                </div>
              );
            }}
          </For>
        </div>
      </Show>
    </div>
  );
};
