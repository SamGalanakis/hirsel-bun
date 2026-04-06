import { type Component, For, Show, createMemo } from "solid-js";
import Badge from "@/components/ui/badge";
import Button from "@/components/ui/button";
import Progress from "@/components/ui/progress";
import { cn } from "@/lib/cn";
import type { ProjectPreparation, ProjectPreparationStatus } from "@/lib/api";

interface ProjectPreparationScreenProps {
  preparation: ProjectPreparation;
  retrying?: boolean;
  onRetry?: () => void;
}

function formatPercent(progress: number): string {
  return `${Math.round(progress * 100)}%`;
}

function statusLabel(status: ProjectPreparationStatus): string {
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

const badgeVariant = (status: ProjectPreparationStatus) => {
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

function stepTone(status: ProjectPreparationStatus): string {
  switch (status) {
    case "done":
      return "border-signal-green/35 bg-signal-green/8";
    case "working":
      return "border-signal-amber/35 bg-signal-amber/8";
    case "failed":
      return "border-signal-red/35 bg-signal-red/8";
    default:
      return "border-border bg-card/50";
  }
}

function progressTone(status: ProjectPreparationStatus): string {
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

const ProjectPreparationScreen: Component<ProjectPreparationScreenProps> = (props) => {
  const currentStep = createMemo(() =>
    props.preparation.steps.find((step) => step.id === props.preparation.current_step_id) ??
    props.preparation.steps.find((step) => step.status === "working") ??
    props.preparation.steps.find((step) => step.status === "failed") ??
    props.preparation.steps[props.preparation.steps.length - 1],
  );

  const completedCount = createMemo(
    () => props.preparation.steps.filter((step) => step.status === "done").length,
  );

  return (
    <div class="flex min-h-0 flex-1 flex-col overflow-hidden bg-background text-foreground">
      <header class="flex h-[54px] shrink-0 items-center justify-between border-b border-border bg-card/95 px-4 backdrop-blur">
        <div class="flex items-center gap-2">
          <a href="#" class="font-display text-lg font-semibold tracking-tight text-foreground">
            HIRSEL
          </a>
          <span class="text-xs text-muted-foreground">/</span>
          <span class="truncate text-sm font-medium">{props.preparation.project.name}</span>
        </div>
        <Badge variant={badgeVariant(props.preparation.status)}>
          {statusLabel(props.preparation.status)}
        </Badge>
      </header>

      <div class="flex-1 overflow-y-auto chassis-scroll">
        <main class="mx-auto max-w-3xl px-4 py-8">
          <div class="mb-6">
            <p class="chassis-label mb-2">Project Bring-Up</p>
            <h1 class="font-display text-3xl tracking-tight text-foreground">
              {props.preparation.headline}
            </h1>
            <Show when={props.preparation.detail}>
              <p class="mt-3 max-w-2xl text-sm leading-6 text-muted-foreground">
                {props.preparation.detail}
              </p>
            </Show>
          </div>

          <div class="mb-6 border border-border bg-card p-4">
            <div class="flex items-center gap-4">
              <div class="flex-1">
                <div class="flex items-center gap-3">
                  <Show when={currentStep()}>
                    <div class={cn(
                      "flex h-9 w-9 items-center justify-center border",
                      stepTone(currentStep()!.status),
                    )}>
                      <span class="h-5 w-5">{stepIcon(currentStep()!.id)}</span>
                    </div>
                  </Show>
                  <div>
                    <div class="text-sm font-semibold text-foreground">
                      {currentStep()?.label ?? "Preparing"}
                    </div>
                    <div class="text-xs text-muted-foreground">
                      {currentStep()?.detail ?? "Waiting for the next step."}
                    </div>
                  </div>
                </div>
              </div>
              <div class="flex items-center gap-4 text-right">
                <div>
                  <div class="chassis-label">Done</div>
                  <div class="font-mono text-lg text-foreground">
                    {completedCount()}/{props.preparation.steps.length}
                  </div>
                </div>
                <div>
                  <div class="chassis-label">Progress</div>
                  <div class="font-mono text-lg text-foreground">
                    {formatPercent(props.preparation.progress)}
                  </div>
                </div>
              </div>
            </div>
            <div class="mt-4">
              <Progress
                value={Math.max(props.preparation.progress * 100, 2)}
                indicatorClass={
                  props.preparation.status === "failed"
                    ? "bg-signal-red"
                    : "bg-signal-amber"
                }
              />
            </div>
          </div>

          <Show when={props.preparation.status === "failed"}>
            <div class="mb-6 border border-signal-red/30 bg-signal-red/8 p-4">
              <p class="text-sm font-medium text-foreground">
                Preparation stopped before the project became ready.
              </p>
              <p class="mt-1 text-xs text-muted-foreground">
                Retry will restart bring-up and rebuild the workspace cleanly.
              </p>
              <Button
                class="mt-3"
                variant="primary"
                size="sm"
                loading={props.retrying}
                onClick={() => props.onRetry?.()}
              >
                Retry bring-up
              </Button>
            </div>
          </Show>

          <div class="space-y-0">
            <For each={props.preparation.steps}>
              {(step, index) => {
                const progress = () => Math.round((step.progress ?? 0) * 100);
                const active = () => step.id === currentStep()?.id;

                return (
                  <div class="grid grid-cols-[36px_minmax(0,1fr)] gap-3">
                    <div class="flex flex-col items-center">
                      <div
                        class={cn(
                          "flex h-9 w-9 items-center justify-center border text-foreground transition-colors",
                          stepTone(step.status),
                        )}
                      >
                        <span class="h-5 w-5">{stepIcon(step.id)}</span>
                      </div>
                      <Show when={index() < props.preparation.steps.length - 1}>
                        <div
                          class={cn(
                            "mt-1 w-px flex-1 min-h-4",
                            step.status === "done"
                              ? "bg-signal-green/40"
                              : step.status === "working"
                                ? "bg-signal-amber/40"
                                : step.status === "failed"
                                  ? "bg-signal-red/40"
                                  : "bg-border",
                          )}
                        />
                      </Show>
                    </div>

                    <div
                      class={cn(
                        "mb-2 border p-3 transition-colors",
                        stepTone(step.status),
                        active() && "shadow-sm",
                      )}
                    >
                      <div class="flex items-start justify-between gap-3">
                        <div class="min-w-0">
                          <div class="flex items-center gap-2">
                            <span class="font-mono text-[11px] text-muted-foreground">
                              {String(index() + 1).padStart(2, "0")}
                            </span>
                            <h2 class="text-sm font-medium text-foreground">
                              {step.label}
                            </h2>
                          </div>
                          <p class="mt-1 text-xs leading-5 text-muted-foreground">
                            {step.detail ?? "Waiting for this stage to begin."}
                          </p>
                        </div>
                        <Badge variant={badgeVariant(step.status)}>{statusLabel(step.status)}</Badge>
                      </div>
                      <Show when={step.status !== "pending"}>
                        <div class="mt-2">
                          <Progress
                            class="h-1"
                            value={Math.max(progress(), step.status === "pending" ? 0 : 2)}
                            indicatorClass={progressTone(step.status)}
                          />
                        </div>
                      </Show>
                    </div>
                  </div>
                );
              }}
            </For>
          </div>
        </main>
      </div>
    </div>
  );
};

export default ProjectPreparationScreen;
