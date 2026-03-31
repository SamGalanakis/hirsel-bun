import { Select as KobalteSelect } from "@kobalte/core/select";
import { type Component, splitProps } from "solid-js";
import { cn } from "@/lib/cn";

/* ── Composed single-value Select ── */

interface SelectOption {
  value: string;
  label: string;
  description?: string;
}

interface SelectProps {
  options: SelectOption[];
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  class?: string;
  disabled?: boolean;
}

const Select: Component<SelectProps> = (props) => {
  const [local] = splitProps(props, [
    "options",
    "value",
    "onChange",
    "placeholder",
    "class",
    "disabled",
  ]);

  const selected = () => local.options.find((o) => o.value === local.value) ?? null;

  return (
    <KobalteSelect<SelectOption>
      options={local.options}
      optionValue="value"
      optionTextValue="label"
      value={selected()}
      onChange={(opt) => {
        if (opt) local.onChange(opt.value);
      }}
      placeholder={local.placeholder}
      disabled={local.disabled}
      disallowEmptySelection
      itemComponent={(itemProps) => (
        <KobalteSelect.Item
          item={itemProps.item}
          class={cn(
            "relative flex flex-col gap-0.5 px-3 py-2.5 text-sm outline-none cursor-pointer",
            "text-foreground",
            "data-[highlighted]:bg-accent data-[highlighted]:text-accent-foreground",
            "data-[selected]:font-medium",
          )}
        >
          <KobalteSelect.ItemLabel class="font-body text-sm">
            {itemProps.item.rawValue.label}
          </KobalteSelect.ItemLabel>
          {itemProps.item.rawValue.description && (
            <span class="text-xs text-muted-foreground">
              {itemProps.item.rawValue.description}
            </span>
          )}
          <KobalteSelect.ItemIndicator class="absolute right-3 top-3">
            <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
              <polyline points="20 6 9 17 4 12" />
            </svg>
          </KobalteSelect.ItemIndicator>
        </KobalteSelect.Item>
      )}
    >
      <KobalteSelect.Trigger
        class={cn(
          "flex h-[38px] w-full items-center justify-between",
          "border border-input bg-background px-3 py-2 text-sm text-foreground",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background",
          "disabled:cursor-not-allowed disabled:opacity-50",
          local.class,
        )}
      >
        <KobalteSelect.Value<SelectOption>>
          {(state) => state.selectedOption()?.label ?? local.placeholder}
        </KobalteSelect.Value>
        <KobalteSelect.Icon class="ml-2 text-muted-foreground">
          <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
            <polyline points="6 9 12 15 18 9" />
          </svg>
        </KobalteSelect.Icon>
      </KobalteSelect.Trigger>

      <KobalteSelect.Portal>
        <KobalteSelect.Content
          class={cn(
            "z-50 min-w-[var(--kb-popper-anchor-width)] overflow-hidden",
            "border border-border bg-popover text-popover-foreground shadow-md",
          )}
        >
          <KobalteSelect.Listbox class="p-1" />
        </KobalteSelect.Content>
      </KobalteSelect.Portal>
    </KobalteSelect>
  );
};

export default Select;
export type { SelectOption };
