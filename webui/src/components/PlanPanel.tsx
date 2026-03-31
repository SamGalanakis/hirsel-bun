import { type Component, For, Show } from "solid-js";
import { cn } from "@/lib/cn";
import type { PlanSnapshot } from "@/lib/api";

interface PlanPanelProps {
  plan: PlanSnapshot;
}

const STATUS_ICON: Record<string, string> = {
  completed: "\u2713",
  in_progress: "\u25B6",
  pending: "\u25CB",
  skipped: "\u2013",
  failed: "\u2717",
  blocked: "\u25A0",
};

const STATUS_COLOR: Record<string, string> = {
  completed: "text-signal-green",
  in_progress: "text-signal-amber",
  pending: "text-muted-foreground/50",
  skipped: "text-muted-foreground/40",
  failed: "text-signal-red",
  blocked: "text-signal-red",
};

const PlanPanel: Component<PlanPanelProps> = (props) => {
  const completed = () => props.plan.plan.filter((s) => s.status === "completed").length;
  const total = () => props.plan.plan.length;
  const pct = () => total() > 0 ? Math.round((completed() / total()) * 100) : 0;

  return (
    <div class="border-b border-border bg-muted/20">
      <div class="flex items-center gap-3 px-4 py-2">
        <span class="chassis-label">Plan</span>
        <span class="font-mono text-[10px] text-muted-foreground">
          {completed()}/{total()}
        </span>
        {/* Tiny inline progress bar */}
        <div class="flex-1 h-1 bg-border max-w-[120px]">
          <div
            class="h-full bg-signal-green transition-all"
            style={{ width: `${pct()}%` }}
          />
        </div>
        <Show when={props.plan.explanation}>
          <span class="ml-auto text-[11px] text-muted-foreground truncate max-w-[50%]">
            {props.plan.explanation}
          </span>
        </Show>
      </div>
      <div class="px-4 pb-2.5 space-y-0">
        <For each={props.plan.plan}>
          {(item) => {
            const icon = STATUS_ICON[item.status] ?? STATUS_ICON.pending;
            const color = STATUS_COLOR[item.status] ?? STATUS_COLOR.pending;
            const isActive = item.status === "in_progress";
            return (
              <div class={cn("flex items-start gap-2 py-[3px]", isActive && "text-foreground")}>
                <span class={cn("font-mono text-[11px] w-3.5 text-center shrink-0 leading-4", color)}>
                  {icon}
                </span>
                <span class={cn(
                  "text-[11px] leading-4",
                  item.status === "completed" && "text-muted-foreground line-through",
                  item.status === "in_progress" && "text-foreground font-medium",
                  item.status === "pending" && "text-muted-foreground",
                  item.status === "skipped" && "text-muted-foreground/50 line-through",
                  item.status === "failed" && "text-signal-red",
                  item.status === "blocked" && "text-signal-red",
                )}>
                  {item.step}
                </span>
              </div>
            );
          }}
        </For>
      </div>
    </div>
  );
};

export default PlanPanel;
