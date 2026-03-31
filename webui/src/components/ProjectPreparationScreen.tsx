import { type Component, For, Show, createMemo } from "solid-js";
import Badge from "@/components/ui/badge";
import Button from "@/components/ui/button";
import Card, {
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import Progress from "@/components/ui/progress";
import type { ProjectPreparation } from "@/lib/api";

interface ProjectPreparationScreenProps {
  preparation: ProjectPreparation;
  retrying?: boolean;
  onRetry?: () => void;
}

function formatPercent(progress: number): string {
  return `${Math.round(progress * 100)}%`;
}

const stepBarTone = (status: string): string => {
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
};

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

const ProjectPreparationScreen: Component<ProjectPreparationScreenProps> = (props) => {
  const currentStep = createMemo(() =>
    props.preparation.steps.find((step) => step.status === "working") ??
    props.preparation.steps.find((step) => step.status === "failed") ??
    props.preparation.steps[props.preparation.steps.length - 1],
  );

  return (
    <div class="min-h-screen bg-background text-foreground">
      <header class="flex items-center justify-between border-b border-border bg-card px-4 py-3">
        <div class="flex items-center gap-2">
          <a href="#" class="font-display text-base tracking-tight">
            HIRSEL
          </a>
          <span class="text-xs text-muted-foreground">/</span>
          <span class="truncate text-sm font-medium">{props.preparation.project.name}</span>
        </div>
        <Badge variant={badgeVariant(props.preparation.status)}>
          {props.preparation.status}
        </Badge>
      </header>

      <main class="mx-auto flex min-h-[calc(100vh-53px)] max-w-6xl items-center px-6 py-10">
        <div class="grid w-full gap-6 lg:grid-cols-[minmax(0,1.25fr)_320px]">
          <Card class="border-border bg-card/85 shadow-lift backdrop-blur-sm">
            <CardHeader class="gap-6 p-7">
              <div class="flex items-start justify-between gap-4">
                <div>
                  <p class="chassis-label mb-3">Runtime Preparation</p>
                  <h1 class="font-display text-4xl tracking-tight text-foreground">
                    {props.preparation.headline}
                  </h1>
                  <Show when={props.preparation.detail}>
                    <p class="mt-3 max-w-2xl text-base text-muted-foreground">
                      {props.preparation.detail}
                    </p>
                  </Show>
                </div>
                <div class="text-right">
                  <div class="chassis-label mb-2">Progress</div>
                  <div class="text-2xl font-mono text-foreground">
                    {formatPercent(props.preparation.progress)}
                  </div>
                </div>
              </div>

              <div>
                <Progress
                  value={Math.max(props.preparation.progress * 100, 2)}
                  indicatorClass={
                    props.preparation.status === "failed"
                      ? "bg-signal-red"
                      : "bg-gradient-to-r from-signal-amber via-signal-amber to-signal-green"
                  }
                />
                <Show when={currentStep()}>
                  {(step) => (
                    <div class="mt-3 flex items-center justify-between gap-4 text-xs">
                      <span class="font-mono uppercase tracking-[0.12em] text-muted-foreground">
                        {step().label}
                      </span>
                      <span class="text-muted-foreground">
                        {step().detail ?? "Waiting for the next step."}
                      </span>
                    </div>
                  )}
                </Show>
              </div>
            </CardHeader>

            <CardContent class="space-y-4 px-7 pb-7">
              <For each={props.preparation.steps}>
                {(step) => {
                  const progress = () => Math.round((step.progress ?? 0) * 100);

                  return (
                    <Card class="bg-background/60 shadow-none">
                      <CardContent class="p-4">
                        <div class="mb-3 flex items-center justify-between gap-3">
                          <div>
                            <div class="text-sm font-medium text-foreground">{step.label}</div>
                            <Show when={step.detail}>
                              <p class="mt-1 text-xs text-muted-foreground">{step.detail}</p>
                            </Show>
                          </div>
                          <Badge variant={badgeVariant(step.status)}>{step.status}</Badge>
                        </div>
                        <Progress
                          class="h-1.5"
                          value={Math.max(progress(), step.status === "pending" ? 0 : 2)}
                          indicatorClass={stepBarTone(step.status)}
                        />
                      </CardContent>
                    </Card>
                  );
                }}
              </For>
            </CardContent>
          </Card>

          <aside class="space-y-5">
            <Card class="bg-card/80">
              <CardHeader class="p-5 pb-3">
                <p class="chassis-label">Details</p>
              </CardHeader>
              <CardContent class="p-5 pt-0">
                <dl class="space-y-3 text-sm">
                  <div class="grid grid-cols-[92px_minmax(0,1fr)] gap-3">
                    <dt class="text-muted-foreground">Project</dt>
                    <dd class="font-medium text-foreground">{props.preparation.project.name}</dd>
                  </div>
                  <div class="grid grid-cols-[92px_minmax(0,1fr)] gap-3">
                    <dt class="text-muted-foreground">Image</dt>
                    <dd class="font-mono text-xs text-foreground">
                      {props.preparation.worker_image}
                    </dd>
                  </div>
                </dl>
              </CardContent>
            </Card>

            <Show when={props.preparation.status === "failed"}>
              <Card class="border-signal-red/30 bg-signal-red/10">
                <CardContent class="p-5">
                  <p class="mb-4 text-sm leading-6 text-foreground">
                    Preparation failed. Check the runtime configuration and retry.
                  </p>
                  <Button
                    variant="primary"
                    size="sm"
                    loading={props.retrying}
                    onClick={() => props.onRetry?.()}
                  >
                    Retry
                  </Button>
                </CardContent>
              </Card>
            </Show>
          </aside>
        </div>
      </main>
    </div>
  );
};

export default ProjectPreparationScreen;
