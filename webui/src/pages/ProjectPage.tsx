import {
  type Component,
  For,
  Show,
  createEffect,
  createSignal,
  on,
  onCleanup,
  onMount,
} from "solid-js";
import { cn } from "@/lib/cn";
import ChatPanel from "@/components/ChatPanel";
import ProjectPreparationScreen from "@/components/ProjectPreparationScreen";
import ThreadSidebar from "@/components/ThreadSidebar";
import ThemeSwitcher from "@/components/ThemeSwitcher";
import {
  type ProjectPageData,
  type ProjectPreparation,
  getProjectPage,
  getProjectPreparation,
  retryProjectPreparation,
  sendChatMessage,
  stopChat,
} from "@/lib/api";

interface ProjectPageProps {
  projectId: number;
}

/* ── Drag-to-resize handle ── */

const ResizeHandle: Component<{ onResize: (deltaX: number) => void }> = (props) => {
  const handlePointerDown = (e: PointerEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const onMove = (me: PointerEvent) => {
      props.onResize(me.clientX - startX);
      // We reset startX each frame for incremental deltas
      (onMove as any)._startX = me.clientX;
    };
    // Use incremental deltas
    let lastX = startX;
    const onMoveIncremental = (me: PointerEvent) => {
      props.onResize(me.clientX - lastX);
      lastX = me.clientX;
    };
    const onUp = () => {
      document.removeEventListener("pointermove", onMoveIncremental);
      document.removeEventListener("pointerup", onUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
    document.addEventListener("pointermove", onMoveIncremental);
    document.addEventListener("pointerup", onUp);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  };

  return (
    <div
      class="shrink-0 w-[5px] cursor-col-resize bg-transparent hover:bg-border/50 active:bg-border transition-colors"
      onPointerDown={handlePointerDown}
    />
  );
};

const SIDEBAR_MIN = 0;
const SIDEBAR_MAX = 360;
const SIDEBAR_SNAP = 120; // Below this, snap to 0
const CANVAS_MIN = 280;
const CANVAS_MAX = 700;

const ProjectPage: Component<ProjectPageProps> = (props) => {
  const [data, setData] = createSignal<ProjectPageData | null>(null);
  const [preparation, setPreparation] = createSignal<ProjectPreparation | null>(null);
  const [error, setError] = createSignal("");
  const [canvasOpen, setCanvasOpen] = createSignal(true);
  const [sidebarWidth, setSidebarWidth] = createSignal(200);
  const [canvasWidth, setCanvasWidth] = createSignal(420);
  const [projectMenuOpen, setProjectMenuOpen] = createSignal(false);
  let projectMenuRef!: HTMLDivElement;
  const [retryingPreparation, setRetryingPreparation] = createSignal(false);
  let refreshTimer: number | undefined;

  const clearRefreshTimer = () => {
    if (refreshTimer !== undefined) {
      window.clearInterval(refreshTimer);
      refreshTimer = undefined;
    }
  };

  const pollProject = async () => {
    try {
      const page = await getProjectPage(props.projectId);
      setData(page);
      setError("");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load project");
    }
  };

  const pollPreparation = async () => {
    try {
      const state = await getProjectPreparation(props.projectId);
      setPreparation(state);
      setError("");
      if (state.status === "done") {
        clearRefreshTimer();
        await pollProject();
        refreshTimer = window.setInterval(() => {
          void pollProject();
        }, 2000);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load project setup");
    }
  };

  const startProjectPolling = () => {
    clearRefreshTimer();
    void pollProject();
    refreshTimer = window.setInterval(() => {
      void pollProject();
    }, 2000);
  };

  const startPreparationPolling = () => {
    clearRefreshTimer();
    void pollPreparation();
    refreshTimer = window.setInterval(() => {
      void pollPreparation();
    }, 350);
  };

  createEffect(
    on(
      () => props.projectId,
      () => {
        setData(null);
        setPreparation(null);
        clearRefreshTimer();
        void (async () => {
          try {
            const state = await getProjectPreparation(props.projectId);
            setPreparation(state);
            setError("");
            if (state.status === "done") {
              startProjectPolling();
              return;
            }
            startPreparationPolling();
          } catch (err) {
            setError(err instanceof Error ? err.message : "Failed to load project");
          }
        })();
        onCleanup(clearRefreshTimer);
      },
    ),
  );

  createEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape" && isRunning()) {
        e.preventDefault();
        void stopChat(props.projectId);
      }
    };
    window.addEventListener("keydown", handler);
    onCleanup(() => window.removeEventListener("keydown", handler));
  });

  const isRunning = () => data()?.activity.has_active_turn ?? false;

  const handleSend = async (content: string) => {
    try {
      await sendChatMessage(props.projectId, content);
      void pollProject();
    } catch (err) {
      console.error("Send failed:", err);
    }
  };

  const handleStop = () => {
    void stopChat(props.projectId);
  };

  const handleRetryPreparation = async () => {
    setRetryingPreparation(true);
    try {
      const state = await retryProjectPreparation(props.projectId);
      setPreparation(state);
      setError("");
      if (state.status === "done") {
        startProjectPolling();
      } else {
        startPreparationPolling();
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to retry preparation");
    } finally {
      setRetryingPreparation(false);
    }
  };

  // Close project menu on outside click
  const handleProjectMenuClickOutside = (e: PointerEvent) => {
    if (projectMenuOpen() && projectMenuRef && !projectMenuRef.contains(e.target as Node)) {
      setProjectMenuOpen(false);
    }
  };
  onMount(() => document.addEventListener("pointerdown", handleProjectMenuClickOutside));
  onCleanup(() => document.removeEventListener("pointerdown", handleProjectMenuClickOutside));

  const hasFocusHtml = () => !!data()?.focus_html;
  const showPreparation = () => {
    const state = preparation();
    return state !== null && state.status !== "done";
  };
  const projectName = () =>
    data()?.project.name ?? preparation()?.project.name ?? `Project ${props.projectId}`;

  return (
    <Show
      when={!showPreparation()}
      fallback={
        <ProjectPreparationScreen
          preparation={preparation()!}
          retrying={retryingPreparation()}
          onRetry={handleRetryPreparation}
        />
      }
    >
      <div class="h-screen bg-background overflow-hidden">
        <header class="flex h-[46px] items-center gap-3 border-b border-border bg-card px-4">
          <a href="#" class="font-display text-base tracking-tight text-foreground">
            HIRSEL
          </a>

          <div ref={projectMenuRef} class="relative flex items-center gap-1.5">
            <span class="text-xs text-muted-foreground">/</span>
            <button
              class="flex items-center gap-1 max-w-[200px] text-sm font-medium text-foreground hover:text-foreground/80 transition-colors"
              onClick={() => setProjectMenuOpen((v) => !v)}
            >
              <span class="truncate">{projectName()}</span>
              <svg class="h-3 w-3 shrink-0 text-muted-foreground" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <polyline points="6 9 12 15 18 9" />
              </svg>
            </button>
            <Show when={projectMenuOpen()}>
              <div class="absolute left-0 top-full mt-1 z-50 min-w-[200px] border border-border bg-popover text-popover-foreground shadow-md">
                <For each={data()?.projects ?? []}>
                  {(p) => (
                    <button
                      class={cn(
                        "flex w-full items-center gap-2 px-3 py-2 text-xs font-body hover:bg-accent transition-colors text-left",
                        p.id === props.projectId ? "text-foreground font-medium" : "text-muted-foreground",
                      )}
                      onClick={() => {
                        window.location.hash = `#project/${p.id}`;
                        setProjectMenuOpen(false);
                      }}
                    >
                      <span class={cn("h-1.5 w-1.5 rounded-full shrink-0", p.id === props.projectId ? "bg-signal-amber" : "bg-border")} />
                      {p.name}
                    </button>
                  )}
                </For>
                <div class="border-t border-border">
                  <a
                    href="#new"
                    class="flex w-full items-center gap-2 px-3 py-2 text-xs font-body text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
                  >
                    <span class="h-1.5 w-1.5 shrink-0 text-center font-mono text-[10px] leading-none">+</span>
                    New project
                  </a>
                </div>
              </div>
            </Show>
          </div>

          <div class="ml-auto flex items-center gap-1">
            <ThemeSwitcher />
            <a
              href="#settings"
              class="inline-flex items-center justify-center h-[34px] w-[34px] text-muted-foreground hover:text-foreground transition-colors border border-border bg-background hover:bg-accent"
              title="Settings"
            >
              <svg class="h-[15px] w-[15px]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
                <circle cx="12" cy="12" r="3" />
              </svg>
            </a>
          </div>
        </header>

        <Show when={error()}>
          <div class="border-b border-border bg-signal-red/10 px-4 py-2 text-xs text-signal-red">
            {error()}
          </div>
        </Show>

        <Show
          when={data()}
          fallback={
            <div class="flex" style={{ height: "calc(100vh - 46px)" }}>
              <div class="w-[220px] shrink-0 border-r border-border bg-card">
                <div class="px-3 py-3 border-b border-border">
                  <div class="h-3 w-16 bg-muted animate-pulse" />
                </div>
                <div class="p-3 space-y-3">
                  <div class="h-4 w-full bg-muted animate-pulse" />
                  <div class="h-4 w-3/4 bg-muted animate-pulse" />
                  <div class="h-4 w-5/6 bg-muted animate-pulse" />
                </div>
              </div>
              <div class="flex-1 flex items-center justify-center text-sm text-muted-foreground">
                Loading project...
              </div>
            </div>
          }
        >
          <div class="flex overflow-hidden" style={{ height: "calc(100vh - 46px)" }}>
              {/* Thread sidebar */}
              <Show when={sidebarWidth() > 0}>
                <div class="shrink-0 h-full overflow-hidden" style={{ width: `${sidebarWidth()}px` }}>
                  <ThreadSidebar
                    threads={data()?.threads ?? []}
                    projectId={props.projectId}
                    activeThreadId={undefined}
                  />
                </div>
              </Show>
              <ResizeHandle
                onResize={(dx) => setSidebarWidth((w) => {
                  const next = w + dx;
                  if (next < SIDEBAR_SNAP) return 0;
                  return Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_SNAP, next));
                })}
              />

              {/* Chat */}
              <div class="flex-1 min-w-0 h-full overflow-hidden">
                <ChatPanel
                  messages={data()?.history ?? []}
                  liveTurn={data()?.activity.live_turn ?? null}
                  isRunning={isRunning()}
                  onSend={handleSend}
                  onStop={handleStop}
                />
              </div>

              {/* Canvas */}
              <Show when={canvasOpen()}>
                <ResizeHandle
                  onResize={(dx) => setCanvasWidth((w) => Math.min(CANVAS_MAX, Math.max(CANVAS_MIN, w - dx)))}
                />
                <div class="shrink-0 h-full flex flex-col bg-card overflow-hidden" style={{ width: `${canvasWidth()}px` }}>
                  <div class="flex-1 overflow-y-auto">
                    <Show
                      when={hasFocusHtml()}
                      fallback={
                        <div class="px-4 py-6 text-xs text-muted-foreground text-center">
                          Canvas is empty. Shepherd will populate it as the project develops.
                        </div>
                      }
                    >
                      <div
                        class="px-4 py-3 text-sm"
                        innerHTML={data()?.focus_html ?? ""}
                      />
                    </Show>
                  </div>
                </div>
              </Show>
          </div>
        </Show>
      </div>
    </Show>
  );
};

export default ProjectPage;
