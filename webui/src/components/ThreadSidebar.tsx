import { type Component, For, Show } from "solid-js";
import { cn } from "@/lib/cn";
import type { ThreadPanelState } from "@/lib/api";

interface ThreadSidebarProps {
  threads: ThreadPanelState[];
  projectId: number;
  activeThreadId: string | undefined;
}

function statusColor(status: string): string {
  switch (status) {
    case "running":
    case "active":
      return "bg-signal-green";
    case "failed":
    case "error":
      return "bg-signal-red";
    default:
      return "bg-muted-foreground/40";
  }
}

const ThreadSidebar: Component<ThreadSidebarProps> = (props) => {
  return (
    <div class="flex flex-col h-full border-r border-border bg-card">
      {/* Header */}
      <div class="flex items-center justify-between px-3 py-3 border-b border-border">
        <span class="chassis-label">Threads</span>
        <Show when={props.threads.length > 0}>
          <span class="text-[10px] text-muted-foreground font-mono">
            {props.threads.length}
          </span>
        </Show>
      </div>

      {/* Thread list */}
      <div class="flex-1 overflow-y-auto min-h-0">
        <Show
          when={props.threads.length > 0}
          fallback={
            <div class="px-3 py-6 text-xs text-muted-foreground text-center">
              No threads yet
            </div>
          }
        >
          <div class="divide-y divide-border">
            <For each={props.threads}>
              {(tp) => {
                const isActive = () => tp.thread.id === props.activeThreadId;
                const status = () => tp.activity.session?.status ?? tp.thread.status;
                const summary = () =>
                  tp.thread.objective || tp.thread.summary || "";

                return (
                  <a
                    href={`#thread/${props.projectId}/${tp.thread.id}`}
                    class={cn(
                      "block px-3 py-2.5 hover:bg-accent transition-colors cursor-pointer",
                      isActive() && "bg-accent",
                    )}
                  >
                    <div class="flex items-center gap-2 mb-0.5">
                      <span
                        class={cn("h-1.5 w-1.5 rounded-full shrink-0", statusColor(status()))}
                      />
                      <span class="text-xs font-medium text-foreground truncate">
                        {tp.thread.title || "Untitled"}
                      </span>
                    </div>
                    <Show when={summary()}>
                      <p class="text-[11px] text-muted-foreground truncate pl-3.5">
                        {summary()}
                      </p>
                    </Show>
                  </a>
                );
              }}
            </For>
          </div>
        </Show>
      </div>
    </div>
  );
};

export default ThreadSidebar;
