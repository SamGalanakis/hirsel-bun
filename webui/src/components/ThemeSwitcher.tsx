import { type Component, For } from "solid-js";
import { DropdownMenu } from "@kobalte/core/dropdown-menu";
import { cn } from "@/lib/cn";
import { themes, useTheme } from "@/lib/theme";

/** HSL swatch colors extracted from the CSS theme vars. */
const THEME_SWATCHES: Record<string, [string, string, string]> = {
  hirsel:        ["hsl(42 20% 95%)", "hsl(30 8% 10%)", "hsl(36 80% 50%)"],
  "hirsel-dark": ["hsl(40 8% 6%)",  "hsl(40 10% 88%)", "hsl(36 80% 50%)"],
  midnight:      ["hsl(230 25% 7%)", "hsl(210 15% 88%)", "hsl(185 80% 55%)"],
  bone:          ["hsl(38 40% 95%)", "hsl(20 8% 10%)",  "hsl(20 60% 40%)"],
};

const ThemeSwitcher: Component<{ class?: string }> = (props) => {
  const { theme, setTheme } = useTheme();

  return (
    <DropdownMenu>
      <DropdownMenu.Trigger
        class={cn(
          "inline-flex items-center justify-center h-[34px] w-[34px]",
          "text-muted-foreground hover:text-foreground transition-colors",
          "border border-border bg-background hover:bg-accent",
          props.class,
        )}
        aria-label="Switch theme"
      >
        {/* Palette icon — matches figments */}
        <svg class="h-[15px] w-[15px]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <circle cx="13.5" cy="6.5" r=".5" fill="currentColor" />
          <circle cx="17.5" cy="10.5" r=".5" fill="currentColor" />
          <circle cx="8.5" cy="7.5" r=".5" fill="currentColor" />
          <circle cx="6.5" cy="12.5" r=".5" fill="currentColor" />
          <path d="M12 2C6.5 2 2 6.5 2 12s4.5 10 10 10c.926 0 1.648-.746 1.648-1.688 0-.437-.18-.835-.437-1.125-.29-.289-.438-.652-.438-1.125a1.64 1.64 0 0 1 1.668-1.668h1.996c3.051 0 5.555-2.503 5.555-5.554C21.965 6.012 17.461 2 12 2z" />
        </svg>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content class="z-50 min-w-[180px] border border-border bg-popover text-popover-foreground shadow-md">
          <DropdownMenu.RadioGroup
            value={theme()}
            onChange={(value) => setTheme(value as any)}
          >
            <div class="p-1">
              <For each={themes}>
                {(t) => (
                  <DropdownMenu.RadioItem
                    value={t.name}
                    class={cn(
                      "flex w-full items-center gap-3 px-2.5 py-2 text-xs font-body cursor-pointer outline-none transition-colors",
                      "data-[highlighted]:bg-accent data-[highlighted]:text-accent-foreground",
                      "text-muted-foreground data-[checked]:text-foreground data-[checked]:font-medium",
                    )}
                  >
                    <span class="flex h-4 w-4 items-center justify-center border border-border shrink-0 data-[checked]:border-foreground">
                      <DropdownMenu.ItemIndicator>
                        <span class="block h-2 w-2 bg-foreground" />
                      </DropdownMenu.ItemIndicator>
                    </span>
                    <span class="flex-1">{t.label}</span>
                    <div class="flex shrink-0 items-center gap-px">
                      <For each={THEME_SWATCHES[t.name] ?? []}>
                        {(color) => (
                          <span
                            class="h-3.5 w-1.5 border border-border"
                            style={{ background: color }}
                          />
                        )}
                      </For>
                    </div>
                  </DropdownMenu.RadioItem>
                )}
              </For>
            </div>
          </DropdownMenu.RadioGroup>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu>
  );
};

export default ThemeSwitcher;
