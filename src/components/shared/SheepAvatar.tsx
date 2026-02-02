import { generateSheepSvg } from '../../lib/sheep-avatar';
import type { SheepConfig, WorkerStatus } from '../../lib/types';

interface SheepAvatarProps {
  config: SheepConfig;
  size?: number;
  status?: WorkerStatus;
  class?: string;
}

export function SheepAvatar(props: SheepAvatarProps) {
  return (
    <div
      class={props.class}
      innerHTML={generateSheepSvg(props.config, props.size, props.status)}
    />
  );
}
