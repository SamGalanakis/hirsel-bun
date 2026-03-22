/**
 * Basecoat-style Switch component for settings forms
 */
import { type Component, Show } from 'solid-js';

export const Switch: Component<{
  id?: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  description?: string;
}> = (props) => {
  return (
    <div role="group" class="field flex items-start justify-between rounded-none border p-4">
      <div class="flex flex-col gap-0.5">
        <label for={props.id} class="font-medium leading-normal">{props.label}</label>
        <Show when={props.description}>
          <p class="text-muted-foreground text-sm">{props.description}</p>
        </Show>
      </div>
      <input
        id={props.id}
        type="checkbox"
        role="switch"
        checked={props.checked}
        onChange={(e) => props.onChange(e.currentTarget.checked)}
      />
    </div>
  );
};
