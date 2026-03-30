import { type Component, createMemo, createSignal, Show } from "solid-js";
import Button from "@/components/ui/button";
import Input from "@/components/ui/input";
import Label from "@/components/ui/label";
import { createProject, listProjects } from "@/lib/api";

function deriveNameFromUrl(url: string): string {
  const trimmed = url.trim().replace(/\/+$/, "").replace(/\.git$/, "");
  const last = trimmed.split("/").pop() ?? "";
  return last;
}

const NewProjectPage: Component = () => {
  const [repoUrl, setRepoUrl] = createSignal("");
  const [nameOverride, setNameOverride] = createSignal("");
  const [branch, setBranch] = createSignal("main");
  const [sandboxImage, setSandboxImage] = createSignal("");
  const [error, setError] = createSignal("");
  const [saving, setSaving] = createSignal(false);
  const [hasProjects, setHasProjects] = createSignal(false);

  // Check if there are existing projects for the back link
  listProjects()
    .then((p) => setHasProjects(p.length > 0))
    .catch(() => {});

  const derivedName = createMemo(() => deriveNameFromUrl(repoUrl()));
  const effectiveName = () => nameOverride().trim() || derivedName();

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    const url = repoUrl().trim();
    if (!url) return;

    setError("");
    setSaving(true);

    try {
      const project = await createProject({
        name: effectiveName(),
        repo_url: url,
        branch: branch().trim() || "main",
        sandbox_image: sandboxImage().trim() || undefined,
      });
      window.location.hash = `#project/${project.id}`;
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create project");
    } finally {
      setSaving(false);
    }
  };

  return (
    <div class="flex items-center justify-center min-h-screen bg-background">
      <div class="w-full max-w-sm px-6">
        <div class="flex items-center justify-between mb-6">
          <div>
            <h1 class="font-display text-2xl text-foreground mb-1">New Project</h1>
            <p class="text-sm text-muted-foreground">
              Point Hirsel at a repository to get started.
            </p>
          </div>
          <Show when={hasProjects()}>
            <a
              href="#"
              class="text-xs text-muted-foreground hover:text-foreground transition-colors"
            >
              Back
            </a>
          </Show>
        </div>

        <form onSubmit={handleSubmit} class="space-y-4">
          <div class="space-y-1.5">
            <Label for="repo-url">Repository URL *</Label>
            <Input
              id="repo-url"
              type="text"
              placeholder="https://github.com/org/repo.git"
              value={repoUrl()}
              onInput={(e) => setRepoUrl(e.currentTarget.value)}
              autofocus
            />
          </div>

          <div class="space-y-1.5">
            <Label for="name">Name</Label>
            <Input
              id="name"
              type="text"
              placeholder={derivedName() || "project-name"}
              value={nameOverride()}
              onInput={(e) => setNameOverride(e.currentTarget.value)}
            />
            <Show when={!nameOverride().trim() && derivedName()}>
              <p class="text-[11px] text-muted-foreground">
                Auto-derived: {derivedName()}
              </p>
            </Show>
          </div>

          <div class="space-y-1.5">
            <Label for="branch">Branch</Label>
            <Input
              id="branch"
              type="text"
              placeholder="main"
              value={branch()}
              onInput={(e) => setBranch(e.currentTarget.value)}
            />
          </div>

          <div class="space-y-1.5">
            <Label for="image">Base Image (optional)</Label>
            <Input
              id="image"
              type="text"
              placeholder="ubuntu:22.04"
              value={sandboxImage()}
              onInput={(e) => setSandboxImage(e.currentTarget.value)}
            />
          </div>

          <Show when={error()}>
            <p class="text-xs text-signal-red">{error()}</p>
          </Show>

          <Button
            variant="primary"
            type="submit"
            class="w-full"
            loading={saving()}
            disabled={!repoUrl().trim()}
          >
            Create Project
          </Button>
        </form>
      </div>
    </div>
  );
};

export default NewProjectPage;
