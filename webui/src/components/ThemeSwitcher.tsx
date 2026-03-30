import { type Component, createSignal, For, Show } from "solid-js";
import { cn } from "@/lib/cn";
import { themes, useTheme } from "@/lib/theme";

const ThemeSwitcher: Component = () => {
  const { theme, setTheme } = useTheme();
  const [open, setOpen] = createSignal(false);

  const currentLabel = () => themes.find((t) => t.name === theme())?.label ?? theme();

  return (
    <div class="relative">
      <button
        class={cn(
          "inline-flex items-center gap-1.5 px-2.5 py-1.5 text-xs font-body",
          "text-muted-foreground hover:text-foreground transition-colors",
          "border border-border bg-background hover:bg-accent",
        )}
        onClick={() => setOpen((v) => !v)}
      >
        {currentLabel()}
        <svg class="h-3 w-3" viewBox="0 0 12 12" fill="none">
          <path d="M3 5l3 3 3-3" stroke="currentColor" stroke-width="1.5" />
        </svg>
      </button>

      <Show when={open()}>
        <div
          class="absolute right-0 top-full mt-1 z-50 min-w-[140px] border border-border bg-popover text-popover-foreground shadow-md"
          onFocusOut={(e) => {
            if (!e.currentTarget.contains(e.relatedTarget as Node)) setOpen(false);
          }}
        >
          <For each={themes}>
            {(t) => (
              <button
                class={cn(
                  "flex w-full items-center gap-2 px-3 py-2 text-xs font-body hover:bg-accent transition-colors text-left",
                  theme() === t.name && "text-foreground font-medium",
                  theme() !== t.name && "text-muted-foreground",
                )}
                onClick={() => {
                  setTheme(t.name);
                  setOpen(false);
                }}
              >
                <span
                  class={cn(
                    "h-2 w-2 rounded-full",
                    theme() === t.name ? "bg-signal-amber" : "bg-border",
                  )}
                />
                {t.label}
              </button>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
};

export default ThemeSwitcher;
