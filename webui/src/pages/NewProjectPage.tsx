import { type Component, Show, createMemo, createSignal, onMount } from "solid-js";
import Button from "@/components/ui/button";
import Input from "@/components/ui/input";
import Label from "@/components/ui/label";
import { createProject, listProjects } from "@/lib/api";
import { ApiError } from "@/lib/api/core";

const NewProjectPage: Component = () => {
  const [name, setName] = createSignal("");
  const [error, setError] = createSignal("");
  const [saving, setSaving] = createSignal(false);
  const [hasProjects, setHasProjects] = createSignal(false);

  onMount(() => {
    void listProjects()
      .then((projects) => setHasProjects(projects.length > 0))
      .catch(() => {});
  });

  const canSubmit = createMemo(() => !!name().trim() && !saving());

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    const trimmedName = name().trim();
    if (!trimmedName) return;
    setError("");
    setSaving(true);
    try {
      const project = await createProject({ name: trimmedName });
      window.location.hash = `#project/${project.id}`;
    } catch (err) {
      if (err instanceof ApiError && err.status === 409) {
        setError(err.message);
      } else {
        setError(err instanceof Error ? err.message : "Failed to create project");
      }
    } finally {
      setSaving(false);
    }
  };

  return (
    <div class="workspace-shell relative flex min-h-screen flex-col bg-background text-foreground">
      <header class="relative z-20 flex h-12 shrink-0 items-center gap-4 border-b border-border/40 bg-background px-4">
        <a href="#" class="group/brand flex items-center gap-2 select-none">
          <span class="font-display text-[15px] font-medium tracking-[0.04em] text-foreground transition-colors group-hover/brand:text-brand">
            HIRSEL
          </span>
          <span class="font-mono text-[9px] tabular-nums text-muted-foreground/30">v0.4</span>
        </a>
        <span class="h-5 w-px bg-border/50" aria-hidden="true" />
        <span class="font-mono text-[10px] uppercase tracking-[0.14em] text-muted-foreground/40">
          New project
        </span>
        <Show when={hasProjects()}>
          <a
            href="#"
            class="ml-auto inline-flex h-7 items-center gap-1.5 px-2 font-mono text-[10px] uppercase tracking-wider text-muted-foreground transition-colors hover:text-foreground"
            onClick={(e) => { e.preventDefault(); history.back(); }}
          >
            <svg viewBox="0 0 16 16" class="h-3 w-3" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M10 12L6 8l4-4" />
            </svg>
            Back
          </a>
        </Show>
      </header>

      <main class="relative z-10 flex flex-1 items-start overflow-auto px-6 py-12 lg:px-12 lg:py-20">
        <div class="mx-auto w-full max-w-xl">
          {/* Engraved slug */}
          <div class="mb-3 flex items-center gap-2 font-mono text-[10px] uppercase tracking-[0.18em] text-muted-foreground/40">
            <span class="inline-block h-px w-6 bg-muted-foreground/30" />
            <span>{hasProjects() ? "New project" : "Getting started"}</span>
          </div>

          <Show
            when={hasProjects()}
            fallback={
              <>
                <h1 class="font-display text-4xl font-normal tracking-tight text-foreground">
                  Your first project
                </h1>
                <p class="mt-3 max-w-md text-[13px] leading-[1.7] text-muted-foreground">
                  A project is a place to think — it holds your conversations, context, and the workspaces you attach. Start with a name; everything else comes after.
                </p>
              </>
            }
          >
            <h1 class="font-display text-4xl font-normal tracking-tight text-foreground">
              New project
            </h1>
            <p class="mt-3 max-w-md text-[13px] leading-[1.7] text-muted-foreground">
              Name it now. Attach workspaces and context once you're inside.
            </p>
          </Show>

          {/* Separator — thin rule with brand tick */}
          <div class="mt-10 mb-8 flex items-center gap-3">
            <span class="h-px flex-1 bg-border/40" />
            <span class="h-1 w-1 bg-brand" />
            <span class="h-px flex-1 bg-border/40" />
          </div>

          <form onSubmit={handleSubmit} class="space-y-6">
            <div class="space-y-2">
              <Label for="name" class="font-mono text-[10px] uppercase tracking-[0.14em] text-muted-foreground/60">
                Name
              </Label>
              <Input
                id="name"
                type="text"
                placeholder="e.g. studio-renderer"
                value={name()}
                onInput={(e) => setName(e.currentTarget.value)}
                autofocus
              />
            </div>

            <Show when={error()}>
              <div class="border border-signal-red/20 bg-signal-red/[0.05] px-4 py-3 text-sm text-signal-red">
                {error()}
              </div>
            </Show>

            <div class="pt-2">
              <Button variant="primary" type="submit" loading={saving()} disabled={!canSubmit()}>
                Create project
              </Button>
            </div>
          </form>
        </div>
      </main>
    </div>
  );
};

export default NewProjectPage;
