/**
 * MultiSelectDropdown - Basecoat-style multi-select dropdown
 *
 * Similar to Dropdown but allows multiple selections with checkboxes.
 */
import { type Component, For, Show, createMemo } from 'solid-js';
import { DropdownShell, type DropdownOption } from './Dropdown';
import { Icon } from './Icon';

export interface MultiSelectDropdownProps {
  value: string[];
  options: DropdownOption[];
  onChange: (value: string[]) => void;
  placeholder?: string;
  label?: string; // Static label to show instead of selected values
  class?: string;
}

export const MultiSelectDropdown: Component<MultiSelectDropdownProps> = (props) => {
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

  return (
    <DropdownShell
      displayLabel={() => (
        <span class="flex items-center gap-1.5">
          {displayLabel()}
          <Show when={selectedCount() > 0 && selectedCount() < props.options.length}>
            <span class="px-1.5 py-0.5 text-[10px] rounded-full bg-accent text-accent-foreground">
              {selectedCount()}
            </span>
          </Show>
        </span>
      )}
      isEmpty={() => props.value.length === 0}
      class={props.class}
      multiselectable
    >
      {() => (
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
      )}
    </DropdownShell>
  );
};
