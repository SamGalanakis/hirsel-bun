/**
 * Shared Dropdown component - Basecoat-style select dropdown
 */
import { type Component, type JSX, Show, For, createEffect, createSignal, onCleanup, onMount } from 'solid-js';
import { Portal } from 'solid-js/web';
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
  const [pos, setPos] = createSignal<{ left: number; top: number; width: number }>({
    left: 0,
    top: 0,
    width: 240,
  });
  let containerRef: HTMLDivElement | undefined;
  let buttonRef: HTMLButtonElement | undefined;
  let popoverRef: HTMLDivElement | undefined;

  const close = () => setOpen(false);

  const updatePosition = () => {
    const btn = buttonRef;
    if (!btn) return;
    const rect = btn.getBoundingClientRect();
    const gutter = 8;
    const width = Math.max(120, rect.width);
    let left = rect.left;
    const top = rect.bottom + 6;
    // Keep within viewport horizontally.
    left = Math.max(gutter, Math.min(left, window.innerWidth - width - gutter));
    setPos({ left, top, width });
  };

  // Close on click outside (including when popover is portaled to body).
  onMount(() => {
    const onPointerDown = (e: PointerEvent) => {
      if (!open()) return;
      const t = e.target as Node | null;
      if (!t) return;
      const inTrigger = !!containerRef && containerRef.contains(t);
      const inPopover = !!popoverRef && popoverRef.contains(t);
      if (!inTrigger && !inPopover) close();
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (!open()) return;
      if (e.key === 'Escape') close();
    };
    document.addEventListener('pointerdown', onPointerDown, true);
    document.addEventListener('keydown', onKeyDown);
    onCleanup(() => {
      document.removeEventListener('pointerdown', onPointerDown, true);
      document.removeEventListener('keydown', onKeyDown);
    });
  });

  // Reposition when opened and on viewport changes.
  createEffect(() => {
    if (!open()) return;
    updatePosition();
    // Second frame helps when fonts/layout settle.
    requestAnimationFrame(updatePosition);
  });
  onMount(() => {
    const onResize = () => open() && updatePosition();
    window.addEventListener('resize', onResize);
    window.addEventListener('scroll', onResize, true);
    onCleanup(() => {
      window.removeEventListener('resize', onResize);
      window.removeEventListener('scroll', onResize, true);
    });
  });

  const triggerClass = () => props.triggerClass || 'btn-outline w-full justify-between';
  const panelClass = () =>
    props.panelClass ||
    'absolute z-50 mt-1 w-full bg-popover border border-border rounded-md shadow-md py-1 max-h-60 overflow-auto';

  return (
    <div ref={containerRef} class={`dropdown ${props.class || ''}`}>
      <button
        ref={buttonRef}
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
        <Portal>
          <div
            ref={popoverRef}
            data-popover
            class={panelClass()}
            style={{
              position: 'fixed',
              left: `${pos().left}px`,
              top: `${pos().top}px`,
              width: `${pos().width}px`,
              'z-index': 9999,
            }}
          >
            <div role="listbox" aria-orientation="vertical" aria-multiselectable={props.multiselectable}>
              {props.children(close)}
            </div>
          </div>
        </Portal>
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
