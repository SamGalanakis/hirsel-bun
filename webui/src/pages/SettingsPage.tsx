import { type Component } from "solid-js";
import SettingsForm from "@/components/SettingsForm";

const SettingsPage: Component = () => {
  return (
    <div class="workspace-shell relative flex min-h-screen flex-col bg-background text-foreground">
      {/* Header — same instrument panel as workspace */}
      <header class="relative z-20 flex h-12 shrink-0 items-center gap-4 border-b border-border/40 bg-background px-4">
        <a href="#" class="group/brand flex items-center gap-2 select-none">
          <span class="font-display text-[15px] font-medium tracking-[0.04em] text-foreground transition-colors group-hover/brand:text-brand">
            HIRSEL
          </span>
          <span class="font-mono text-[9px] tabular-nums text-muted-foreground/30">v0.4</span>
        </a>
        <span class="h-5 w-px bg-border/50" aria-hidden="true" />
        <span class="font-mono text-[10px] uppercase tracking-[0.14em] text-brand/80">
          Settings
        </span>

        <a
          href="#"
          class="ml-auto inline-flex h-7 w-7 items-center justify-center text-muted-foreground/40 transition-colors hover:text-foreground"
          onClick={(e) => { e.preventDefault(); history.back(); }}
          title="Back"
          aria-label="Back"
        >
          <svg viewBox="0 0 16 16" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
            <path d="M10 12L6 8l4-4" />
          </svg>
        </a>
      </header>

      <main class="relative z-10 flex flex-1 flex-col items-start px-6 py-10 lg:px-12 overflow-auto">
        <div class="mx-auto w-full max-w-2xl">
          <SettingsForm />
        </div>
      </main>
    </div>
  );
};

export default SettingsPage;
