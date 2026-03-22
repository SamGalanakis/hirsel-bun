/**
 * Collapsed cluster view for multiple consecutive tools
 *
 * Shows count + status pips, expands to show individual tools.
 * Click on a tool to see its input/output details.
 * Used by both WorkerOutputViewer and ShepherdConsole.
 */
import { Component, For, Show, createSignal } from 'solid-js';
import { Icon } from './Icon';
import { ToolStatusPip } from './ToolStatusPip';
import { getToolIcon, getToolShortLabel, getToolStatusIndicator } from '../../lib/tool-utils';

/** Generic tool info for clustering - works with both ChatToolCall and WorkerEvent */
export interface ToolInfo {
  id: string;
  title: string | null | undefined;
  kind: string | null | undefined;
  status: string;
  input?: string | null;
  output?: string | null;
}

export interface ToolClusterProps {
  /** Tools to display in this cluster */
  tools: ToolInfo[];
}

/** Parse and format JSON input for display */
const formatInput = (input: string | null | undefined): string | null => {
  if (!input) return null;
  try {
    const parsed = JSON.parse(input);
    return JSON.stringify(parsed, null, 2);
  } catch {
    return input;
  }
};

/** Collapsed cluster view with status pips */
export const ToolCluster: Component<ToolClusterProps> = (props) => {
  const [expanded, setExpanded] = createSignal(false);
  const [selectedToolId, setSelectedToolId] = createSignal<string | null>(null);

  const toolCount = () => props.tools.length;
  const toolStatuses = () => props.tools.map((t) => t.status);
  const selectedTool = () => props.tools.find((t) => t.id === selectedToolId());

  const toggleTool = (id: string) => {
    setSelectedToolId((prev) => (prev === id ? null : id));
  };

  return (
    <div class="tool-cluster-container">
      {/* Collapsed view */}
      <button onClick={() => setExpanded(!expanded())} class="tool-cluster">
        <Icon name="layers" class="w-3.5 h-3.5 text-wool-400" />
        <span class="text-wool-300">{toolCount()} tools</span>

        {/* Status pips (max 12, then overflow indicator) */}
        <div class="tool-pips">
          <For each={toolStatuses().slice(0, 12)}>
            {(status) => <ToolStatusPip status={status} />}
          </For>
          <Show when={toolCount() > 12}>
            <span class="text-[10px] text-wool-500">+{toolCount() - 12}</span>
          </Show>
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
            {(tool) => {
              const statusIndicator = () => getToolStatusIndicator(tool.status);
              const isSelected = () => selectedToolId() === tool.id;

              return (
                <div class="tool-row-container">
                  <button
                    class="tool-row"
                    classList={{ selected: isSelected() }}
                    onClick={() => toggleTool(tool.id)}
                  >
                    {/* Status indicator */}
                    <Show when={statusIndicator().animate}>
                      <span class="w-3 h-3 border border-amber-500 border-t-transparent rounded-none animate-spin shrink-0" />
                    </Show>
                    <Show when={!statusIndicator().animate}>
                      <Icon
                        name={statusIndicator().icon}
                        class={`w-3 h-3 shrink-0 ${statusIndicator().color}`}
                      />
                    </Show>

                    {/* Tool icon */}
                    <Icon name={getToolIcon(tool.kind)} class="w-3 h-3 text-wool-500 shrink-0" />

                    {/* Tool name */}
                    <span class="text-wool-300 truncate">{getToolShortLabel(tool.title)}</span>

                    {/* Status label for working tools */}
                    <Show when={tool.status === 'in_progress'}>
                      <span class="text-amber-500 text-[10px] ml-auto">running</span>
                    </Show>
                    <Show when={tool.status === 'pending'}>
                      <span class="text-wool-600 text-[10px] ml-auto">queued</span>
                    </Show>

                    {/* Expand indicator */}
                    <Icon
                      name={isSelected() ? 'chevron-up' : 'chevron-down'}
                      class="w-3 h-3 text-wool-600 ml-1"
                    />
                  </button>

                  {/* Tool details drawer */}
                  <Show when={isSelected()}>
                    <div class="tool-row-details">
                      <div class="text-[10px] text-wool-500 mb-1 truncate" title={tool.title || ''}>
                        {tool.title || 'Tool'}
                      </div>
                      <Show when={tool.input}>
                        <div class="mb-2">
                          <p class="text-[10px] text-wool-600 mb-0.5 uppercase tracking-wide">
                            Input
                          </p>
                          <pre class="text-[10px] text-wool-400 bg-pasture-900 p-1.5 rounded-none overflow-x-auto whitespace-pre-wrap break-all max-h-24 overflow-y-auto">
                            {formatInput(tool.input)}
                          </pre>
                        </div>
                      </Show>
                      <Show when={tool.output}>
                        <div>
                          <p class="text-[10px] text-wool-600 mb-0.5 uppercase tracking-wide">
                            Output
                          </p>
                          <pre class="text-[10px] text-wool-400 bg-pasture-900 p-1.5 rounded-none overflow-x-auto whitespace-pre-wrap break-all max-h-24 overflow-y-auto">
                            {tool.output}
                          </pre>
                        </div>
                      </Show>
                      <Show when={!tool.input && !tool.output}>
                        <p class="text-[10px] text-wool-600 italic">No details available</p>
                      </Show>
                    </div>
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
