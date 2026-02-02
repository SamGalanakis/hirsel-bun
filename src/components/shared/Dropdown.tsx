/**
 * Shared Dropdown component - Basecoat-style select dropdown
 */
import { type Component, For, Show, createSignal } from 'solid-js';
import { useClickOutside, useEscapeKey } from '../../hooks';
import { Icon } from './Icon';

export interface DropdownOption {
  value: string;
  label: string;
}

export interface DropdownProps {
  value: string;
  options: DropdownOption[];
  onChange: (value: string) => void;
  placeholder?: string;
  class?: string;
}

export const Dropdown: Component<DropdownProps> = (props) => {
  const [open, setOpen] = createSignal(false);
  let containerRef: HTMLDivElement | undefined;

  const selectedLabel = () => {
    const option = props.options.find((o) => o.value === props.value);
    return option?.label || props.placeholder || 'Select...';
  };

  // Close on click outside or escape
  useClickOutside(() => containerRef, () => {
    if (open()) setOpen(false);
  });
  useEscapeKey(() => {
    if (open()) setOpen(false);
  });

  return (
    <div ref={containerRef} class={`dropdown relative ${props.class || ''}`}>
      <button
        type="button"
        class="btn-outline w-full justify-between"
        onClick={() => setOpen(!open())}
        aria-haspopup="listbox"
        aria-expanded={open()}
      >
        <span class="truncate flex-1 text-left" classList={{ 'text-muted-foreground': !props.value }}>
          {selectedLabel()}
        </span>
        <Icon name="chevrons-up-down" class="w-4 h-4 opacity-50 shrink-0" />
      </button>
      <Show when={open()}>
        <div
          data-popover
          class="absolute z-50 mt-1 w-full bg-popover border border-border rounded-md shadow-md py-1 max-h-60 overflow-auto"
        >
          <div role="listbox" aria-orientation="vertical">
            <For each={props.options}>
              {(option) => (
                <div
                  role="option"
                  aria-selected={props.value === option.value}
                  class="px-3 py-2 text-sm cursor-pointer hover:bg-accent flex items-center justify-between"
                  classList={{ 'bg-accent/50': props.value === option.value }}
                  onClick={() => {
                    props.onChange(option.value);
                    setOpen(false);
                  }}
                >
                  <span>{option.label}</span>
                  <Show when={props.value === option.value}>
                    <Icon name="check" class="w-4 h-4 text-primary" />
                  </Show>
                </div>
              )}
            </For>
          </div>
        </div>
      </Show>
    </div>
  );
};
