/**
 * About tab - version info
 */
import { type Component, Show, type Accessor } from 'solid-js';
import type { VersionInfo } from '../../../lib/types';

export interface AboutTabProps {
  versionInfo: Accessor<VersionInfo | null>;
}

export const AboutTab: Component<AboutTabProps> = (props) => {
  return (
    <div class="max-w-2xl mx-auto space-y-4">
      <div>
        <h3 class="font-medium text-wool-200">Hirsel</h3>
        <p class="text-sm text-wool-500 mt-1">
          Herd your AI coding agents
        </p>
      </div>
      <Show when={props.versionInfo()}>
        <div class="space-y-2 text-sm">
          <div class="flex justify-between">
            <span class="text-wool-500">Version</span>
            <span class="text-wool-300">{props.versionInfo()?.version}</span>
          </div>
          <div class="flex justify-between">
            <span class="text-wool-500">Build</span>
            <span class="text-wool-300 font-mono text-xs">{props.versionInfo()?.gitSha}</span>
          </div>
          <div class="flex justify-between">
            <span class="text-wool-500">Build Date</span>
            <span class="text-wool-300">{props.versionInfo()?.buildDate}</span>
          </div>
        </div>
      </Show>
    </div>
  );
};
