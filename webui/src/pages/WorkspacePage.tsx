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
import { cn } from "@/lib/cn";
import CanvasSurface from "@/components/CanvasSurface";
import ChatMessage from "@/components/ChatMessage";
import PlanPanel from "@/components/PlanPanel";
import ProjectPreparationScreen from "@/components/ProjectPreparationScreen";
import ThemeSwitcher from "@/components/ThemeSwitcher";
import { DropdownMenu } from "@kobalte/core/dropdown-menu";
import {
  type ProjectPageData,
  type ProjectPreparation,
  type ThreadPageData,
  type ThreadPanelState,
  getProjectPage,
  getProjectPreparation,
  getThreadPage,
  retryProjectPreparation,
  sendChatMessage,
  sendThreadMessage,
  stopChat,
  stopThreadChat,
} from "@/lib/api";

interface WorkspacePageProps {
  projectId: number;
  threadId?: string;
}

const ROOT_CHANNEL_LABEL = "Project Channel";
const MOBILE_MEDIA = "(max-width: 900px)";
const CANVAS_WIDTH_KEY = "hirsel_workspace_canvas_width";
const CANVAS_MIN = 300;

const PROJECT_SUGGESTIONS = [
  "Survey the repository and identify the riskiest areas.",
  "What threads should we create next for this project?",
  "Summarize overall project progress and open work.",
  "Review the current canvas and explain what changed.",
];

const THREAD_SUGGESTIONS = [
  "Summarize the current state of this thread.",
  "What should happen next in this thread?",
  "Review the latest changes for risk and regressions.",
  "Update the plan based on current progress.",
];

function statusDotClass(status: string): string {
  switch (status) {
    case "running":
    case "active":
      return "bg-signal-green";
    case "waiting":
    case "blocked":
      return "bg-signal-amber";
    case "failed":
    case "error":
      return "bg-signal-red";
    case "ready":
      return "bg-signal-blue";
    default:
      return "bg-muted-foreground/40";
  }
}

function statusLabel(status: string): string {
  switch (status) {
    case "running":
    case "active":
      return "running";
    case "waiting":
      return "waiting";
    case "blocked":
      return "blocked";
    case "failed":
    case "error":
      return "failed";
    case "done":
      return "done";
    case "draft":
      return "draft";
    case "ready":
      return "ready";
    default:
      return status || "idle";
  }
}

function focusSourceLabel(value: string | null | undefined): string | null {
  if (!value) return null;
  return value
    .replace(/[_-]+/g, " ")
    .replace(/\b\w/g, (char) => char.toUpperCase());
}

function threadSummary(thread: ThreadPanelState): string {
  return thread.thread.objective || thread.thread.summary || "No summary yet.";
}

function menuIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <path d="M4 7h16" />
      <path d="M4 12h16" />
      <path d="M4 17h16" />
    </svg>
  );
}

function panelIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <rect x="3" y="4" width="18" height="16" rx="1" />
      <path d="M15 4v16" />
    </svg>
  );
}

function settingsIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  );
}

