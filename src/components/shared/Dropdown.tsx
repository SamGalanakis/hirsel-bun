/**
 * Shared Dropdown component - Basecoat-style select dropdown
 */
import { type Component, type JSX, Show, For, createSignal } from 'solid-js';
import { useModalClosing } from '../../hooks';
import { Icon } from './Icon';

export interface DropdownOption {
  value: string;
  label: string;
}

// Shared shell for dropdown trigger + popover container
interface DropdownShellProps {
  displayLabel: () => JSX.Element;
  isEmpty: () => boolean;
  class?: string;
  triggerClass?: string;
  panelClass?: string;
  multiselectable?: boolean;
  children: (close: () => void) => JSX.Element;
}

export const DropdownShell: Component<DropdownShellProps> = (props) => {
  const [open, setOpen] = createSignal(false);
  let containerRef: HTMLDivElement | undefined;

  useModalClosing(() => containerRef, open, () => setOpen(false));

  const triggerClass = () => props.triggerClass || 'btn-outline w-full justify-between';
  const panelClass = () =>
    props.panelClass ||
    'absolute z-50 mt-1 w-full bg-popover border border-border rounded-md shadow-md py-1 max-h-60 overflow-auto';

  return (
    <div ref={containerRef} class={`dropdown relative ${props.class || ''}`}>
      <button
        type="button"
        class={triggerClass()}
        onClick={() => setOpen(!open())}
        aria-haspopup="listbox"
        aria-expanded={open()}
      >
        <span class="truncate flex-1 text-left" classList={{ 'text-muted-foreground': props.isEmpty() }}>
          {props.displayLabel()}
        </span>
        <Icon name="chevrons-up-down" class="w-4 h-4 opacity-50 shrink-0" />
      </button>
      <Show when={open()}>
        <div
          data-popover
          class={panelClass()}
        >
          <div role="listbox" aria-orientation="vertical" aria-multiselectable={props.multiselectable}>
            {props.children(() => setOpen(false))}
          </div>
        </div>
      </Show>
    </div>
  );
};

export interface DropdownProps {
  value: string;
  options: DropdownOption[];
  onChange: (value: string) => void;
  placeholder?: string;
  class?: string;
  triggerClass?: string;
  panelClass?: string;
}

export const Dropdown: Component<DropdownProps> = (props) => {
  const selectedLabel = () => {
    const option = props.options.find((o) => o.value === props.value);
    return option?.label || props.placeholder || 'Select...';
  };

  return (
    <DropdownShell
      displayLabel={() => <>{selectedLabel()}</>}
      isEmpty={() => !props.value}
      class={props.class}
      triggerClass={props.triggerClass}
      panelClass={props.panelClass}
    >
      {(close) => (
        <For each={props.options}>
          {(option) => (
            <div
              role="option"
              aria-selected={props.value === option.value}
              class="px-3 py-2 text-sm cursor-pointer hover:bg-accent flex items-center justify-between"
              classList={{ 'bg-accent/50': props.value === option.value }}
              onClick={() => {
                props.onChange(option.value);
                close();
              }}
            >
              <span>{option.label}</span>
              <Show when={props.value === option.value}>
                <Icon name="check" class="w-4 h-4 text-primary" />
              </Show>
            </div>
          )}
        </For>
      )}
    </DropdownShell>
  );
};
