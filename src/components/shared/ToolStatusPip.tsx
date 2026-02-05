/**
 * Small status indicator pip (6x6px circle)
 *
 * Used by ToolCluster to show compact tool status.
 */
import { Component } from 'solid-js';
import { getToolPipClass } from '../../lib/tool-utils';

export interface ToolStatusPipProps {
  status: string | null | undefined;
}

/** Small status circle for tool clusters */
export const ToolStatusPip: Component<ToolStatusPipProps> = (props) => {
  const pipClass = () => getToolPipClass(props.status);

  return <span class={`tool-pip ${pipClass()}`} />;
};
