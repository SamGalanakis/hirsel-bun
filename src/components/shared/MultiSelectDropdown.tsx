/**
 * MultiSelectDropdown - Basecoat-style multi-select dropdown
 *
 * Similar to Dropdown but allows multiple selections with checkboxes.
 */
import { type Component, For, Show, createSignal, createMemo } from 'solid-js';
import { useModalClosing } from '../../hooks';
import { Icon } from './Icon';

export interface MultiSelectOption {
  value: string;
  label: string;
}

export interface MultiSelectDropdownProps {
  value: string[];
  options: MultiSelectOption[];
  onChange: (value: string[]) => void;
  placeholder?: string;
  label?: string; // Static label to show instead of selected values
  class?: string;
}

export const MultiSelectDropdown: Component<MultiSelectDropdownProps> = (props) => {
  const [open, setOpen] = createSignal(false);
  let containerRef: HTMLDivElement | undefined;

  const selectedCount = createMemo(() => props.value.length);

  const displayLabel = createMemo(() => {
    // Use static label if provided
    if (props.label) {
      return props.label;
    }
    if (props.value.length === 0) {
      return props.placeholder || 'Select...';
    }
    if (props.value.length === props.options.length) {
      return 'All';
    }
    // Show first selected label + count if more
    const firstSelected = props.options.find((o) => props.value.includes(o.value));
    if (props.value.length === 1) {
      return firstSelected?.label || props.placeholder || 'Select...';
    }
    return `${firstSelected?.label} +${props.value.length - 1}`;
  });

  const toggleOption = (optionValue: string) => {
    const currentValues = props.value;
    if (currentValues.includes(optionValue)) {
      props.onChange(currentValues.filter((v) => v !== optionValue));
    } else {
      props.onChange([...currentValues, optionValue]);
    }
  };

  // Close on click outside or escape
  useModalClosing(() => containerRef, open, () => setOpen(false));

  return (
    <div ref={containerRef} class={`dropdown relative ${props.class || ''}`}>
      <button
        type="button"
        class="btn-outline w-full justify-between"
        onClick={() => setOpen(!open())}
        aria-haspopup="listbox"
        aria-expanded={open()}
      >
        <span class="truncate flex-1 text-left flex items-center gap-1.5" classList={{ 'text-muted-foreground': props.value.length === 0 }}>
          {displayLabel()}
          <Show when={selectedCount() > 0 && selectedCount() < props.options.length}>
            <span class="px-1.5 py-0.5 text-[10px] rounded-full bg-accent text-accent-foreground">
              {selectedCount()}
            </span>
          </Show>
        </span>
        <Icon name="chevrons-up-down" class="w-4 h-4 opacity-50 shrink-0" />
      </button>
      <Show when={open()}>
        <div
          data-popover
          class="absolute z-50 mt-1 w-full bg-popover border border-border rounded-md shadow-md py-1 max-h-60 overflow-auto"
        >
          <div role="listbox" aria-orientation="vertical" aria-multiselectable="true">
            <For each={props.options}>
              {(option) => {
                const isSelected = () => props.value.includes(option.value);
                return (
                  <div
                    role="option"
                    aria-selected={isSelected()}
                    class="px-3 py-2 text-sm cursor-pointer hover:bg-accent flex items-center gap-2"
                    classList={{ 'bg-accent/50': isSelected() }}
                    onClick={() => toggleOption(option.value)}
                  >
                    <div
                      class="w-4 h-4 rounded border flex items-center justify-center shrink-0"
                      classList={{
                        'bg-primary border-primary': isSelected(),
                        'border-muted-foreground': !isSelected(),
                      }}
                    >
                      <Show when={isSelected()}>
                        <Icon name="check" class="w-3 h-3 text-primary-foreground" />
                      </Show>
                    </div>
                    <span>{option.label}</span>
                  </div>
                );
              }}
            </For>
          </div>
        </div>
      </Show>
    </div>
  );
};
