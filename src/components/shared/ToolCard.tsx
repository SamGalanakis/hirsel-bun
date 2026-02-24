/**
 * Shared tool call card component
 *
 * Used by both ShepherdConsole and WorkerOutputViewer for consistent tool display.
 */

import { Component, Show } from 'solid-js';
import { Icon } from './Icon';
import { getToolIcon, getToolShortLabel, getToolStatusIndicator, isToolWorking } from '../../lib/tool-utils';

/** Props for ToolCard component */
export interface ToolCardProps {
  /** Full tool name/title (e.g., "mcp__hirsel__board_view") */
  title: string | null | undefined;
  /** Tool kind (read, edit, execute, etc.) */
  kind: string | null | undefined;
  /** Tool execution status */
  status: string | null | undefined;
  /** Tool input (JSON string) */
  input: string | null | undefined;
  /** Tool output (JSON string) */
  output: string | null | undefined;
  /** Whether the card is expanded */
  expanded: boolean;
  /** Toggle expand/collapse */
  onToggle: () => void;
}

/** Craft-aesthetic tool call card with working indicator */
export const ToolCard: Component<ToolCardProps> = (props) => {
  const iconName = () => getToolIcon(props.kind);
  const shortLabel = () => getToolShortLabel(props.title);
  const working = () => isToolWorking(props.status);
  const statusIndicator = () => getToolStatusIndicator(props.status);

  const statusClass = () => {
    if (props.status === 'in_progress') return 'working';
    if (props.status === 'pending') return 'pending';
    if (props.status === 'completed') return 'completed';
    if (props.status === 'failed') return 'failed';
    return '';
  };

  // Parse input for display
  const parsedInput = () => {
    const input = props.input;
    if (!input) return null;
    try {
      const parsed = JSON.parse(input);
      return JSON.stringify(parsed, null, 2);
    } catch {
      return input;
    }
  };

  return (
    <div class="relative">
      {/* Card */}
      <button
        onClick={props.onToggle}
        class={`shepherd-tool-card ${statusClass()}`}
        classList={{
          'text-amber-400': props.status === 'in_progress',
          'text-wool-500': props.status === 'pending',
          'text-wool-400': props.status === 'completed',
          'text-terra': props.status === 'failed',
        }}
      >
        {/* Status indicator: distinct for pending vs in_progress */}
        <Show when={statusIndicator().animate}>
          <span class="w-3 h-3 border border-current border-t-transparent rounded-full animate-spin" />
        </Show>
        <Show when={!statusIndicator().animate && working()}>
          <Icon name={statusIndicator().icon} class={`w-3 h-3 ${statusIndicator().color}`} />
        </Show>
        <Show when={!working()}>
          <Icon name={iconName()} class="w-3 h-3" />
        </Show>
        <span>{shortLabel()}</span>
        <Show when={props.status === 'completed'}>
          <Icon name="check" class="w-3 h-3 text-sage" />
        </Show>
        <Show when={props.status === 'failed'}>
          <Icon name="x" class="w-3 h-3" />
        </Show>
      </button>

      {/* Expanded drawer */}
      <Show when={props.expanded}>
        <div class="shepherd-tool-drawer absolute left-0 top-full mt-1 z-50 w-72 bg-pasture-800 border border-pasture-600 rounded-lg shadow-xl overflow-hidden">
          {/* Header */}
          <div class="px-2.5 py-1.5 bg-pasture-700 border-b border-pasture-600 flex items-center justify-between">
            <span class="text-[11px] font-medium text-wool-200 truncate flex-1">
              {props.title || 'Tool'}
            </span>
            <button
              onClick={props.onToggle}
              class="p-0.5 rounded hover:bg-pasture-600 text-wool-500"
            >
              <Icon name="x" class="w-3 h-3" />
            </button>
          </div>

          {/* Content */}
          <div class="p-2 space-y-2 max-h-48 overflow-y-auto">
            <Show when={parsedInput()}>
              <div>
                <p class="text-[10px] text-wool-500 mb-0.5 uppercase tracking-wide">Input</p>
                <pre class="text-[10px] text-wool-300 bg-pasture-900 p-1.5 rounded overflow-x-auto whitespace-pre-wrap break-all">
                  {parsedInput()}
                </pre>
              </div>
            </Show>
            <Show when={props.output}>
              <div>
                <p class="text-[10px] text-wool-500 mb-0.5 uppercase tracking-wide">Output</p>
                <pre class="text-[10px] text-wool-300 bg-pasture-900 p-1.5 rounded overflow-x-auto whitespace-pre-wrap break-all max-h-24 overflow-y-auto">
                  {props.output}
                </pre>
              </div>
            </Show>
            <Show when={!props.input && !props.output}>
              <p class="text-[10px] text-wool-600 italic">No details available</p>
            </Show>
          </div>
        </div>
      </Show>
    </div>
  );
};
