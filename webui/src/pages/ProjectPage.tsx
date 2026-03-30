import {
  type Component,
  createEffect,
  createSignal,
  on,
  onCleanup,
  Show,
} from "solid-js";
import { cn } from "@/lib/cn";
import ChatPanel from "@/components/ChatPanel";
import ThreadSidebar from "@/components/ThreadSidebar";
import ThemeSwitcher from "@/components/ThemeSwitcher";
import {
  type ProjectPageData,
  getProjectPage,
  sendChatMessage,
  stopChat,
} from "@/lib/api";

interface ProjectPageProps {
  projectId: number;
}

const ProjectPage: Component<ProjectPageProps> = (props) => {
  const [data, setData] = createSignal<ProjectPageData | null>(null);
  const [error, setError] = createSignal("");
  const [canvasOpen, setCanvasOpen] = createSignal(true);

  // Poll for updates
  const poll = async () => {
    try {
      const page = await getProjectPage(props.projectId);
      setData(page);
      setError("");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load project");
    }
  };

  // Initial fetch + interval
  createEffect(
    on(
      () => props.projectId,
      () => {
        poll();
        const id = setInterval(poll, 2000);
        onCleanup(() => clearInterval(id));
      },
    ),
  );

  // Escape key handler
  createEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape" && isRunning()) {
        e.preventDefault();
        stopChat(props.projectId);
      }
    };
    window.addEventListener("keydown", handler);
    onCleanup(() => window.removeEventListener("keydown", handler));
  });

  const isRunning = () => {
    const d = data();
    return d?.activity.has_active_turn ?? false;
  };

  const handleSend = async (content: string) => {
    try {
      await sendChatMessage(props.projectId, content);
      // Trigger immediate refresh
      poll();
    } catch (err) {
      console.error("Send failed:", err);
    }
  };

  const handleStop = () => {
    stopChat(props.projectId);
  };

  const hasFocusHtml = () => {
    const d = data();
    return !!d?.focus_html;
  };

  return (
    <div class="flex flex-col h-screen bg-background">
      {/* Titlebar */}
      <header class="flex items-center gap-3 px-4 h-[46px] shrink-0 border-b border-border bg-card">
        <a href="#" class="font-display text-base text-foreground tracking-tight">
          HIRSEL
        </a>

        <Show when={data()}>
          {(d) => (
            <div class="flex items-center gap-1.5">
              <span class="text-muted-foreground text-xs">/</span>
              <span class="text-sm text-foreground font-medium truncate max-w-[200px]">
                {d().project.name}
              </span>
            </div>
          )}
        </Show>

        <div class="ml-auto flex items-center gap-2">
          <ThemeSwitcher />
          <a
            href="#settings"
            class="text-xs text-muted-foreground hover:text-foreground transition-colors"
          >
            Settings
          </a>
        </div>
      </header>

      {/* Error banner */}
      <Show when={error()}>
        <div class="px-4 py-2 bg-signal-red/10 text-signal-red text-xs border-b border-border">
          {error()}
        </div>
      </Show>

      {/* Main layout */}
      <div class="flex flex-1 min-h-0">
        {/* Thread sidebar */}
        <div class="w-[220px] shrink-0">
          <ThreadSidebar
            threads={data()?.threads ?? []}
            projectId={props.projectId}
            activeThreadId={undefined}
          />
        </div>

        {/* Main area */}
        <div class="flex-1 flex flex-col min-w-0">
          {/* Canvas strip */}
          <Show when={hasFocusHtml()}>
            <div
              class={cn(
                "border-b border-border overflow-hidden transition-all",
                canvasOpen() ? "max-h-[300px]" : "max-h-0",
              )}
            >
              <div class="flex items-center justify-between px-4 py-1.5 bg-muted/30">
                <div class="flex items-center gap-2">
                  <span class="chassis-label">Canvas</span>
                  <Show when={data()?.focus_source}>
                    <span class="text-[10px] text-muted-foreground font-mono">
                      {data()!.focus_source}
                    </span>
                  </Show>
                </div>
                <button
                  class="text-xs text-muted-foreground hover:text-foreground transition-colors"
                  onClick={() => setCanvasOpen((v) => !v)}
                >
                  {canvasOpen() ? "Collapse" : "Expand"}
                </button>
              </div>
              <div
                class="px-4 py-3 text-sm overflow-auto max-h-[260px]"
                innerHTML={data()?.focus_html ?? ""}
              />
            </div>
          </Show>

          {/* Chat panel */}
          <div class="flex-1 min-h-0">
            <ChatPanel
              messages={data()?.history ?? []}
              liveTurn={data()?.activity.live_turn ?? null}
              isRunning={isRunning()}
              hasQueued={data()?.has_queued ?? false}
              onSend={handleSend}
              onStop={handleStop}
            />
          </div>
        </div>
      </div>
    </div>
  );
};

export default ProjectPage;
