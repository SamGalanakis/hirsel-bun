import {
  type Component,
  Show,
  createEffect,
  createMemo,
  createSignal,
  on,
  onCleanup,
  onMount,
} from "solid-js";
import Badge from "@/components/ui/badge";
import Button from "@/components/ui/button";
import Input from "@/components/ui/input";
import Label from "@/components/ui/label";
import {
  createProject,
  listProjects,
  probeProjectCreate,
  type ProjectCreateProbe,
} from "@/lib/api";

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
      () => [repoUrl().trim(), branch().trim()] as const,
      ([nextRepoUrl, nextBranch]) => {
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
          void runProbe(nextRepoUrl, nextBranch || undefined);
        }, 450);
        onCleanup(() => window.clearTimeout(timer));
      },
    ),
  );

  const effectiveName = createMemo(() => nameOverride().trim() || probe()?.suggested_name || "");
  const effectiveBranch = createMemo(() => branch().trim() || probe()?.selected_branch || "");
  const effectiveImage = createMemo(() => sandboxImage().trim() || probe()?.worker_image || "");
  const canSubmit = createMemo(() => !!repoUrl().trim() && !!probe() && !probing() && !saving());

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
      setError(err instanceof Error ? err.message : "Failed to create project");
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

      <main class="mx-auto max-w-xl px-6 py-10">
        <Show
          when={hasProjects()}
          fallback={
            <div class="mb-10 rounded-sm border border-border bg-card/60 px-5 py-5">
              <div class="flex items-start gap-4">
                <div class="flex h-10 w-10 shrink-0 items-center justify-center border border-border bg-background text-signal-amber">
                  <span class="font-mono text-sm">◎</span>
                </div>
                <div class="space-y-1.5">
                  <h1 class="font-display text-3xl tracking-tight text-foreground">No projects yet</h1>
                  <p class="max-w-lg text-sm leading-6 text-muted-foreground">
                    Create your first project to start a shepherd workspace. Add a repository URL and Hirsel will prepare the rest.
                  </p>
                </div>
              </div>
            </div>
          }
        >
          <div class="mb-8">
            <h1 class="font-display text-3xl tracking-tight text-foreground">Add a project</h1>
          </div>
        </Show>

        <form onSubmit={handleSubmit} class="space-y-5">
          <div class="space-y-1.5">
            <Label for="repo-url">Repository URL</Label>
            <Input
              id="repo-url"
              type="text"
              placeholder="https://github.com/org/repo.git"
              value={repoUrl()}
              onInput={(e) => setRepoUrl(e.currentTarget.value)}
              autofocus
            />
            <Show when={probing()}>
              <p class="text-[11px] text-muted-foreground">Inspecting remote...</p>
            </Show>
          </div>

          <div class="grid gap-4 md:grid-cols-2">
            <div class="space-y-1.5">
              <Label for="name">Project name</Label>
              <Input
                id="name"
                type="text"
                placeholder={probe()?.suggested_name || "project-name"}
                value={nameOverride()}
                onInput={(e) => setNameOverride(e.currentTarget.value)}
              />
              <Show when={probe()?.suggested_name && !nameOverride().trim()}>
                <p class="text-[11px] text-muted-foreground">{probe()!.suggested_name}</p>
              </Show>
            </div>

            <div class="space-y-1.5">
              <Label for="branch">Branch</Label>
              <Input
                id="branch"
                type="text"
                placeholder={probe()?.selected_branch || "auto-detect"}
                value={branch()}
                onInput={(e) => setBranch(e.currentTarget.value)}
              />
              <Show when={effectiveBranch()}>
                <p class="text-[11px] text-muted-foreground">{effectiveBranch()}</p>
              </Show>
            </div>
          </div>

          <div class="space-y-1.5">
            <Label for="image">Worker image</Label>
            <Input
              id="image"
              type="text"
              placeholder={probe()?.worker_image || "auto-detect"}
              value={sandboxImage()}
              onInput={(e) => setSandboxImage(e.currentTarget.value)}
            />
            <Show when={effectiveImage() && !sandboxImage().trim()}>
              <p class="text-[11px] text-muted-foreground">{effectiveImage()}</p>
            </Show>
          </div>

          <Show when={probe()}>
            <div class="flex items-center gap-2 text-xs text-muted-foreground">
              <Badge variant={probe()!.has_root_flake ? "success" : "warning"}>
                {probe()!.has_root_flake ? "Flake found" : "No flake"}
              </Badge>
              <span>
                {probe()!.has_root_flake
                  ? "Ready to start -- threads can run immediately."
                  : "Setup will have shepherd create a flake before the project is marked ready."}
              </span>
            </div>
          </Show>

          <Show when={probeError()}>
            <div class="border border-signal-red/30 bg-signal-red/10 px-4 py-3 text-sm text-signal-red">
              {probeError()}
            </div>
          </Show>

          <Show when={error()}>
            <div class="border border-signal-red/30 bg-signal-red/10 px-4 py-3 text-sm text-signal-red">
              {error()}
            </div>
          </Show>

          <Button variant="primary" type="submit" loading={saving()} disabled={!canSubmit()}>
            Create project
          </Button>
        </form>
      </main>
    </div>
  );
};

export default NewProjectPage;
