import {
  type Component,
  For,
  Show,
  createEffect,
  createMemo,
  createSignal,
  on,
  onCleanup,
  onMount,
} from "solid-js";
import Button from "@/components/ui/button";
import Input from "@/components/ui/input";
import Label from "@/components/ui/label";
import {
  createProject,
  listProjects,
  probeProjectCreate,
  type ProjectCreateProbe,
} from "@/lib/api";
import { ApiError } from "@/lib/api/core";
import { cn } from "@/lib/cn";

const NewProjectPage: Component = () => {
  const [repoUrl, setRepoUrl] = createSignal("");
  const [nameOverride, setNameOverride] = createSignal("");
  const [branch, setBranch] = createSignal("");
  const [sandboxImage, setSandboxImage] = createSignal("");
  const [error, setError] = createSignal("");
  const [saving, setSaving] = createSignal(false);
  const [hasProjects, setHasProjects] = createSignal(false);
  const [probe, setProbe] = createSignal<ProjectCreateProbe | null>(null);
  const [probeError, setProbeError] = createSignal("");
  const [probing, setProbing] = createSignal(false);
  const [showAdvanced, setShowAdvanced] = createSignal(false);
  let probeRequest = 0;

  onMount(() => {
    void listProjects()
      .then((projects) => setHasProjects(projects.length > 0))
      .catch(() => {});
  });

  const runProbe = async (url: string, branchOverride?: string) => {
    const requestId = ++probeRequest;
    setProbing(true);
    setProbeError("");
    try {
      const result = await probeProjectCreate({ repo_url: url, branch: branchOverride });
      if (requestId !== probeRequest) return;
      setProbe(result);
      if (!branch()) setBranch(result.selected_branch);
      setError("");
    } catch (err) {
      if (requestId !== probeRequest) return;
      setProbe(null);
      setProbeError(err instanceof Error ? err.message : "Failed to inspect repository");
    } finally {
      if (requestId === probeRequest) setProbing(false);
    }
  };

  createEffect(
    on(
      () => repoUrl().trim(),
      (nextRepoUrl) => {
        if (!nextRepoUrl) {
          probeRequest += 1;
          setProbe(null); setProbeError(""); setProbing(false);
          return;
        }
        const looksRemote = nextRepoUrl.includes("://") || nextRepoUrl.startsWith("git@") || nextRepoUrl.startsWith("ssh://");
        if (!looksRemote) {
          probeRequest += 1;
          setProbe(null); setProbeError(""); setProbing(false);
          return;
        }
        setProbe(null); setProbeError("");
        const timer = window.setTimeout(() => {
          void runProbe(nextRepoUrl);
        }, 450);
        onCleanup(() => window.clearTimeout(timer));
      },
    ),
  );

  const effectiveName = createMemo(() => nameOverride().trim() || probe()?.suggested_name || "");
  const effectiveBranch = createMemo(() => branch().trim() || probe()?.selected_branch || "");
  const effectiveImage = createMemo(() => sandboxImage().trim() || probe()?.worker_image || "");
  const canSubmit = createMemo(() => !!repoUrl().trim() && !!probe() && !probing() && !saving());
  const branches = createMemo(() => probe()?.branches ?? []);

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    const url = repoUrl().trim();
    if (!url) return;
    if (!probe()) {
      await runProbe(url, branch().trim() || undefined);
      if (!probe()) return;
    }
    setError(""); setSaving(true);
    try {
      const project = await createProject({
        name: effectiveName(),
        repo_url: probe()!.normalized_repo_url,
        branch: effectiveBranch() || undefined,
        sandbox_image: sandboxImage().trim() && sandboxImage().trim() !== probe()!.worker_image
          ? sandboxImage().trim() : undefined,
      });
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
    <div class="min-h-screen bg-background text-foreground">
      <header class="flex h-[54px] shrink-0 items-center justify-between border-b border-border bg-card/95 px-4 backdrop-blur">
        <div class="flex items-center gap-2">
          <a href="#" class="font-display text-base font-semibold tracking-tight text-foreground">HIRSEL</a>
          <span class="text-xs text-muted-foreground">/</span>
          <span class="text-sm font-medium">New project</span>
        </div>
        <Show when={hasProjects()}>
          <a
            href="#"
            class="text-xs text-muted-foreground transition-colors hover:text-foreground"
            onClick={(e) => { e.preventDefault(); history.back(); }}
          >
            Back
          </a>
        </Show>
      </header>

      <main class="mx-auto max-w-lg px-6 py-10">
        <Show
          when={hasProjects()}
          fallback={
            <div class="mb-8">
              <h1 class="font-display text-2xl tracking-tight text-foreground">Create your first project</h1>
              <p class="mt-1 text-sm text-muted-foreground">Add a repository URL and Hirsel will prepare the rest.</p>
            </div>
          }
        >
          <div class="mb-8">
            <h1 class="font-display text-2xl tracking-tight text-foreground">Add a project</h1>
          </div>
        </Show>

        <form onSubmit={handleSubmit} class="space-y-5">
          {/* Repository URL */}
          <div class="space-y-1.5">
            <Label for="repo-url">Repository URL</Label>
            <Input
              id="repo-url"
              type="text"
              placeholder="https://github.com/org/repo"
              value={repoUrl()}
              onInput={(e) => setRepoUrl(e.currentTarget.value)}
              autofocus
            />
            <Show when={probing()}>
              <div class="flex items-center gap-2 text-[11px] text-muted-foreground">
                <span class="h-1.5 w-1.5 rounded-full bg-signal-amber animate-pulse-dot" />
                Inspecting remote...
              </div>
            </Show>
          </div>

          {/* Fields that appear after probe succeeds */}
          <Show when={probe()}>
            <div class="space-y-4 border-t border-border pt-5">
              {/* Project name */}
              <div class="space-y-1.5">
                <Label for="name">Project name</Label>
                <Input
                  id="name"
                  type="text"
                  placeholder={probe()?.suggested_name || "project-name"}
                  value={nameOverride()}
                  onInput={(e) => setNameOverride(e.currentTarget.value)}
                />
              </div>

              {/* Branch dropdown */}
              <div class="space-y-1.5">
                <Label for="branch">Branch</Label>
                <Show
                  when={branches().length > 0}
                  fallback={
                    <Input
                      id="branch"
                      type="text"
                      placeholder="main"
                      value={branch()}
                      onInput={(e) => setBranch(e.currentTarget.value)}
                    />
                  }
                >
                  <div class="relative">
                    <select
                      id="branch"
                      class="z-input w-full appearance-none pr-8"
                      value={branch()}
                      onChange={(e) => setBranch(e.currentTarget.value)}
                    >
                      <For each={branches()}>
                        {(b) => (
                          <option
                            value={b}
                            selected={b === effectiveBranch()}
                          >
                            {b}{b === probe()?.selected_branch ? " (default)" : ""}
                          </option>
                        )}
                      </For>
                    </select>
                    <svg class="pointer-events-none absolute right-2.5 top-1/2 h-3 w-3 -translate-y-1/2 text-muted-foreground" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2">
                      <path d="M4 6l4 4 4-4" />
                    </svg>
                  </div>
                </Show>
              </div>

              {/* Advanced toggle */}
              <button
                type="button"
                class="flex items-center gap-1.5 text-[11px] text-muted-foreground/60 transition-colors hover:text-muted-foreground"
                onClick={() => setShowAdvanced((v) => !v)}
              >
                <svg
                  viewBox="0 0 16 16"
                  class={cn("h-2.5 w-2.5 transition-transform", showAdvanced() && "rotate-90")}
                  fill="none" stroke="currentColor" stroke-width="2"
                >
                  <path d="M6 4l4 4-4 4" />
                </svg>
                Advanced
              </button>

              <Show when={showAdvanced()}>
                <div class="space-y-1.5 pl-4 border-l border-border/50">
                  <Label for="image">Worker image</Label>
                  <Input
                    id="image"
                    type="text"
                    placeholder={probe()?.worker_image || "auto-detect"}
                    value={sandboxImage()}
                    onInput={(e) => setSandboxImage(e.currentTarget.value)}
                  />
                  <p class="text-[11px] text-muted-foreground/50">Docker image for the project sandbox. Leave blank for the default.</p>
                </div>
              </Show>
            </div>
          </Show>

          {/* Error */}
          <Show when={probeError() || error()}>
            <div class="border border-signal-red/30 bg-signal-red/10 px-4 py-3 text-sm text-signal-red">
              {probeError() || error()}
            </div>
          </Show>

          {/* Submit */}
          <Show when={probe()}>
            <Button variant="primary" type="submit" loading={saving()} disabled={!canSubmit()}>
              Create project
            </Button>
          </Show>
        </form>
      </main>
    </div>
  );
};

export default NewProjectPage;