const WorkspacePage: Component<WorkspacePageProps> = (props) => {
  const [projectData, setProjectData] = createSignal<ProjectPageData | null>(null);
  const [threadData, setThreadData] = createSignal<ThreadPageData | null>(null);
  const [preparation, setPreparation] = createSignal<ProjectPreparation | null>(null);
  const [error, setError] = createSignal("");
  const [retryingPreparation, setRetryingPreparation] = createSignal(false);
  const [mobileSidebarOpen, setMobileSidebarOpen] = createSignal(false);
  const [compactViewport, setCompactViewport] = createSignal(
    window.matchMedia(MOBILE_MEDIA).matches,
  );
  const [canvasOpen, setCanvasOpen] = createSignal(window.innerWidth >= 1180);
  const [canvasWidth, setCanvasWidth] = createSignal(
    Math.min(
      Math.floor(window.innerWidth * 0.46),
      Math.max(CANVAS_MIN, Number(localStorage.getItem(CANVAS_WIDTH_KEY)) || 420),
    ),
  );
  const [canvasDragging, setCanvasDragging] = createSignal(false);
  const [canvasFullscreen, setCanvasFullscreen] = createSignal(false);
  const [input, setInput] = createSignal("");
  const [stickToBottom, setStickToBottom] = createSignal(true);
  let refreshTimer: number | undefined;
  let transcriptRef: HTMLDivElement | undefined;
  let inputRef: HTMLTextAreaElement | undefined;

  const clearRefreshTimer = () => {
    if (refreshTimer !== undefined) {
      window.clearInterval(refreshTimer);
      refreshTimer = undefined;
    }
  };

  const pollProject = async (): Promise<boolean> => {
    try {
      const page = await getProjectPage(props.projectId);
      setProjectData(page);
      return true;
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load project");
      return false;
    }
  };

  const pollThread = async (): Promise<boolean> => {
    if (!props.threadId) {
      setThreadData(null);
      return true;
    }
    try {
      const page = await getThreadPage(props.projectId, props.threadId);
      setThreadData(page);
      return true;
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load thread");
      return false;
    }
  };

  const refreshWorkspace = async () => {
    const [projectOk, threadOk] = await Promise.all([pollProject(), pollThread()]);
    if (projectOk && threadOk) {
      setError("");
    }
  };

  const pollPreparation = async () => {
    try {
      const state = await getProjectPreparation(props.projectId);
      setPreparation(state);
      setError("");
      if (state.status === "done") {
        clearRefreshTimer();
        await refreshWorkspace();
        refreshTimer = window.setInterval(() => {
          void refreshWorkspace();
        }, 2000);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load project setup");
    }
  };

  const startPreparationPolling = () => {
    clearRefreshTimer();
    void pollPreparation();
    refreshTimer = window.setInterval(() => {
      void pollPreparation();
    }, 350);
  };

  const startWorkspacePolling = () => {
    clearRefreshTimer();
    void refreshWorkspace();
    refreshTimer = window.setInterval(() => {
      void refreshWorkspace();
    }, 2000);
  };

  createEffect(
    on(
      () => [props.projectId, props.threadId],
      () => {
        setProjectData(null);
        setThreadData(null);
        setPreparation(null);
        setError("");
        setInput("");
        setMobileSidebarOpen(false);
        clearRefreshTimer();

        void (async () => {
          try {
            const state = await getProjectPreparation(props.projectId);
            setPreparation(state);
            if (state.status === "done") {
              startWorkspacePolling();
            } else {
              startPreparationPolling();
            }
          } catch (err) {
            setError(err instanceof Error ? err.message : "Failed to load project");
          }
        })();

        onCleanup(clearRefreshTimer);
      },
    ),
  );

  createEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (event.key === "Escape" && isRunning()) {
        event.preventDefault();
        void handleStop();
      }
    };
    window.addEventListener("keydown", handler);
    onCleanup(() => window.removeEventListener("keydown", handler));
  });

  createEffect(
    on(
      () => [
        activeMessages().length,
        activeMessages().at(-1)?.id ?? null,
        activeLiveTurn()?.chunks_json ?? null,
        activeLiveTurn()?.status ?? null,
      ],
      () => {
        if (!stickToBottom()) return;
        queueMicrotask(() => {
          if (transcriptRef) {
            transcriptRef.scrollTop = transcriptRef.scrollHeight;
          }
        });
      },
    ),
  );

  createEffect(
    on(
      () => props.threadId,
      () => {
        setStickToBottom(true);
        queueMicrotask(() => {
          if (transcriptRef) {
            transcriptRef.scrollTop = transcriptRef.scrollHeight;
          }
        });
      },
    ),
  );

  createEffect(() => {
    if (!canvasOpen()) {
      setCanvasFullscreen(false);
    }
  });

  onMount(() => {
    const media = window.matchMedia(MOBILE_MEDIA);
    const handleMedia = () => {
      const compact = media.matches;
      setCompactViewport(compact);
      if (!compact) {
        setMobileSidebarOpen(false);
      }
      if (compact && canvasOpen()) {
        setCanvasFullscreen(true);
      }
    };

    media.addEventListener("change", handleMedia);
    handleMedia();

    onCleanup(() => {
      media.removeEventListener("change", handleMedia);
    });
  });

  const showPreparation = () => {
    const state = preparation();
    return state !== null && state.status !== "done";
  };

  const projectName = () =>
    projectData()?.project.name ?? preparation()?.project.name ?? `Project ${props.projectId}`;

  const projectDescription = () =>
    projectData()?.project.description?.trim() ||
    "Talk to Shepherd about the project as a whole, then dive into a thread when the work splits.";

  const activeThreadPanel = createMemo(
    () => projectData()?.threads.find((thread) => thread.thread.id === props.threadId) ?? null,
  );

  const activeTitle = () => {
    if (!props.threadId) return ROOT_CHANNEL_LABEL;
    return threadData()?.thread.title || activeThreadPanel()?.thread.title || "Untitled Thread";
  };

  const activeSummary = () => {
    if (!props.threadId) return projectDescription();
    return (
      threadData()?.thread.objective ||
      activeThreadPanel()?.thread.objective ||
      activeThreadPanel()?.thread.summary ||
      "Use this thread for focused execution."
    );
  };

  const activeStatus = () => {
    if (!props.threadId) {
      return projectData()?.activity.has_active_turn ? "running" : "ready";
    }
    return threadData()?.thread.status || activeThreadPanel()?.thread.status || "active";
  };

  const projectScopeStatus = () =>
    projectData()?.activity.has_active_turn ? "running" : "ready";

  const activeMessages = () =>
    props.threadId ? threadData()?.history ?? [] : projectData()?.history ?? [];

  const activeLiveTurn = () =>
    props.threadId
      ? threadData()?.activity.live_turn ?? null
      : projectData()?.activity.live_turn ?? null;

  const isRunning = () =>
    props.threadId
      ? threadData()?.activity.has_active_turn ?? false
      : projectData()?.activity.has_active_turn ?? false;

  const threads = () => projectData()?.threads ?? [];
  const runningThreads = () =>
    threads().filter((thread) => {
      const status = thread.activity.session?.status ?? thread.thread.status;
      return status === "running" || status === "active";
    }).length;

  const suggestions = () => (props.threadId ? THREAD_SUGGESTIONS : PROJECT_SUGGESTIONS);

  const planProgressText = () => {
    const plan = threadData()?.plan?.plan ?? [];
    if (plan.length === 0) return null;
    const completed = plan.filter((item) => item.status === "completed").length;
    return `${completed}/${plan.length}`;
  };

  const focusHtml = () => {
    return projectData()?.focus_html ?? "";
  };
  const focusSource = () => focusSourceLabel(projectData()?.focus_source);

  const handleRetryPreparation = async () => {
    setRetryingPreparation(true);
    try {
      const state = await retryProjectPreparation(props.projectId);
      setPreparation(state);
      setError("");
      if (state.status === "done") {
        startWorkspacePolling();
      } else {
        startPreparationPolling();
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to retry preparation");
    } finally {
      setRetryingPreparation(false);
    }
  };

  const handleStop = async () => {
    if (props.threadId) {
      await stopThreadChat(props.projectId, props.threadId);
    } else {
      await stopChat(props.projectId);
    }
    void refreshWorkspace();
  };

  const handleSubmit = async (event?: Event) => {
    event?.preventDefault();
    const content = input().trim();
    if (!content) return;

    try {
      setStickToBottom(true);
      if (props.threadId) {
        await sendThreadMessage(props.projectId, props.threadId, content);
      } else {
        await sendChatMessage(props.projectId, content);
      }
      setInput("");
      if (inputRef) {
        inputRef.style.height = "auto";
      }
      void refreshWorkspace();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to send message");
    }
  };

  const updateStickinessFromScroll = () => {
    if (!transcriptRef) return;
    const { scrollTop, scrollHeight, clientHeight } = transcriptRef;
    setStickToBottom(scrollHeight - scrollTop - clientHeight < 80);
  };

  const autoGrowInput = () => {
    if (!inputRef) return;
    inputRef.style.height = "auto";
    inputRef.style.height = `${Math.min(inputRef.scrollHeight, 220)}px`;
  };

  const useSuggestion = (suggestion: string) => {
    setInput(suggestion);
    requestAnimationFrame(() => {
      if (!inputRef) return;
      inputRef.focus();
      autoGrowInput();
      inputRef.selectionStart = inputRef.value.length;
      inputRef.selectionEnd = inputRef.value.length;
    });
  };

  const toggleCanvas = () => {
    if (canvasOpen()) {
      setCanvasFullscreen(false);
      setCanvasOpen(false);
      return;
    }
    setCanvasOpen(true);
    if (compactViewport()) {
      setCanvasFullscreen(true);
    }
  };

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
      <div class="workspace-shell relative flex min-h-0 flex-1 flex-col overflow-hidden bg-background">
        <header class="relative z-20 flex h-[54px] shrink-0 items-center gap-3 border-b border-border bg-card/95 px-4 backdrop-blur">
          <button
            type="button"
            class="flex h-8 w-8 items-center justify-center border border-border bg-background text-muted-foreground transition-colors hover:text-foreground md:hidden"
            onClick={() => setMobileSidebarOpen(true)}
            aria-label="Open threads"
            title="Open threads"
          >
            <span class="h-4 w-4">{menuIcon()}</span>
          </button>

          <a href="#" class="font-display text-lg font-semibold tracking-tight text-foreground">
            HIRSEL
          </a>

          <div class="flex min-w-0 items-center gap-1.5">
            <span class="text-xs text-muted-foreground">/</span>
            <DropdownMenu>
              <DropdownMenu.Trigger
                class="flex min-w-0 items-center gap-1.5 text-left text-sm font-medium text-foreground transition-opacity hover:opacity-75"
              >
                <span class="truncate">{projectName()}</span>
                <svg class="h-3 w-3 shrink-0 text-muted-foreground" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <polyline points="6 9 12 15 18 9" />
                </svg>
              </DropdownMenu.Trigger>
              <DropdownMenu.Portal>
                <DropdownMenu.Content class="z-50 min-w-[220px] border border-border bg-card shadow-lift">
                  <div class="p-1">
                    <For each={projectData()?.projects ?? []}>
                      {(project) => (
                        <DropdownMenu.Item
                          class={cn(
                            "flex w-full items-center gap-2 px-3 py-2 text-xs outline-none cursor-pointer transition-colors",
                            "data-[highlighted]:bg-secondary",
                            project.id === props.projectId ? "text-foreground" : "text-muted-foreground",
                          )}
                          onSelect={() => { window.location.hash = `#project/${project.id}`; }}
                        >
                          <span
                            class={cn(
                              "h-1.5 w-1.5 shrink-0 rounded-full",
                              project.id === props.projectId ? "bg-signal-amber" : "bg-border",
                            )}
                          />
                          <span class="truncate">{project.name}</span>
                        </DropdownMenu.Item>
                      )}
                    </For>
                  </div>
                  <DropdownMenu.Separator class="h-px bg-border" />
                  <div class="p-1">
                    <DropdownMenu.Item
                      class="flex items-center gap-2 px-3 py-2 text-xs text-muted-foreground outline-none cursor-pointer transition-colors data-[highlighted]:bg-secondary data-[highlighted]:text-foreground"
                      onSelect={() => { window.location.hash = "#new"; }}
                    >
                      <span class="font-mono text-[11px]">+</span>
                      New project
                    </DropdownMenu.Item>
                  </div>
                </DropdownMenu.Content>
              </DropdownMenu.Portal>
            </DropdownMenu>
          </div>

          <Show when={props.threadId}>
            <span class="hidden text-xs text-muted-foreground md:inline">/</span>
            <span class="hidden max-w-[220px] truncate text-xs text-ink-2 md:inline">
              {activeTitle()}
            </span>
          </Show>

          <div class="ml-auto flex items-center gap-2">
            <ThemeSwitcher />
            <a
              href="#settings"
              class="inline-flex h-[34px] w-[34px] items-center justify-center border border-border bg-background text-muted-foreground transition-colors hover:text-foreground"
              title="Settings"
            >
              <span class="h-[15px] w-[15px]">{settingsIcon()}</span>
            </a>
          </div>
        </header>

        <Show when={error()}>
          <div class="relative z-10 border-b border-border bg-signal-red/10 px-4 py-2 text-xs text-signal-red">
            {error()}
          </div>
        </Show>

        <div class="relative flex flex-1 overflow-hidden">
          <button
            class={cn(
              "absolute inset-0 z-20 transition-opacity duration-200 md:hidden",
              compactViewport() && mobileSidebarOpen()
                ? "bg-foreground/20 backdrop-blur-[2px] opacity-100"
                : "pointer-events-none opacity-0",
            )}
            onClick={() => setMobileSidebarOpen(false)}
            aria-label="Close threads"
          />

          <aside
            class={cn(
              "z-30 flex w-[280px] shrink-0 flex-col overflow-hidden border-r border-border bg-card/92 backdrop-blur transition-transform duration-200",
              compactViewport()
                ? cn(
                    "absolute inset-y-0 left-0 shadow-lift",
                    mobileSidebarOpen() ? "translate-x-0" : "-translate-x-full",
                  )
                : "relative translate-x-0",
            )}
          >
            <Show when={compactViewport()}>
              <div class="flex items-center justify-between border-b border-border px-4 py-2">
                <span class="font-display text-sm font-semibold tracking-tight text-foreground">{projectName()}</span>
                <button
                  type="button"
                  class="flex h-6 w-6 items-center justify-center text-muted-foreground transition-colors hover:text-foreground"
                  onClick={() => setMobileSidebarOpen(false)}
                  aria-label="Close threads"
                >
                  <svg viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="2">
                    <path d="M6 6l12 12" />
                    <path d="M18 6L6 18" />
                  </svg>
                </button>
              </div>
            </Show>

            <div class="flex-1 overflow-y-auto px-2 py-2">

              <a
                href={`#project/${props.projectId}`}
                class={cn(
                  "group block border px-3 py-2.5 transition-colors",
                  !props.threadId
                    ? "border-border bg-background shadow-sm"
                    : "border-transparent hover:border-border hover:bg-background",
                )}
                onClick={() => setMobileSidebarOpen(false)}
              >
                <div class="flex items-center gap-2">
                  <span class={cn("h-1.5 w-1.5 shrink-0 rounded-full", statusDotClass(projectScopeStatus()))} />
                  <span class="truncate text-sm font-medium text-foreground">{ROOT_CHANNEL_LABEL}</span>
                  <span class="ml-auto font-mono text-[11px] text-muted-foreground">
                    {statusLabel(projectScopeStatus())}
                  </span>
                </div>
                <p class="mt-1 truncate pl-[10px] text-[11px] text-muted-foreground">
                  Project-wide coordination, canvas work, and top-level guidance.
                </p>
              </a>

              <div class="mt-2 space-y-1">
                <Show
                  when={threads().length > 0}
                  fallback={
                    <div class="border border-dashed border-border px-3 py-6 text-center text-xs text-muted-foreground">
                      No threads yet. Ask Shepherd to create one when the work needs a focused lane.
                    </div>
                  }
                >
                  <For each={threads()}>
                    {(thread) => {
                      const status = () => thread.activity.session?.status ?? thread.thread.status;
                      const meta = () => {
                        const progress = thread.plan_progress
                          ? `${thread.plan_progress.completed}/${thread.plan_progress.total}`
                          : null;
                        const label = statusLabel(status());
                        return progress ? `${progress} ${label}` : label;
                      };

                      return (
                        <a
                          href={`#thread/${props.projectId}/${thread.thread.id}`}
                          class={cn(
                            "group block border px-3 py-2.5 transition-all",
                            props.threadId === thread.thread.id
                              ? "border-border bg-background shadow-sm"
                              : "border-transparent hover:border-border hover:bg-background hover:shadow-sm active:scale-[0.995]",
                          )}
                          onClick={() => setMobileSidebarOpen(false)}
                        >
                          <div class="flex items-center gap-2">
                            <span class={cn("h-1.5 w-1.5 shrink-0 rounded-full", statusDotClass(status()))} />
                            <span class="truncate text-sm font-medium text-foreground">
                              {thread.thread.title || "Untitled Thread"}
                            </span>
                            <span class="ml-auto font-mono text-[11px] text-muted-foreground">
                              {meta()}
                            </span>
                          </div>
                          <p class="mt-1 truncate pl-[10px] text-[11px] text-muted-foreground">
                            {threadSummary(thread)}
                          </p>
                        </a>
                      );
                    }}
                  </For>
                </Show>
              </div>
            </div>
          </aside>

          <div class="relative flex min-w-0 flex-1 overflow-hidden">
            <main class="relative flex min-w-0 flex-1 flex-col overflow-hidden">
              <div class="relative flex h-10 shrink-0 items-center gap-2 border-b border-border bg-card px-4">
                <span class="truncate text-sm font-medium text-foreground">
                  {activeTitle()}
                </span>
                <span class={cn("h-1.5 w-1.5 shrink-0 rounded-full", statusDotClass(activeStatus()))} />
                <Show when={planProgressText()}>
                  <span class="font-mono text-[11px] text-muted-foreground">
                    {planProgressText()}
                  </span>
                </Show>
                <div class="ml-auto flex shrink-0 items-center gap-1">
                  <button
                    type="button"
                    class={cn(
                      "flex h-7 w-7 items-center justify-center text-muted-foreground transition-colors hover:text-foreground",
                      canvasOpen() && "bg-background text-foreground",
                    )}
                    onClick={toggleCanvas}
                    aria-label={canvasOpen() ? "Close canvas" : "Open canvas"}
                    title={canvasOpen() ? "Close canvas" : "Open canvas"}
                  >
                    <span class="h-4 w-4">{panelIcon()}</span>
                  </button>
                </div>
              </div>

              <Show when={threadData()?.plan}>
                {(plan) => <PlanPanel plan={plan()} />}
              </Show>

              <div
                ref={transcriptRef}
                class="flex-1 overflow-y-auto chassis-scroll"
                role="log"
                aria-live="polite"
                onScroll={updateStickinessFromScroll}
              >
                <div class="mx-auto flex max-w-3xl flex-col gap-3 px-4 py-5">
                  <Show
                    when={
                      (props.threadId ? !!threadData() : !!projectData()) &&
                      (activeMessages().length > 0 || !!activeLiveTurn())
                    }
                    fallback={
                      <Show
                        when={props.threadId ? !!threadData() : !!projectData()}
                        fallback={
                          <div class="flex flex-col gap-3 py-5">
                            <div class="border border-border bg-card px-4 py-3 shadow-sm">
                              <div class="mb-2 flex items-center gap-2">
                                <div class="skeleton h-3 w-12 rounded-none" />
                                <div class="skeleton h-3 w-16 rounded-none" />
                              </div>
                              <div class="space-y-2">
                                <div class="skeleton h-3 w-full rounded-none" />
                                <div class="skeleton h-3 w-4/5 rounded-none" />
                                <div class="skeleton h-3 w-3/5 rounded-none" />
                              </div>
                            </div>
                            <div class="ml-10 border border-border bg-card px-4 py-3 shadow-sm">
                              <div class="mb-2 flex items-center gap-2">
                                <div class="skeleton h-3 w-8 rounded-none" />
                              </div>
                              <div class="space-y-2">
                                <div class="skeleton h-3 w-full rounded-none" />
                                <div class="skeleton h-3 w-2/3 rounded-none" />
                              </div>
                            </div>
                            <div class="border border-border bg-card px-4 py-3 shadow-sm">
                              <div class="mb-2 flex items-center gap-2">
                                <div class="skeleton h-3 w-16 rounded-none" />
                                <div class="skeleton h-3 w-12 rounded-none" />
                              </div>
                              <div class="space-y-2">
                                <div class="skeleton h-3 w-full rounded-none" />
                                <div class="skeleton h-3 w-3/4 rounded-none" />
                              </div>
                            </div>
                          </div>
                        }
                      >
                        <div class="flex flex-col items-center justify-center gap-5 py-24 text-center">
                          <div class="flex h-14 w-14 items-center justify-center border border-border bg-card text-signal-amber shadow-sm">
                            <span class="font-mono text-lg">◎</span>
                          </div>
                          <div class="space-y-2">
                            <h3 class="font-display text-2xl font-semibold tracking-tight text-foreground">
                              {activeTitle()}
                            </h3>
                            <p class="max-w-xl text-sm leading-6 text-muted-foreground">
                              {props.threadId
                                ? "Use this thread for focused execution while the canvas keeps the project-wide HTML view visible."
                                : "This is the studio-style project channel. Coordinate overall work here, then branch into threads when the job becomes specific."}
                            </p>
                          </div>
                          <div class="grid w-full max-w-2xl gap-2 lg:grid-cols-2">
                            <For each={suggestions()}>
                              {(suggestion) => (
                                <button
                                  type="button"
                                  class="border border-border bg-card px-3 py-3 text-left text-xs text-ink-2 transition-all hover:border-signal-amber/25 hover:bg-background hover:shadow-sm"
                                  onClick={() => useSuggestion(suggestion)}
                                >
                                  {suggestion}
                                </button>
                              )}
                            </For>
                          </div>
                        </div>
                      </Show>
                    }
                  >
                    <For each={activeMessages()}>
                      {(message) => (
                        <ChatMessage
                          role={message.role}
                          chunksJson={message.chunks_json}
                          timestamp={message.timestamp}
                        />
                      )}
                    </For>

                    <Show when={activeLiveTurn()}>
                      {(turn) => (
                        <ChatMessage
                          role="assistant"
                          chunksJson={turn().chunks_json}
                          timestamp={turn().updated_at}
                        />
                      )}
                    </Show>
                  </Show>
                </div>
              </div>

              <div class="relative shrink-0 border-t border-border bg-card">
                <div class="absolute top-0 right-0 h-px w-10 bg-gradient-to-l from-signal-amber/30 to-transparent" />
                <div class="mx-auto max-w-3xl px-4 py-3">
                  <form onSubmit={handleSubmit}>
                    <div class="flex items-end gap-1 border border-border bg-background p-1 shadow-sm transition-colors focus-within:border-ring">
                      <textarea
                        ref={inputRef}
                        class="min-h-[42px] max-h-[220px] flex-1 resize-none appearance-none border-0 bg-transparent px-3 py-2 text-sm text-foreground outline-none placeholder:text-muted-foreground"
                        rows={1}
                        value={input()}
                        placeholder={isRunning() ? "Type a follow-up…" : "Message Shepherd…"}
                        onInput={(event) => {
                          setInput(event.currentTarget.value);
                          autoGrowInput();
                        }}
                        onKeyDown={(event) => {
                          if (event.key === "Enter" && !event.shiftKey) {
                            event.preventDefault();
                            void handleSubmit();
                          }
                        }}
                      />
                      <Show
                        when={!isRunning()}
                        fallback={
                          <button
                            type="button"
                            class="flex h-10 w-10 shrink-0 items-center justify-center bg-destructive text-white transition-colors hover:bg-destructive/90"
                            onClick={() => void handleStop()}
                            aria-label="Stop"
                            title="Stop generating"
                          >
                            <svg viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="currentColor">
                              <rect x="6" y="6" width="12" height="12" rx="1" />
                            </svg>
                          </button>
                        }
                      >
                        <button
                          type="submit"
                          class="flex h-10 w-10 shrink-0 items-center justify-center bg-foreground text-background transition-colors hover:bg-foreground/90 disabled:opacity-30"
                          disabled={!input().trim()}
                          aria-label="Send"
                          title="Send"
                        >
                          <svg viewBox="0 0 24 24" class="h-4 w-4" fill="none" stroke="currentColor" stroke-width="2">
                            <path d="M22 2L11 13" />
                            <path d="M22 2L15 22 11 13 2 9z" />
                          </svg>
                        </button>
                      </Show>
                    </div>
                  </form>
                </div>
              </div>
            </main>

            <Show when={canvasOpen()}>
              <Show when={!canvasFullscreen() && !compactViewport()}>
                <div
                  class={cn(
                    "group relative flex w-2 shrink-0 cursor-col-resize items-center justify-center border-l border-border transition-colors hover:border-ring hover:bg-secondary/50",
                    canvasDragging() && "border-ring bg-signal-blue/20",
                  )}
                  onPointerDown={(event) => {
                    event.preventDefault();
                    setCanvasDragging(true);
                    const startX = event.clientX;
                    const startW = canvasWidth();
                    const onMove = (moveEvent: PointerEvent) => {
                      const delta = startX - moveEvent.clientX;
                      const maxW = Math.floor(window.innerWidth * 0.58);
                      const next = Math.max(CANVAS_MIN, Math.min(maxW, startW + delta));
                      setCanvasWidth(next);
                    };
                    const onUp = () => {
                      setCanvasDragging(false);
                      localStorage.setItem(CANVAS_WIDTH_KEY, String(canvasWidth()));
                      window.removeEventListener("pointermove", onMove);
                      window.removeEventListener("pointerup", onUp);
                    };
                    window.addEventListener("pointermove", onMove);
                    window.addEventListener("pointerup", onUp);
                  }}
                >
                  <div class={cn(
                    "h-8 w-px rounded-full transition-opacity",
                    canvasDragging() ? "bg-muted-foreground/60 opacity-100" : "bg-muted-foreground/30 opacity-0 group-hover:opacity-100",
                  )} />
                </div>
              </Show>

              <aside
                class={cn(
                  "flex flex-col border-l border-border bg-card canvas-enter",
                  canvasFullscreen() || compactViewport()
                    ? "absolute inset-0 z-40 shadow-lift"
                    : "shrink-0",
                )}
                style={
                  canvasFullscreen() || compactViewport()
                    ? undefined
                    : { width: `${canvasWidth()}px` }
                }
              >
                <div class="relative flex items-center justify-between border-b border-border bg-card px-3 py-2">
                  <div class="absolute top-0 left-0 h-px w-8 bg-gradient-to-r from-signal-blue/60 to-transparent" />
                  <div class="flex min-w-0 items-center gap-2">
                    <span class="font-mono text-[11px] uppercase tracking-[0.12em] text-signal-blue">
                      HTML Canvas
                    </span>
                    <Show when={focusSource()}>
                      <>
                        <span class="h-3 w-px bg-border" />
                        <span class="truncate font-mono text-[11px] text-muted-foreground">
                          {focusSource()}
                        </span>
                      </>
                    </Show>
                  </div>

                  <div class="flex items-center gap-2">
                    <Show when={!compactViewport()}>
                      <button
                        type="button"
                        class="border border-border px-2 py-1 font-mono text-[11px] uppercase tracking-[0.08em] text-muted-foreground transition-colors hover:text-foreground"
                        onClick={() => setCanvasFullscreen((current) => !current)}
                      >
                        {canvasFullscreen() ? "Windowed" : "Fullscreen"}
                      </button>
                    </Show>
                    <button
                      type="button"
                      class="flex h-6 w-6 items-center justify-center text-muted-foreground transition-colors hover:text-foreground"
                      onClick={() => {
                        setCanvasFullscreen(false);
                        setCanvasOpen(false);
                      }}
                      aria-label="Close canvas"
                      title="Close canvas"
                    >
                      <svg viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="2">
                        <path d="M6 6l12 12" />
                        <path d="M18 6L6 18" />
                      </svg>
                    </button>
                  </div>
                </div>

                <div class="flex-1 overflow-y-auto bg-background">
                  <Show
                    when={focusHtml().trim()}
                    fallback={
                      <div class="flex h-full items-center justify-center p-8 text-center">
                        <div class="max-w-sm space-y-2">
                          <p class="font-display text-xl font-semibold tracking-tight text-foreground">
                            Canvas is empty
                          </p>
                          <p class="text-sm leading-6 text-muted-foreground">
                            Shepherd will populate the HTML canvas as project context evolves.
                          </p>
                        </div>
                      </div>
                    }
                  >
                    <CanvasSurface html={focusHtml()} projectId={props.projectId} />
                  </Show>
                </div>
              </aside>
            </Show>
          </div>
        </div>
      </div>
    </Show>
  );
};

export default WorkspacePage;
