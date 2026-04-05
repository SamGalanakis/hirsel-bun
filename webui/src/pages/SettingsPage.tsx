import { type Component } from "solid-js";
import SettingsForm from "@/components/SettingsForm";

const SettingsPage: Component = () => {
  return (
    <div class="min-h-screen bg-background text-foreground">
      <header class="flex h-[54px] shrink-0 items-center justify-between border-b border-border bg-card/95 px-4 backdrop-blur">
        <div class="flex items-center gap-2">
          <a href="#" class="font-display text-base font-semibold tracking-tight text-foreground">HIRSEL</a>
          <span class="text-xs text-muted-foreground">/</span>
          <span class="text-sm font-medium">Settings</span>
        </div>
        <a
          href="#"
          class="text-xs text-muted-foreground transition-colors hover:text-foreground"
          onClick={(e) => { e.preventDefault(); history.back(); }}
        >
          Back
        </a>
      </header>

      <main class="mx-auto max-w-xl px-6 py-10">
        <SettingsForm />
      </main>
    </div>
  );
};

export default SettingsPage;
