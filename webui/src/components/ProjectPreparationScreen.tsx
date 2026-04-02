import { type Component, For, Show, createMemo } from "solid-js";
import Badge from "@/components/ui/badge";
import Button from "@/components/ui/button";
import Progress from "@/components/ui/progress";
import { cn } from "@/lib/cn";
import type { ProjectPreparation, ProjectPreparationStep } from "@/lib/api";

interface ProjectPreparationScreenProps {
  preparation: ProjectPreparation;
  retrying?: boolean;
  onRetry?: () => void;
}

function formatPercent(progress: number): string {
  return `${Math.round(progress * 100)}%`;
}

function statusLabel(status: string): string {
  switch (status) {
    case "done":
      return "Done";
    case "working":
      return "Working";
    case "failed":
      return "Failed";
    default:
      return "Queued";
  }
}

const badgeVariant = (status: string) => {
  switch (status) {
    case "done":
      return "success" as const;
    case "working":
      return "warning" as const;
    case "failed":
      return "destructive" as const;
    default:
      return "default" as const;
  }
};

function stepTone(status: string): string {
  switch (status) {
    case "done":
      return "border-signal-green/35 bg-signal-green/10";
    case "working":
      return "border-signal-amber/35 bg-signal-amber/10";
    case "failed":
      return "border-signal-red/35 bg-signal-red/10";
    default:
      return "border-border/70 bg-background/45";
  }
}

function progressTone(status: string): string {
  switch (status) {
    case "done":
      return "bg-signal-green";
    case "working":
      return "bg-signal-amber";
    case "failed":
      return "bg-signal-red";
    default:
      return "bg-muted";
  }
}

function stepIcon(id: string) {
  switch (id) {
    case "tavily":
      return (
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
          <circle cx="11" cy="11" r="6.5" />
          <path d="m16 16 4 4" />
        </svg>
      );
    case "workspace":
      return (
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
          <path d="M3 7.5h18" />
          <path d="M5 5h14a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2Z" />
          <path d="M8 12h8" />
        </svg>
      );
    case "flake":
      return (
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
          <path d="M12 3 7.5 7.5 12 12l4.5-4.5L12 3Z" />
          <path d="M7.5 12 3 16.5 7.5 21 12 16.5 7.5 12Z" />
          <path d="m16.5 12-4.5 4.5L16.5 21 21 16.5 16.5 12Z" />
        </svg>
      );
    case "image":
      return (
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
          <rect x="4" y="5" width="16" height="14" rx="2" />
          <path d="M8 9h8" />
          <path d="M8 13h8" />
          <path d="M8 17h5" />
        </svg>
      );
    case "shepherd":
      return (
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
          <path d="M12 4 5 8v4c0 4.2 2.8 7.7 7 8 4.2-.3 7-3.8 7-8V8l-7-4Z" />
          <path d="M9.5 12.5 11 14l3.5-4" />
        </svg>
      );
    case "environment":
      return (
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
          <path d="M4 12h6" />
          <path d="M14 12h6" />
          <path d="m10 8 4 4-4 4" />
          <rect x="3" y="5" width="18" height="14" rx="2" />
        </svg>
      );
    default:
      return (
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
          <circle cx="12" cy="12" r="7" />
        </svg>
      );
  }
}

function setupPathSummary(step: ProjectPreparationStep | undefined): {
  label: string;
  detail: string;
} {
  const detail = step?.detail?.toLowerCase() ?? "";
  if (detail.includes("created `flake.nix`") || detail.includes("will author one")) {
    return {
      label: "Bootstrap Path",
      detail: "Shepherd is authoring the repo flake and then re-entering the real project environment.",
    };
  }
  if (detail.includes("repo flake detected")) {
    return {
      label: "Direct Path",
      detail: "The repository already ships a flake, so setup can move straight into the project environment.",
    };
  }
  return {
    label: "Setup Path",
    detail: "Hirsel is deciding whether it can enter the repo environment directly or needs a bootstrap pass first.",
  };
}

