/**
 * Collapsible thinking block component
 *
 * Displays AI thinking/reasoning with brain icon.
 * Used by both GypMessenger and WorkerOutputViewer.
 */
import { Component, Show, createSignal } from 'solid-js';
import { Icon } from './Icon';

export interface ThinkingBlockProps {
  /** Thinking content to display */
  content: string;
  /** Whether the block starts expanded (default: false) */
  defaultExpanded?: boolean;
  /** Whether to allow collapsing (default: true) */
  collapsible?: boolean;
}

/** Collapsible thought bubble with brain icon */
export const ThinkingBlock: Component<ThinkingBlockProps> = (props) => {
  const [expanded, setExpanded] = createSignal(props.defaultExpanded ?? false);
  const isCollapsible = () => props.collapsible !== false;

  return (
    <div
      class={`gyp-thinking-block p-2 ${isCollapsible() ? 'cursor-pointer' : ''} ${expanded() ? 'expanded' : 'collapsed'}`}
      onClick={() => isCollapsible() && setExpanded(!expanded())}
    >
      <div class="flex items-center gap-1.5 text-amber-500/70 text-xs mb-1">
        <Icon name="brain" class="w-3 h-3" />
        <span class="italic" style="font-family: 'ET Book', serif;">
          Thinking
        </span>
        <Show when={isCollapsible()}>
          <Icon
            name={expanded() ? 'chevron-up' : 'chevron-down'}
            class="w-3 h-3 ml-auto"
          />
        </Show>
      </div>
      <p
        class="text-xs text-wool-500 whitespace-pre-wrap overflow-hidden"
        classList={{ 'line-clamp-1': isCollapsible() && !expanded() }}
      >
        {props.content}
      </p>
    </div>
  );
};
