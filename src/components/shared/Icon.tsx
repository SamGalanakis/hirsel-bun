import type { Component } from 'solid-js';
import { getIcon } from '../../lib/icons';

interface IconProps {
  name: string;
  size?: number;
  strokeWidth?: number;
  class?: string;
}

export const Icon: Component<IconProps> = (props) => {
  const svgString = () => getIcon(props.name, props.size ?? 16, props.strokeWidth ?? 2);

  return (
    <span
      class={props.class}
      innerHTML={svgString()}
      style={{ display: 'inline-flex', 'align-items': 'center', 'justify-content': 'center' }}
    />
  );
};
