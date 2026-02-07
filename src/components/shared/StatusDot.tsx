import type { Component } from 'solid-js';
import { getWorkerStatusConfig } from '../../lib/utils/status';

export const StatusDot: Component<{ status: string; size?: 'sm' | 'md' }> = (props) => {
  const config = () => getWorkerStatusConfig(props.status);
  const sizeClass = () => props.size === 'sm' ? 'w-1.5 h-1.5' : 'w-2 h-2';
  return <span class={`rounded-full ${sizeClass()} ${config().dotClass}`} />;
};