const ProjectPreparationScreen: Component<ProjectPreparationScreenProps> = (props) => {
  const currentStep = createMemo(() =>
    props.preparation.steps.find((step) => step.status === "working") ??
    props.preparation.steps.find((step) => step.status === "failed") ??
    props.preparation.steps[props.preparation.steps.length - 1],
  );

  const currentStepIndex = createMemo(() =>
    Math.max(
      props.preparation.steps.findIndex((step) => step.id === currentStep()?.id),
      0,
    ),
  );

  const completedCount = createMemo(
    () => props.preparation.steps.filter((step) => step.status === "done").length,
  );

  const flakeStep = createMemo(() =>
    props.preparation.steps.find((step) => step.id === "flake"),
  );

  const envStep = createMemo(() =>
    props.preparation.steps.find((step) => step.id === "environment"),
  );

  const setupPath = createMemo(() => setupPathSummary(flakeStep()));

  return (
    <div class="min-h-screen overflow-hidden bg-background text-foreground">
      <div class="pointer-events-none absolute inset-0">
        <div class="absolute inset-0 bg-[radial-gradient(circle_at_top_left,rgba(214,162,70,0.18),transparent_32%),radial-gradient(circle_at_78%_18%,rgba(94,163,255,0.13),transparent_26%),linear-gradient(180deg,rgba(16,17,20,0.98),rgba(11,12,15,1))]" />
        <div class="absolute inset-x-0 top-0 h-72 bg-[linear-gradient(180deg,rgba(255,255,255,0.05),transparent)]" />
        <div class="absolute inset-y-0 left-[8%] w-px bg-gradient-to-b from-transparent via-white/14 to-transparent" />
        <div class="absolute inset-y-0 right-[12%] w-px bg-gradient-to-b from-transparent via-signal-amber/16 to-transparent" />
      </div>

      <header class="relative flex h-[54px] shrink-0 items-center justify-between border-b border-border/80 bg-card/70 px-4 backdrop-blur-xl">
        <div class="flex items-center gap-2">
          <a href="#" class="font-display text-base font-semibold tracking-tight text-foreground">
            HIRSEL
          </a>
          <span class="text-xs text-muted-foreground">/</span>
          <span class="truncate text-sm font-medium">{props.preparation.project.name}</span>
        </div>
        <Badge variant={badgeVariant(props.preparation.status)}>
          {statusLabel(props.preparation.status)}
        </Badge>
      </header>

      <main class="relative mx-auto flex min-h-[calc(100vh-54px)] max-w-7xl items-center px-5 py-8 sm:px-6 lg:px-8">
        <div class="grid w-full gap-6 xl:grid-cols-[minmax(0,1.35fr)_340px]">
          <section class="relative overflow-hidden rounded-[32px] border border-border/80 bg-card/75 shadow-[0_40px_120px_rgba(0,0,0,0.36)] backdrop-blur-xl">
            <div class="absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-white/35 to-transparent" />
            <div class="absolute inset-y-0 right-0 w-[32%] bg-[radial-gradient(circle_at_center,rgba(214,162,70,0.08),transparent_70%)]" />

            <div class="relative p-6 sm:p-8 lg:p-10">
              <div class="grid gap-8 lg:grid-cols-[minmax(0,1fr)_280px]">
                <div>
                  <p class="chassis-label mb-3">Project Bring-Up</p>
                  <h1 class="font-display text-4xl tracking-[-0.04em] text-foreground sm:text-5xl">
                    {props.preparation.headline}
                  </h1>
                  <Show when={props.preparation.detail}>
                    <p class="mt-4 max-w-2xl text-sm leading-7 text-muted-foreground sm:text-base">
                      {props.preparation.detail}
                    </p>
                  </Show>
                </div>

                <div class="rounded-[28px] border border-border/70 bg-background/45 p-5 backdrop-blur">
                  <p class="chassis-label">Current Path</p>
                  <div class="mt-4 flex items-center justify-between gap-4">
                    <div>
                      <p class="font-display text-2xl tracking-tight text-foreground">
                        {setupPath().label}
                      </p>
                      <p class="mt-2 text-sm leading-6 text-muted-foreground">
                        {setupPath().detail}
                      </p>
                    </div>
                    <div class="rounded-full border border-border/60 bg-background/60 px-4 py-3 text-right">
                      <div class="text-[10px] uppercase tracking-[0.22em] text-muted-foreground">
                        Progress
                      </div>
                      <div class="mt-1 font-mono text-2xl text-foreground">
                        {formatPercent(props.preparation.progress)}
                      </div>
                    </div>
                  </div>

                  <div class="mt-6 grid grid-cols-3 gap-2 text-[11px] uppercase tracking-[0.18em] text-muted-foreground">
                    <div class={cn("rounded-full border px-3 py-2 text-center", flakeStep()?.status === "done" && flakeStep()?.detail?.includes("detected") ? "border-signal-green/35 bg-signal-green/12 text-foreground" : "border-border/60 bg-background/55")}>
                      Checkout
                    </div>
                    <div class={cn("rounded-full border px-3 py-2 text-center", flakeStep()?.status === "working" || flakeStep()?.detail?.includes("author") || flakeStep()?.detail?.includes("created") ? "border-signal-amber/35 bg-signal-amber/12 text-foreground" : "border-border/60 bg-background/55")}>
                      Bootstrap
                    </div>
                    <div class={cn("rounded-full border px-3 py-2 text-center", envStep()?.status === "done" ? "border-signal-green/35 bg-signal-green/12 text-foreground" : envStep()?.status === "working" ? "border-signal-amber/35 bg-signal-amber/12 text-foreground" : "border-border/60 bg-background/55")}>
                      Repo Env
                    </div>
                  </div>
                </div>
              </div>

              <div class="mt-8 rounded-[28px] border border-border/70 bg-background/40 p-5 shadow-[inset_0_1px_0_rgba(255,255,255,0.04)]">
                <div class="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
                  <div>
                    <p class="chassis-label">Current Operation</p>
                    <div class="mt-2 flex items-center gap-3">
                      <div class="flex h-11 w-11 items-center justify-center rounded-2xl border border-signal-amber/35 bg-signal-amber/14 text-signal-amber">
                        {stepIcon(currentStep()?.id ?? "current")}
                      </div>
                      <div>
                        <div class="text-xl font-semibold text-foreground">
                          {currentStep()?.label ?? "Preparing project"}
                        </div>
                        <div class="text-sm text-muted-foreground">
                          {currentStep()?.detail ?? "Waiting for the next transition."}
                        </div>
                      </div>
                    </div>
                  </div>

                  <div class="grid grid-cols-2 gap-3 sm:w-[240px]">
                    <div class="rounded-2xl border border-border/60 bg-background/60 px-4 py-3">
                      <div class="text-[10px] uppercase tracking-[0.18em] text-muted-foreground">
                        Completed
                      </div>
                      <div class="mt-1 font-mono text-xl text-foreground">
                        {completedCount()}/{props.preparation.steps.length}
                      </div>
                    </div>
                    <div class="rounded-2xl border border-border/60 bg-background/60 px-4 py-3">
                      <div class="text-[10px] uppercase tracking-[0.18em] text-muted-foreground">
                        Station
                      </div>
                      <div class="mt-1 font-mono text-xl text-foreground">
                        {String(currentStepIndex() + 1).padStart(2, "0")}
                      </div>
                    </div>
                  </div>
                </div>

                <div class="mt-5">
                  <Progress
                    value={Math.max(props.preparation.progress * 100, 2)}
                    indicatorClass={
                      props.preparation.status === "failed"
                        ? "bg-signal-red"
                        : "bg-[linear-gradient(90deg,var(--signal-amber)_0%,#f3d37a_48%,var(--signal-green)_100%)]"
                    }
                  />
                </div>
              </div>

              <div class="mt-8 space-y-4">
                <For each={props.preparation.steps}>
                  {(step, index) => {
                    const progress = () => Math.round((step.progress ?? 0) * 100);
                    const active = () => step.id === currentStep()?.id;

                    return (
                      <div class="grid grid-cols-[52px_minmax(0,1fr)] gap-4">
                        <div class="flex flex-col items-center">
                          <div
                            class={cn(
                              "flex h-12 w-12 items-center justify-center rounded-2xl border text-foreground transition-all",
                              stepTone(step.status),
                              active() && step.status === "working" && "shadow-[0_0_30px_rgba(214,162,70,0.2)]",
                            )}
                          >
                            {stepIcon(step.id)}
                          </div>
                          <Show when={index() < props.preparation.steps.length - 1}>
                            <div
                              class={cn(
                                "mt-2 w-px flex-1 min-h-8 bg-gradient-to-b",
                                step.status === "done"
                                  ? "from-signal-green/65 to-border"
                                  : step.status === "working"
                                    ? "from-signal-amber/65 to-border"
                                    : step.status === "failed"
                                      ? "from-signal-red/65 to-border"
                                      : "from-border to-border/20",
                              )}
                            />
                          </Show>
                        </div>

                        <div
                          class={cn(
                            "rounded-[24px] border p-4 transition-all sm:p-5",
                            stepTone(step.status),
                            active() && "shadow-[0_20px_40px_rgba(0,0,0,0.16)]",
                          )}
                        >
                          <div class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
                            <div class="min-w-0">
                              <div class="flex items-center gap-3">
                                <span class="font-mono text-[11px] uppercase tracking-[0.22em] text-muted-foreground">
                                  {String(index() + 1).padStart(2, "0")}
                                </span>
                                <h2 class="text-sm font-semibold text-foreground sm:text-base">
                                  {step.label}
                                </h2>
                              </div>
                              <p class="mt-3 text-sm leading-6 text-muted-foreground">
                                {step.detail ?? "Waiting for this stage to begin."}
                              </p>
                            </div>

                            <Badge variant={badgeVariant(step.status)}>{statusLabel(step.status)}</Badge>
                          </div>

                          <div class="mt-4">
                            <div class="mb-2 flex items-center justify-between text-[11px] uppercase tracking-[0.16em] text-muted-foreground">
                              <span>Stage progress</span>
                              <span>{progress()}%</span>
                            </div>
                            <Progress
                              class="h-1.5"
                              value={Math.max(progress(), step.status === "pending" ? 0 : 2)}
                              indicatorClass={progressTone(step.status)}
                            />
                          </div>
                        </div>
                      </div>
                    );
                  }}
                </For>
              </div>
            </div>
          </section>

          <aside class="space-y-5">
            <section class="overflow-hidden rounded-[30px] border border-border/80 bg-card/80 p-5 backdrop-blur-xl">
              <p class="chassis-label">Manifest</p>
              <dl class="mt-4 space-y-4 text-sm">
                <div class="grid grid-cols-[92px_minmax(0,1fr)] gap-3">
                  <dt class="text-muted-foreground">Project</dt>
                  <dd class="font-semibold text-foreground">{props.preparation.project.name}</dd>
                </div>
                <div class="grid grid-cols-[92px_minmax(0,1fr)] gap-3">
                  <dt class="text-muted-foreground">Image</dt>
                  <dd class="font-mono text-xs leading-5 text-foreground">
                    {props.preparation.worker_image}
                  </dd>
                </div>
                <div class="grid grid-cols-[92px_minmax(0,1fr)] gap-3">
                  <dt class="text-muted-foreground">Current</dt>
                  <dd class="text-foreground">
                    {currentStep()?.label ?? "Preparing project"}
                  </dd>
                </div>
                <div class="grid grid-cols-[92px_minmax(0,1fr)] gap-3">
                  <dt class="text-muted-foreground">Updated</dt>
                  <dd class="font-mono text-xs text-foreground">
                    {new Date(props.preparation.updated_at).toLocaleTimeString([], {
                      hour: "2-digit",
                      minute: "2-digit",
                      second: "2-digit",
                    })}
                  </dd>
                </div>
              </dl>
            </section>

            <section class="overflow-hidden rounded-[30px] border border-border/80 bg-card/78 p-5 backdrop-blur-xl">
              <p class="chassis-label">Readiness</p>
              <div class="mt-4 space-y-3">
                <div class="rounded-2xl border border-border/70 bg-background/55 px-4 py-3">
                  <div class="text-[10px] uppercase tracking-[0.2em] text-muted-foreground">
                    Repo Flake
                  </div>
                  <div class="mt-1 text-sm text-foreground">
                    {flakeStep()?.detail ?? "Awaiting repo inspection."}
                  </div>
                </div>
                <div class="rounded-2xl border border-border/70 bg-background/55 px-4 py-3">
                  <div class="text-[10px] uppercase tracking-[0.2em] text-muted-foreground">
                    Repo Environment
                  </div>
                  <div class="mt-1 text-sm text-foreground">
                    {envStep()?.detail ?? "Will enter the repo flake before setup completes."}
                  </div>
                </div>
              </div>
            </section>

            <Show when={props.preparation.status === "failed"}>
              <section class="rounded-[30px] border border-signal-red/30 bg-signal-red/10 p-5">
                <p class="font-display text-2xl tracking-tight text-foreground">Recovery</p>
                <p class="mt-3 text-sm leading-6 text-foreground">
                  Preparation stopped before the project became ready. Retry will restart bring-up and rebuild the shepherd workspace cleanly.
                </p>
                <Button
                  class="mt-5"
                  variant="primary"
                  size="sm"
                  loading={props.retrying}
                  onClick={() => props.onRetry?.()}
                >
                  Retry bring-up
                </Button>
              </section>
            </Show>
          </aside>
        </div>
      </main>
    </div>
  );
};

export default ProjectPreparationScreen;
