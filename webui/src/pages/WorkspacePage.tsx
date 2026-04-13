import {
  type Component,
  For,
  Show,
  Suspense,
  createEffect,
  createMemo,
  createSignal,
  lazy,
  on,
  onCleanup,
  onMount,
} from "solid-js";
import { cn } from "@/lib/cn";
import ChatComposer from "@/components/ChatComposer";
import ChatTranscript from "@/components/chat/ChatTranscript";
import SettingsForm from "@/components/SettingsForm";
import { matchesAction } from "@/lib/keybindings";
import {
  type ChatMessage as ApiChatMessage,
  ApiError,
  type Project,
  type ProjectSurface,
  type ScopeActivity,
  type LiveTurn,
  type LiveUpdateEvent,
  type SettingsResponse,
  type ThreadDetail,
  type ThreadSummary,
  getSettings,
  getWorkspaceSnapshot,
  listProjects,
  listThreads,
  sendChatMessage,
  sendThreadMessage,
  stopChat,
  stopThreadChat,
  subscribeProjectEvents,
  sendLibrarianMessage,
  stopLibrarianChat,
  deleteProject,
  saveProjectSettings,
  addProjectWorkspace,
  removeProjectWorkspace,
  type ProjectWorkspaceEntry,
} from "@/lib/api";

const CanvasView = lazy(() => import("@/components/CanvasView"));
const TerminalPanel = lazy(() => import("@/components/TerminalPanel"));
const WorkspaceBrowser = lazy(() => import("@/components/WorkspaceBrowser"));

interface WorkspacePageProps {
  projectId: number;
  threadId?: string;
  librarianView?: boolean;
}

const ROOT_CHANNEL_LABEL = "Shepherd";
const MOBILE_MEDIA = "(max-width: 900px)";
const INSPECTOR_WIDTH_KEY = "hirsel_workspace_inspector_width";
const INSPECTOR_MIN = 320;
const INSPECTOR_DEFAULT = 640;
const SIDEBAR_WIDTH_KEY = "hirsel_workspace_sidebar_width";
const SIDEBAR_MIN = 160;
const SIDEBAR_MAX = 360;
const MAIN_MIN = 420;
const SIDEBAR_DEFAULT = 210;
const LIBRARIAN_FULL_SCAN_MESSAGE =
  "Full workspace scan.\n\nExplore the current workspace, refresh the graph, lore, and canvas anywhere they are stale or incomplete, and then reply with a concise summary of what you updated or that everything was already up to date.";

type WorkspaceBannerError = {
  message: string;
  action: "open-settings" | null;
  raw: string;
};

function statusDotClass(status: string): string {
  switch (status) {
    case "starting":
    case "starting_container":
    case "waiting_for_socket":
    case "running":
    case "active":
      return status === "running" || status === "active" ? "bg-signal-green" : "bg-signal-amber";
    case "interrupting":
    case "queued":
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
    case "starting":
      return "starting";
    case "starting_container":
      return "starting runtime";
    case "waiting_for_socket":
      return "waiting for runtime";
    case "missing_artifact":
      return "worker image missing";
    case "running":
    case "active":
      return "running";
    case "interrupting":
      return "stopping";
    case "queued":
      return "queued";
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

function statusPriority(status: string): number {
  switch (status) {
    case "running":
    case "active":
      return 0;
    case "waiting":
    case "blocked":
      return 1;
    case "failed":
    case "error":
      return 2;
    case "ready":
    case "draft":
      return 3;
    case "done":
      return 4;
    default:
      return 5;
  }
}

function focusSourceLabel(value: string | null | undefined): string | null {
  if (!value) return null;
  return value
    .replace(/[_-]+/g, " ")
    .replace(/\b\w/g, (char) => char.toUpperCase());
}

function threadSummary(thread: ThreadSummary): string {
  return thread.thread.objective || thread.thread.summary || "No summary yet.";
}

function classifyWorkspaceError(value: string | null | undefined): WorkspaceBannerError | null {
  const trimmed = value?.trim();
  if (!trimmed) return null;

  const lower = trimmed.toLowerCase();
  if (lower.includes("codex credentials not configured")) {
    return {
      message: "Codex isn't connected yet. Open settings to sign in, or switch to another provider.",
      action: "open-settings",
      raw: trimmed,
    };
  }
  if (lower.includes("openrouter api key not configured")) {
    return {
      message: "OpenRouter needs an API key. Add one in settings, or switch to another provider.",
      action: "open-settings",
      raw: trimmed,
    };
  }
  if (lower.includes("lock is already locked by another process")) {
    return {
      message: "Hirsel's database is locked by another process. Restart Hirsel to continue.",
      action: null,
      raw: trimmed,
    };
  }
  if (lower.includes("rpc connection closed")) {
    return {
      message: "Lost connection to the local agent. Restart Hirsel and try again.",
      action: null,
      raw: trimmed,
    };
  }
  if (lower.includes("returned no user-visible output")
    || lower.includes("model returned no usable output")) {
    return {
      message: "The model finished without producing a response. Try rephrasing your message.",
      action: null,
      raw: trimmed,
    };
  }
  return {
    message: trimmed,
    action: null,
    raw: trimmed,
  };
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

function filesIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
      <path d="M3 7h6l2 2h10v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
      <path d="M3 7V5a2 2 0 0 1 2-2h4l2 2" />
    </svg>
  );
}

function libraryIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
      <circle cx="12" cy="12" r="3" />
      <line x1="12" y1="3" x2="12" y2="9" />
      <line x1="12" y1="15" x2="12" y2="21" />
      <line x1="3" y1="12" x2="9" y2="12" />
      <line x1="15" y1="12" x2="21" y2="12" />
      <line x1="5.6" y1="5.6" x2="8.5" y2="8.5" />
      <line x1="15.5" y1="15.5" x2="18.4" y2="18.4" />
      <line x1="5.6" y1="18.4" x2="8.5" y2="15.5" />
      <line x1="15.5" y1="8.5" x2="18.4" y2="5.6" />
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
  const [projects, setProjects] = createSignal<Project[]>([]);
  const [project, setProject] = createSignal<Project | null>(null);
  const [projectActivity, setProjectActivity] = createSignal<ScopeActivity | null>(null);
  const [projectHistory, setProjectHistory] = createSignal<ApiChatMessage[]>([]);
  const [projectSurface, setProjectSurface] = createSignal<ProjectSurface | null>(null);
  const [threads, setThreads] = createSignal<ThreadSummary[]>([]);
  const [threadDetail, setThreadDetail] = createSignal<ThreadDetail | null>(null);
  const [threadHistory, setThreadHistory] = createSignal<ApiChatMessage[]>([]);
  const [focusedTask, setFocusedTask] = createSignal<import("@/lib/api/types").Task | null>(null);
  const [canvasReloadNonce, setCanvasReloadNonce] = createSignal(0);
  const [librarianActivity, setLibrarianActivity] = createSignal<ScopeActivity | null>(null);
  const [librarianHistory, setLibrarianHistory] = createSignal<ApiChatMessage[]>([]);
  const [error, setError] = createSignal("");
  const [connectionOk, setConnectionOk] = createSignal(true);
  const [mobileSidebarOpen, setMobileSidebarOpen] = createSignal(false);
  const [compactViewport, setCompactViewport] = createSignal(
    window.matchMedia(MOBILE_MEDIA).matches,
  );
  const [inspectorOpen, setInspectorOpen] = createSignal(window.innerWidth >= 1180);
  const [inspectorWidth, setInspectorWidth] = createSignal(
    Math.min(
      Math.floor(window.innerWidth * 0.46),
      window.innerWidth - SIDEBAR_DEFAULT - MAIN_MIN,
      Math.max(INSPECTOR_MIN, Number(localStorage.getItem(INSPECTOR_WIDTH_KEY)) || INSPECTOR_DEFAULT),
    ),
  );
  const [inspectorDragging, setInspectorDragging] = createSignal(false);
  const [inspectorFullscreen, setInspectorFullscreen] = createSignal(false);
  const [inspectorTab, setInspectorTab] = createSignal<"files">("files");
  const [sidebarWidth, setSidebarWidth] = createSignal(
    Math.max(SIDEBAR_MIN, Math.min(SIDEBAR_MAX, Number(localStorage.getItem(SIDEBAR_WIDTH_KEY)) || SIDEBAR_DEFAULT)),
  );
  const [sidebarCollapsed, setSidebarCollapsed] = createSignal(false);
  const [sidebarDragging, setSidebarDragging] = createSignal(false);
  const [settingsOpen, setSettingsOpen] = createSignal(false);
  const [projectSettingsOpen, setProjectSettingsOpen] = createSignal(false);
  const [input, setInput] = createSignal("");
  const [optimisticLiveTurn, setOptimisticLiveTurn] = createSignal<LiveTurn | null>(null);
  const [optimisticTurnStartedAt, setOptimisticTurnStartedAt] = createSignal<string | null>(null);
  const [composerFocusNonce, setComposerFocusNonce] = createSignal(0);
  const [stickToBottom, setStickToBottom] = createSignal(true);
  const [justStopped, setJustStopped] = createSignal(false);
  const [optimisticUserMessage, setOptimisticUserMessage] = createSignal<ApiChatMessage | null>(null);
  const [expandedProjects, setExpandedProjects] = createSignal<Set<number>>(new Set());
  const [otherProjectThreads, setOtherProjectThreads] = createSignal<Map<number, ThreadSummary[]>>(new Map());
  const [dismissedRuntimeErrorRaw, setDismissedRuntimeErrorRaw] = createSignal<string | null>(null);
  const [scanning, setScanning] = createSignal(false);
  const [terminalOpen, setTerminalOpen] = createSignal(false);
  const [terminalHeight, setTerminalHeight] = createSignal(
    Math.max(150, Number(localStorage.getItem("hirsel_terminal_height")) || 250),
  );
  const [roleModels, setRoleModels] = createSignal<SettingsResponse["role_models"] | null>(null);
  let liveUpdatesCleanup: (() => void) | undefined;
  const scheduledRefreshes = new Map<string, number>();
  let transcriptRef: HTMLDivElement | undefined;

  const clearScheduledRefreshes = () => {
    for (const timer of scheduledRefreshes.values()) {
      window.clearTimeout(timer);
    }
    scheduledRefreshes.clear();
  };

  const clearOptimisticTurn = () => {
    setOptimisticLiveTurn(null);
    setOptimisticTurnStartedAt(null);
  };

  const beginOptimisticTurn = (status: string) => {
    const startedAt = new Date().toISOString();
    setOptimisticTurnStartedAt(startedAt);
    setOptimisticLiveTurn({
      chunks_json: "[]",
      status,
      updated_at: startedAt,
    });
  };

  const clearLiveUpdates = () => {
    liveUpdatesCleanup?.();
    liveUpdatesCleanup = undefined;
    clearScheduledRefreshes();
  };

  const scheduleRefresh = (key: string, work: () => Promise<void>, delay = 90) => {
    if (scheduledRefreshes.has(key)) return;
    const timer = window.setTimeout(() => {
      scheduledRefreshes.delete(key);
      void work();
    }, delay);
    scheduledRefreshes.set(key, timer);
  };

  const redirectAfterMissingProject = async () => {
    try {
      const updated = await listProjects();
      setProjects(updated);
      if (updated.length === 0) {
        window.location.hash = "#new";
        return;
      }
      window.location.hash = `#project/${updated[0].id}`;
    } catch (redirectError) {
      console.error("Failed to redirect after missing project", redirectError);
      window.location.hash = "#new";
    }
  };

  const loadWorkspaceSnapshotResource = async (): Promise<boolean> => {
    try {
      const [projectList, snapshot] = await Promise.all([
        listProjects(),
        getWorkspaceSnapshot(props.projectId, {
          threadId: props.threadId,
          librarian: props.librarianView,
        }),
      ]);
      setProjects(projectList);
      setProject(snapshot.project);
      setProjectActivity(snapshot.project_activity);
      setProjectHistory(snapshot.project_history);
      setProjectSurface(snapshot.surface);
      setThreads(snapshot.threads);
      setThreadDetail(snapshot.thread_detail);
      setThreadHistory(snapshot.thread_history);
      setFocusedTask(snapshot.focused_task);
      setLibrarianActivity(snapshot.librarian_activity);
      setLibrarianHistory(snapshot.librarian_history);
      setConnectionOk(true);
      return true;
    } catch (err) {
      if (err instanceof ApiError && err.status === 404) {
        void redirectAfterMissingProject();
        return false;
      }
      setConnectionOk(false);
      setError(err instanceof Error ? err.message : "Failed to load workspace");
      return false;
    }
  };

  const refreshWorkspace = async () => {
    if (await loadWorkspaceSnapshotResource()) {
      setError("");
    }
  };

  const listAndApplyProjects = async (): Promise<void> => {
    try {
      setProjects(await listProjects());
    } catch {
      // ignore project list refresh failures; workspace refresh carries the main state
    }
  };

  const handleLiveUpdate = (event: LiveUpdateEvent) => {
    if (event.projectId !== props.projectId) return;

    switch (event.kind) {
      case "project_changed":
        scheduleRefresh("projects", listAndApplyProjects, 0);
        scheduleRefresh("workspace", refreshWorkspace, 0);
        break;
      case "project_surface_changed":
      case "project_history_changed":
      case "project_activity_changed":
      case "librarian_history_changed":
      case "librarian_activity_changed":
        scheduleRefresh("workspace", refreshWorkspace, 0);
        break;
      case "knowledge_graph_changed":
        setCanvasReloadNonce((n) => n + 1);
        break;
      case "threads_changed":
      case "thread_changed":
      case "thread_history_changed":
      case "thread_activity_changed":
        scheduleRefresh("workspace", refreshWorkspace, 0);
        setCanvasReloadNonce((n) => n + 1);
        break;
      case "tasks_changed":
      case "task_changed":
      case "canvas_layout_changed":
        setCanvasReloadNonce((n) => n + 1);
        break;
    }
  };

  const connectLiveUpdates = () => {
    clearLiveUpdates();
    liveUpdatesCleanup = subscribeProjectEvents(
      props.projectId,
      handleLiveUpdate,
      () => setConnectionOk(true),
      () => setConnectionOk(false),
    );
  };

  createEffect(
    on(
      () => [props.projectId, props.threadId],
      () => {
        setProjects([]);
        setProject(null);
        setProjectActivity(null);
        setProjectHistory([]);
        setProjectSurface(null);
        setThreads([]);
        setThreadDetail(null);
        setThreadHistory([]);
        setLibrarianActivity(null);
        setLibrarianHistory([]);
        setError("");
        setInput("");
        clearOptimisticTurn();
        setOptimisticUserMessage(null);
        setMobileSidebarOpen(false);
        setSettingsOpen(false);
        setProjectSettingsOpen(false);
        clearLiveUpdates();

        void (async () => {
          await loadWorkspaceSnapshotResource();
          connectLiveUpdates();
        })();

        onCleanup(clearLiveUpdates);
      },
    ),
  );

  createEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (matchesAction(event, "toggle-terminal")) {
        event.preventDefault();
        setTerminalOpen((v) => !v);
        return;
      }
      if (matchesAction(event, "toggle-sidebar")) {
        event.preventDefault();
        if (compactViewport()) {
          setMobileSidebarOpen((v) => !v);
        } else {
          setSidebarCollapsed((v) => !v);
        }
        return;
      }
      if (matchesAction(event, "close-panel")) {
        if (settingsOpen() || projectSettingsOpen()) {
          event.preventDefault();
          setSettingsOpen(false);
          setProjectSettingsOpen(false);
          return;
        }
        if (terminalOpen()) {
          event.preventDefault();
          setTerminalOpen(false);
          return;
        }
      }
      if (matchesAction(event, "stop-generation")) {
        if (isRunning()) {
          event.preventDefault();
          void handleStop();
        }
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
    if (!inspectorOpen()) {
      setInspectorFullscreen(false);
    }
  });

  createEffect(() => {
    if (!activeRuntimeError()) {
      setDismissedRuntimeErrorRaw(null);
    }
  });

  createEffect(() => {
    const optimistic = optimisticLiveTurn();
    const startedAt = optimisticTurnStartedAt();
    if (!optimistic || !startedAt) return;

    const backendLiveTurn = props.librarianView
      ? librarianActivity()?.live_turn
      : props.threadId
        ? threadDetail()?.activity.live_turn
        : projectActivity()?.live_turn;
    const backendHasActiveTurn = props.librarianView
      ? librarianActivity()?.has_active_turn ?? false
      : props.threadId
        ? threadDetail()?.activity.has_active_turn ?? false
        : projectActivity()?.has_active_turn ?? false;
    const backendSessionStatus = props.librarianView
      ? librarianActivity()?.session?.status ?? null
      : props.threadId
        ? threadDetail()?.activity.session?.status ?? null
        : projectActivity()?.session?.status ?? null;

    if (backendLiveTurn || backendHasActiveTurn) {
      clearOptimisticTurn();
      return;
    }

    const optimisticAgeMs = Date.now() - Date.parse(startedAt);
    if (backendSessionStatus === "idle" && Number.isFinite(optimisticAgeMs) && optimisticAgeMs > 1500) {
      clearOptimisticTurn();
      return;
    }

    const runtimeError = props.librarianView
      ? librarianActivity()?.session?.last_error
      : props.threadId
        ? threadDetail()?.activity.session?.last_error
        : projectActivity()?.session?.last_error;
    if (runtimeError) {
      clearOptimisticTurn();
      return;
    }

    const lastAssistantMessage = [...activeMessages()]
      .reverse()
      .find((message) => message.role === "assistant");
    if (lastAssistantMessage && lastAssistantMessage.timestamp >= startedAt) {
      clearOptimisticTurn();
    }
  });

  onMount(() => {
    const media = window.matchMedia(MOBILE_MEDIA);
    const loadRoleModels = () => {
      void getSettings().then((settings) => setRoleModels(settings.role_models)).catch(() => {});
    };
    const handleMedia = () => {
      const compact = media.matches;
      setCompactViewport(compact);
      if (!compact) {
        setMobileSidebarOpen(false);
        setInspectorFullscreen(false);
      } else if (inspectorOpen()) {
        setInspectorFullscreen(true);
      }
    };

    media.addEventListener("change", handleMedia);
    handleMedia();
    loadRoleModels();
    window.addEventListener("hirsel-settings-changed", loadRoleModels);

    onCleanup(() => {
      media.removeEventListener("change", handleMedia);
      window.removeEventListener("hirsel-settings-changed", loadRoleModels);
    });
  });

  const projectName = () =>
    project()?.name ?? `Project ${props.projectId}`;

  const activeModel = () => {
    const models = roleModels();
    if (!models) return "";
    const role = props.librarianView ? models.librarian : props.threadId ? models.thread : models.shepherd;
    const model = role.effective_model;
    const variant = role.effective_model_variant;
    if (!model) return "";
    return variant ? `${model} · ${variant}` : model;
  };

  const activeThreadPanel = createMemo(
    () => threads().find((thread) => thread.thread.id === props.threadId) ?? null,
  );

  const activeTitle = () => {
    if (settingsOpen()) return "Settings";
    if (projectSettingsOpen()) return "Project Settings";
    if (props.librarianView) return "Librarian";
    if (!props.threadId) return ROOT_CHANNEL_LABEL;
    return threadDetail()?.thread.title || activeThreadPanel()?.thread.title || "Untitled Thread";
  };

  const activeStatus = () => {
    const sessionStatus = activeSessionStatus();
    if (sessionStatus) return sessionStatus;
    if (activeExecutionStatus()) return "running";
    if (!props.threadId && !props.librarianView) return "ready";
    return threadDetail()?.thread.status || activeThreadPanel()?.thread.status || "active";
  };

  const projectScopeStatus = () =>
    !props.threadId && !props.librarianView && isRunning() ? "running" : "ready";

  const activeMessages = () => {
    const history = (props.librarianView ? librarianHistory() : (props.threadId ? threadHistory() : projectHistory())).filter(
      (message) => message.role !== "system",
    );
    const optimistic = optimisticUserMessage();
    if (!optimistic) return history;
    const lastMessage = history[history.length - 1];
    if (!lastMessage || lastMessage.role !== "user" || lastMessage.timestamp < optimistic.timestamp) {
      return [...history, optimistic];
    }
    setOptimisticUserMessage(null);
    return history;
  };

  const activeLiveTurn = () => {
    const backendLiveTurn = props.librarianView
      ? librarianActivity()?.live_turn ?? null
      : props.threadId
        ? threadDetail()?.activity.live_turn ?? null
        : projectActivity()?.live_turn ?? null;
    return backendLiveTurn ?? optimisticLiveTurn();
  };

  const activeExecutionStatus = () => {
    const optimistic = optimisticLiveTurn();
    if (optimistic && ["starting", "queued", "running", "interrupting"].includes(optimistic.status)) {
      return optimistic.status;
    }
    return activeLiveTurn()?.status ?? null;
  };

  const activeSessionStatus = () => {
    if (props.librarianView) return librarianActivity()?.session?.status ?? null;
    if (props.threadId) return threadDetail()?.activity.session?.status ?? activeThreadPanel()?.activity.session?.status ?? null;
    return projectActivity()?.session?.status ?? null;
  };

  const activeRuntimeError = () =>
    classifyWorkspaceError(
      props.librarianView
        ? librarianActivity()?.session?.last_error
        : props.threadId
          ? threadDetail()?.activity.session?.last_error
          : projectActivity()?.session?.last_error,
    );
  const visibleRuntimeError = () => {
    const banner = activeRuntimeError();
    if (!banner) return null;
    if (banner.raw === dismissedRuntimeErrorRaw()) return null;
    return banner;
  };

  const pageError = () => classifyWorkspaceError(error());

  const isRunning = () => {
    return ["queued", "starting", "running", "interrupting"].includes(activeExecutionStatus() ?? "");
  };

  const sortedThreads = createMemo(() => {
    const list = threads();
    return [...list].sort((a, b) => {
      const sa = a.activity.session?.status ?? a.thread.status;
      const sb = b.activity.session?.status ?? b.thread.status;
      return statusPriority(sa) - statusPriority(sb);
    });
  });

  const runningThreads = () =>
    sortedThreads().filter((thread) => {
      const status = thread.activity.session?.status ?? thread.thread.status;
      return status === "running" || status === "active" || status === "starting_container" || status === "waiting_for_socket";
    }).length;

  /* ── Multi-project sidebar helpers ── */

  // Auto-expand current project on first load / when project changes
  createEffect(
    on(
      () => props.projectId,
      (id) => {
        setExpandedProjects((prev) => {
          if (prev.has(id)) return prev;
          const next = new Set(prev);
          next.add(id);
          return next;
        });
      },
    ),
  );

  const isProjectExpanded = (id: number) => expandedProjects().has(id);

  const toggleProjectExpand = async (id: number) => {
    const expanded = new Set(expandedProjects());
    if (expanded.has(id)) {
      expanded.delete(id);
      setExpandedProjects(expanded);
      return;
    }
    expanded.add(id);
    setExpandedProjects(expanded);
    if (id !== props.projectId && !otherProjectThreads().has(id)) {
      try {
        const t = await listThreads(id);
        setOtherProjectThreads((prev) => new Map(prev).set(id, t));
      } catch { /* ignore — just show empty */ }
    }
  };

  const threadsForProject = (id: number): ThreadSummary[] =>
    id === props.projectId ? sortedThreads() : (otherProjectThreads().get(id) ?? []);

  const focusHtml = () => {
    return projectSurface()?.canvas_html ?? "";
  };
  const focusSource = () => focusSourceLabel(projectSurface()?.canvas_source);

  const handleStop = async () => {
    // Clear the live turn immediately so the UI doesn't show a lingering "Stopping" bubble.
    // The backend will persist the interrupted message into history via SSE.
    setOptimisticLiveTurn(null);
    if (props.librarianView) {
      await stopLibrarianChat(props.projectId);
    } else if (props.threadId) {
      await stopThreadChat(props.projectId, props.threadId);
    } else {
      await stopChat(props.projectId);
    }
    setJustStopped(true);
    setTimeout(() => setJustStopped(false), 3000);
  };

  const handleKnowledgeScan = async () => {
    if (scanning()) return;
    setScanning(true);
    setError("");
    setStickToBottom(true);
    setJustStopped(false);
    setOptimisticUserMessage({
      id: -1,
      role: "user",
      message_kind: "chat",
      preview_text: null,
      collapsed_by_default: false,
      chunks_json: JSON.stringify([{ type: "text", content: LIBRARIAN_FULL_SCAN_MESSAGE }]),
      timestamp: new Date().toISOString(),
    });
    const shouldStartOptimisticTurn = !isRunning();
    if (shouldStartOptimisticTurn) {
      beginOptimisticTurn("running");
    }
    try {
      const response = await sendLibrarianMessage(props.projectId, LIBRARIAN_FULL_SCAN_MESSAGE);
      if (shouldStartOptimisticTurn && !response.started) {
        clearOptimisticTurn();
      }
    } catch (err) {
      clearOptimisticTurn();
      setOptimisticUserMessage(null);
      setError(err instanceof Error ? err.message : "Failed to start librarian scan");
    } finally {
      setScanning(false);
    }
  };

  const handleSubmit = async (event?: Event) => {
    event?.preventDefault();
    const content = input().trim();
    if (!content) return;

    setInput("");
    setStickToBottom(true);
    setJustStopped(false);
    setOptimisticUserMessage({
      id: -1,
      role: "user",
      message_kind: "chat",
      preview_text: null,
      collapsed_by_default: false,
      chunks_json: JSON.stringify([{ type: "text", content }]),
      timestamp: new Date().toISOString(),
    });
    const shouldStartOptimisticTurn = !isRunning();
    if (shouldStartOptimisticTurn) {
      beginOptimisticTurn("running");
    }

    try {
      const response = props.librarianView
        ? await sendLibrarianMessage(props.projectId, content)
        : props.threadId
          ? await sendThreadMessage(props.projectId, props.threadId, content)
          : await sendChatMessage(props.projectId, content);
      if (shouldStartOptimisticTurn && !response.started) {
        clearOptimisticTurn();
      }
    } catch (err) {
      clearOptimisticTurn();
      setOptimisticUserMessage(null);
      setInput(content);
      setError(err instanceof Error ? err.message : "Failed to send message");
    }
  };

  const updateStickinessFromScroll = () => {
    if (!transcriptRef) return;
    const { scrollTop, scrollHeight, clientHeight } = transcriptRef;
    setStickToBottom(scrollHeight - scrollTop - clientHeight < 80);
  };

  const useSuggestion = (suggestion: string) => {
    setInput(suggestion);
    setComposerFocusNonce((current) => current + 1);
  };

  const openInspectorTab = (tab: "files") => {
    if (inspectorOpen() && inspectorTab() === tab) {
      setInspectorFullscreen(false);
      setInspectorOpen(false);
      return;
    }
    setInspectorTab(tab);
    setInspectorOpen(true);
    if (compactViewport()) {
      setInspectorFullscreen(true);
    }
  };

  return (
    <div class="workspace-shell relative flex min-h-0 flex-1 flex-col overflow-hidden bg-background">
        <a
          href="#main-content"
          class="sr-only focus:not-sr-only focus:absolute focus:left-4 focus:top-3 focus:z-50 focus:bg-brand focus:px-3 focus:py-1.5 focus:font-mono focus:text-[10px] focus:uppercase focus:tracking-wider focus:text-background"
        >
          Skip to main content
        </a>
        <header class="relative z-20 flex h-12 shrink-0 items-center gap-4 border-b border-border/40 bg-background px-4">
          <button
            type="button"
            class={cn(
              "flex h-8 w-8 items-center justify-center text-muted-foreground/70 transition-colors hover:text-foreground",
              !compactViewport() && !sidebarCollapsed() && "hidden",
            )}
            onClick={() => {
              if (compactViewport()) {
                setMobileSidebarOpen(true);
              } else {
                setSidebarCollapsed(false);
              }
            }}
            aria-label="Open project list"
            title="Projects"
          >
            <span class="h-4 w-4">{menuIcon()}</span>
          </button>

          {/* Engraved nameplate */}
          <a
            href="#"
            class="group/brand flex items-center gap-2 select-none"
          >
            <span class="font-display text-[15px] font-medium tracking-[0.04em] text-foreground transition-colors group-hover/brand:text-brand">
              HIRSEL
            </span>
            <span class="font-mono text-[9px] tabular-nums text-muted-foreground/30">v0.4</span>
          </a>

          {/* Vertical divider */}
          <span class="h-5 w-px bg-border/50" aria-hidden="true" />

          {/* Project + scope path */}
          <div class="flex min-w-0 flex-1 items-center gap-3">
            <h1 class="m-0 truncate">
              <a
                href={`#project/${props.projectId}`}
                class="truncate font-display text-[15px] font-medium tracking-tight text-foreground transition-colors hover:text-brand"
                title={projectName()}
              >
                {projectName()}
              </a>
            </h1>
            <Show when={props.threadId && !settingsOpen() && !projectSettingsOpen()}>
              <span class="hidden font-mono text-[10px] uppercase tracking-[0.14em] text-muted-foreground/70 md:inline">
                Thread
              </span>
              <span class="hidden max-w-[280px] truncate text-[13px] text-foreground/80 md:inline">
                {activeTitle()}
              </span>
            </Show>
            <Show when={props.librarianView}>
              <span class="hidden font-mono text-[10px] uppercase tracking-[0.14em] text-muted-foreground/70 md:inline">
                Librarian
              </span>
            </Show>
          </div>

          {/* Right controls */}
          <div class="ml-auto flex items-center gap-1">
            {/* Connection pilot light — just the dot, no label */}
            <div
              class="flex h-8 items-center gap-1.5 px-1.5"
              title={connectionOk() ? "Connected — live updates on" : "Connection lost. Live updates paused — check your network or restart Hirsel."}
              role="status"
              aria-live="polite"
              aria-label={connectionOk() ? "Connected" : "Connection lost"}
            >
              <span
                class={cn(
                  "h-1.5 w-1.5 rounded-full transition-colors",
                  connectionOk() ? "bg-signal-green/70" : "bg-signal-red animate-pulse-dot",
                )}
                aria-hidden="true"
              />
              <Show when={!connectionOk()}>
                <span class="font-mono text-[9px] uppercase tracking-wider text-signal-red/80">offline</span>
              </Show>
            </div>

            <button
              type="button"
              class={cn(
                "inline-flex h-8 w-8 items-center justify-center text-muted-foreground/70 transition-colors hover:text-foreground",
                settingsOpen() && "text-brand",
              )}
              title="Settings"
              aria-label="Open settings"
              onClick={() => { setSettingsOpen((v) => !v); setProjectSettingsOpen(false); }}
            >
              <span class="h-[14px] w-[14px]">{settingsIcon()}</span>
            </button>
          </div>
        </header>

        <Show when={pageError()}>
          <div class="relative z-10 flex items-center justify-between border-b border-signal-red/20 bg-signal-red/[0.06] px-4 py-2.5 text-xs text-signal-red">
            {(() => {
              const banner = pageError();
              if (!banner) return null;
              return (
              <>
                <span>{banner.message}</span>
                <div class="ml-3 flex items-center gap-3">
                  <Show when={banner.action === "open-settings"}>
                    <button
                      type="button"
                      class="text-[11px] font-medium text-signal-red transition-colors hover:text-signal-red/80"
                      onClick={() => setSettingsOpen(true)}
                    >
                      Open Settings
                    </button>
                  </Show>
                  <button
                    type="button"
                    class="text-signal-red/60 hover:text-signal-red"
                    onClick={() => setError("")}
                  >
                    <svg viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="2">
                      <path d="M6 6l12 12" />
                      <path d="M18 6L6 18" />
                    </svg>
                  </button>
                </div>
              </>
              );
            })()}
          </div>
        </Show>

        <div class="relative flex min-h-0 flex-1 overflow-hidden">
          <button
            class={cn(
              "absolute inset-0 z-20 transition-opacity duration-200 md:hidden",
              compactViewport() && mobileSidebarOpen()
                ? "bg-background/80 opacity-100"
                : "pointer-events-none opacity-0",
            )}
            onClick={() => setMobileSidebarOpen(false)}
            aria-label="Close project list"
          />

          <nav
            aria-label="Projects"
            class={cn(
              "z-30 flex shrink-0 flex-col overflow-hidden bg-card",
              compactViewport()
                ? cn(
                    "absolute inset-y-0 left-0 w-[280px] shadow-lift transition-transform duration-200",
                    mobileSidebarOpen() ? "translate-x-0" : "-translate-x-full",
                  )
                : "relative",
            )}
            style={compactViewport() ? undefined : { width: sidebarCollapsed() ? "0px" : `${sidebarWidth()}px` }}
          >
            <Show when={compactViewport()}>
              <div class="flex items-center justify-between border-b border-border/40 px-4 py-2.5">
                <span class="font-mono text-[10px] font-medium uppercase tracking-[0.14em] text-muted-foreground">Projects</span>
                <button
                  type="button"
                  class="flex h-6 w-6 items-center justify-center text-muted-foreground transition-colors hover:text-foreground"
                  onClick={() => setMobileSidebarOpen(false)}
                  aria-label="Close sidebar"
                >
                  <svg viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="2">
                    <path d="M6 6l12 12" />
                    <path d="M18 6L6 18" />
                  </svg>
                </button>
              </div>
            </Show>

            <div class="flex-1 overflow-y-auto">
              {/* PROJECTS header (desktop only — mobile has its own) */}
              <Show when={!compactViewport()}>
                <div class="flex items-center justify-between px-4 py-3">
                  <span class="font-mono text-[10px] font-medium uppercase tracking-[0.14em] text-muted-foreground">Projects</span>
                  <a
                    href="#new"
                    class="flex h-6 w-6 items-center justify-center rounded text-muted-foreground/70 transition-colors hover:bg-secondary/60 hover:text-foreground"
                    title="New project"
                    aria-label="Create a new project"
                  >
                    <svg viewBox="0 0 16 16" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="1.5">
                      <path d="M8 3v10M3 8h10" />
                    </svg>
                  </a>
                </div>
              </Show>

              <div class="px-2 pb-4">
                <For each={projects()}>
                  {(project) => {
                    const isCurrent = () => project.id === props.projectId;
                    const expanded = () => isProjectExpanded(project.id);
                    const projectThreads = () => threadsForProject(project.id);

                    return (
                      <div class="mt-0.5">
                        {/* ── Project header row ── */}
                        <div class="group/proj flex items-center">
                          <button
                            type="button"
                            class="flex h-7 w-5 shrink-0 items-center justify-center text-muted-foreground/50 transition-colors hover:text-foreground"
                            onClick={() => void toggleProjectExpand(project.id)}
                            aria-label={expanded() ? "Collapse" : "Expand"}
                          >
                            <svg
                              viewBox="0 0 16 16"
                              class={cn("h-2.5 w-2.5 transition-transform duration-150", expanded() && "rotate-90")}
                              fill="none"
                              stroke="currentColor"
                              stroke-width="2"
                            >
                              <path d="M6 4l4 4-4 4" />
                            </svg>
                          </button>
                          <a
                            href={`#project/${project.id}`}
                            class={cn(
                              "flex flex-1 min-w-0 items-center gap-2 px-1.5 py-1 text-sm transition-colors",
                              isCurrent()
                                ? "font-medium text-foreground"
                                : "text-muted-foreground hover:text-foreground",
                            )}
                            onClick={() => { setMobileSidebarOpen(false); setSettingsOpen(false); setProjectSettingsOpen(false); }}
                          >
                            <svg viewBox="0 0 16 16" class="h-3 w-3 shrink-0 opacity-40" fill="none" stroke="currentColor" stroke-width="1.5">
                              <path d="M2 5V13H14V5" />
                              <path d="M2 5L7 2H9L14 5" />
                            </svg>
                            <span class="truncate">{project.name}</span>
                          </a>
                          <Show when={isCurrent()}>
                            <button
                              type="button"
                              class={cn(
                                "flex h-6 w-6 shrink-0 items-center justify-center text-muted-foreground/40 transition-all hover:text-foreground",
                                projectSettingsOpen() ? "opacity-100 text-foreground" : "opacity-0 group-hover/proj:opacity-100",
                              )}
                              onClick={(e) => {
                                e.preventDefault();
                                e.stopPropagation();
                                setProjectSettingsOpen((v) => !v);
                                setSettingsOpen(false);
                              }}
                              aria-label="Project settings"
                              title="Project settings"
                            >
                              <svg viewBox="0 0 16 16" class="h-3 w-3" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                                <circle cx="8" cy="8" r="2" />
                                <path d="M6.7 2.5l-.4 1.2a4.5 4.5 0 0 0-1 .6L4 3.9l-1.3 2.2 1 .9a4.5 4.5 0 0 0 0 1.2l-1 .9L4 11.3l1.3-.4a4.5 4.5 0 0 0 1 .6l.4 1.2h2.6l.4-1.2a4.5 4.5 0 0 0 1-.6l1.3.4 1.3-2.2-1-.9a4.5 4.5 0 0 0 0-1.2l1-.9L12 3.9l-1.3.4a4.5 4.5 0 0 0-1-.6l-.4-1.2z" />
                              </svg>
                            </button>
                          </Show>
                        </div>

                        {/* ── Expanded: project channels + threads ── */}
                        <Show when={expanded()}>
                          <div class="ml-[10px] border-l border-border/40 pl-1 pb-1">

                            {/* Shepherd + Librarian (compact, only when current project) */}
                            <Show when={isCurrent()}>
                              <a
                                href={`#project/${project.id}`}
                                class={cn(
                                  "flex items-center gap-2 px-2 py-1.5 text-xs transition-colors",
                                  !props.threadId && !props.librarianView && !settingsOpen() && !projectSettingsOpen()
                                    ? "text-foreground"
                                    : "text-muted-foreground hover:text-foreground",
                                )}
                                style={!props.threadId && !props.librarianView && !settingsOpen() && !projectSettingsOpen()
                                  ? { "background": "color-mix(in oklch, oklch(var(--brand)) 6%, transparent)" }
                                  : undefined}
                                onClick={() => { setMobileSidebarOpen(false); setSettingsOpen(false); setProjectSettingsOpen(false); }}
                              >
                                <svg viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1.3" class="shrink-0">
                                  <rect x="2" y="2" width="5" height="5" />
                                  <rect x="9" y="2" width="5" height="5" />
                                  <rect x="2" y="9" width="5" height="5" />
                                  <rect x="9" y="9" width="5" height="5" />
                                </svg>
                                <span class="flex-1 truncate">Canvas</span>
                              </a>
                              <a
                                href={`#librarian/${project.id}`}
                                class={cn(
                                  "flex items-center gap-2 px-2 py-1.5 text-xs transition-colors",
                                  props.librarianView && !settingsOpen() && !projectSettingsOpen()
                                    ? "text-foreground"
                                    : "text-muted-foreground hover:text-foreground",
                                )}
                                style={props.librarianView && !settingsOpen() && !projectSettingsOpen()
                                  ? { "background": "color-mix(in oklch, oklch(var(--brand)) 6%, transparent)" }
                                  : undefined}
                                onClick={() => { setMobileSidebarOpen(false); setSettingsOpen(false); setProjectSettingsOpen(false); }}
                              >
                                <span class={cn(
                                  "h-1.5 w-1.5 shrink-0 rounded-full",
                                  librarianActivity()?.has_active_turn ? "bg-signal-amber animate-pulse-dot" : "bg-muted-foreground/30",
                                )} />
                                <span class="flex-1 truncate">Librarian</span>
                              </a>
                            </Show>

                            {/* Thread list */}
                            <For each={projectThreads()}>
                              {(thread) => {
                                const status = () => isCurrent()
                                  ? (thread.activity.session?.status ?? thread.thread.status)
                                  : thread.thread.status;
                                const meta = () => {
                                  if (!isCurrent()) return "";
                                  const progress = thread.plan_progress
                                    ? `${thread.plan_progress.completed}/${thread.plan_progress.total}`
                                    : null;
                                  const label = statusLabel(status());
                                  return progress ? `${progress} ${label}` : label;
                                };
                                const isActive = () => props.threadId === thread.thread.id && isCurrent() && !settingsOpen() && !projectSettingsOpen();

                                const hasHighlight = () => !!thread.thread.highlight;

                                return (
                                  <a
                                    href={`#thread/${project.id}/${thread.thread.id}`}
                                    data-thread-id={thread.thread.id}
                                    class={cn(
                                      "group flex flex-col gap-0.5 px-2 py-1.5 text-xs transition-all",
                                      isActive()
                                        ? "text-foreground"
                                        : "text-muted-foreground hover:text-foreground active:scale-[0.995]",
                                      hasHighlight() && !isActive()
                                        ? "border-l-2 border-signal-amber/50"
                                        : "",
                                    )}
                                    style={{
                                      ...(isActive() ? { "background": "color-mix(in oklch, oklch(var(--brand)) 6%, transparent)" } : {}),
                                      ...(hasHighlight() ? { "box-shadow": "inset 0 0 12px oklch(var(--signal-amber) / 0.08)" } : {}),
                                    }}
                                    onClick={() => { setMobileSidebarOpen(false); setSettingsOpen(false); setProjectSettingsOpen(false); }}
                                  >
                                    <div class="flex items-center gap-2">
                                      <span class={cn(
                                        "h-1.5 w-1.5 shrink-0 rounded-full",
                                        isActive() ? "bg-brand" : hasHighlight() ? "bg-signal-amber animate-pulse-dot" : statusDotClass(status()),
                                      )} />
                                      <span class="flex-1 truncate">{thread.thread.title || "Untitled Thread"}</span>
                                      <span class="ml-auto shrink-0 font-mono text-[9px] uppercase tracking-wider text-muted-foreground/50">{meta()}</span>
                                    </div>
                                    <Show when={hasHighlight()}>
                                      <div class="pl-4 text-[10px] leading-tight text-signal-amber/70 line-clamp-2">
                                        {thread.thread.highlight}
                                      </div>
                                    </Show>
                                  </a>
                                );
                              }}
                            </For>

                            <Show when={isCurrent() && projectThreads().length === 0}>
                              <div class="px-2 py-3 text-[11px] text-muted-foreground/70 italic">
                                No threads yet
                              </div>
                            </Show>
                          </div>
                        </Show>
                      </div>
                    );
                  }}
                </For>
              </div>
            </div>
          </nav>

          <Show when={!compactViewport()}>
            <div
              class={cn(
                "relative z-10 flex w-0 shrink-0 cursor-col-resize select-none items-center justify-center",
                "after:absolute after:inset-y-0 after:-left-2 after:-right-2 after:content-['']",
                sidebarDragging() && "bg-ring/20",
              )}
              onPointerDown={(event) => {
                event.preventDefault();
                setSidebarDragging(true);
                const startX = event.clientX;
                const startW = sidebarCollapsed() ? 0 : sidebarWidth();
                const onMove = (moveEvent: PointerEvent) => {
                  const next = startW + (moveEvent.clientX - startX);
                  if (next < SIDEBAR_MIN * 0.6) {
                    setSidebarCollapsed(true);
                  } else {
                    const inspW = inspectorOpen() ? inspectorWidth() : 0;
                    const maxSidebar = Math.min(SIDEBAR_MAX, window.innerWidth - inspW - MAIN_MIN);
                    setSidebarCollapsed(false);
                    setSidebarWidth(Math.max(SIDEBAR_MIN, Math.min(maxSidebar, next)));
                  }
                };
                const onUp = () => {
                  setSidebarDragging(false);
                  if (!sidebarCollapsed()) {
                    localStorage.setItem(SIDEBAR_WIDTH_KEY, String(sidebarWidth()));
                  }
                  window.removeEventListener("pointermove", onMove);
                  window.removeEventListener("pointerup", onUp);
                };
                window.addEventListener("pointermove", onMove);
                window.addEventListener("pointerup", onUp);
              }}
              onDblClick={() => setSidebarCollapsed((v) => !v)}
            />
          </Show>

          <div class="relative flex min-w-0 flex-1 overflow-hidden">
            <main id="main-content" class="relative flex flex-1 flex-col overflow-hidden" style={{ "min-width": `${MAIN_MIN}px` }}>
              <div class="relative flex h-10 shrink-0 items-center gap-3 border-b border-border/30 bg-background px-4">
                <Show when={settingsOpen()}>
                  <span class="font-mono text-[10px] uppercase tracking-[0.14em] text-brand/80">
                    Settings
                  </span>
                </Show>
                <Show when={projectSettingsOpen()}>
                  <span class="font-mono text-[10px] uppercase tracking-[0.14em] text-brand/80">
                    Project
                  </span>
                  <span class="h-3 w-px bg-border/40" aria-hidden="true" />
                  <span class="truncate text-[13px] font-medium text-foreground">
                    {project()?.name ?? "Project"}
                  </span>
                </Show>
                <Show when={!settingsOpen() && !projectSettingsOpen()}>
                  <Show when={props.threadId}>
                    <span class="font-mono text-[10px] uppercase tracking-[0.14em] text-muted-foreground/70">
                      Thread
                    </span>
                    <span class="h-3 w-px bg-border/40" aria-hidden="true" />
                  </Show>
                  <span class="truncate text-[13px] font-medium text-foreground">
                    {activeTitle()}
                  </span>
                  <span class={cn("h-1.5 w-1.5 shrink-0 rounded-full", statusDotClass(activeStatus()))} />
                  <button
                    type="button"
                    class="ml-0.5 font-mono text-[10px] text-muted-foreground/70 transition-colors hover:text-muted-foreground"
                    onClick={() => setSettingsOpen(true)}
                    title="Change model"
                    aria-label={`Current model: ${activeModel()}. Click to change.`}
                  >
                    {activeModel()}
                  </button>
                </Show>
                <Show when={settingsOpen() || projectSettingsOpen()}>
                  <button
                    type="button"
                    class="ml-auto flex h-8 w-8 items-center justify-center text-muted-foreground/70 transition-colors hover:text-foreground"
                    onClick={() => { setSettingsOpen(false); setProjectSettingsOpen(false); }}
                    title="Close (Esc)"
                    aria-label="Close"
                  >
                    <svg viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                      <path d="M6 6l12 12" />
                      <path d="M18 6L6 18" />
                    </svg>
                  </button>
                </Show>
                <Show when={!settingsOpen() && !projectSettingsOpen() && !inspectorOpen()}>
                  <div class="ml-auto flex shrink-0 items-center gap-1">
                    <button
                      type="button"
                      class="flex h-8 w-8 items-center justify-center text-muted-foreground/60 transition-colors hover:text-foreground"
                      onClick={() => openInspectorTab("files")}
                      title="Open files"
                      aria-label="Open files panel"
                    >
                      <span class="h-3.5 w-3.5">{filesIcon()}</span>
                    </button>
                  </div>
                </Show>
              </div>

              <Show
                when={!settingsOpen() && !projectSettingsOpen()}
                fallback={
                  <div class="flex-1 overflow-y-auto chassis-scroll">
                    <Show when={settingsOpen()}>
                      <div class="mx-auto max-w-2xl px-8 py-8">
                        <SettingsForm onClose={() => setSettingsOpen(false)} />
                      </div>
                    </Show>
                    <Show when={projectSettingsOpen()}>
                      {(() => {
                        const proj = () => project();
                        const [nameVal, setNameVal] = createSignal(proj()?.name ?? "");
                        const [saving, setSaving] = createSignal(false);
                        const [confirmDelete, setConfirmDelete] = createSignal(false);
                        const [statusMsg, setStatusMsg] = createSignal("");

                        createEffect(() => {
                          const p = proj();
                          if (p) {
                            setNameVal(p.name);
                          }
                        });

                        const isDirty = () => {
                          const p = proj();
                          if (!p) return false;
                          return nameVal() !== p.name;
                        };

                        const handleSave = async () => {
                          const p = proj();
                          if (!p) return;
                          const name = nameVal().trim();
                          if (!name) return;
                          setSaving(true);
                          setStatusMsg("");
                          try {
                            await saveProjectSettings(p.id, { name });
                            const updated = await listProjects();
                            setProjects(updated);
                            setStatusMsg("Saved");
                            setTimeout(() => setStatusMsg(""), 2000);
                          } catch (e) {
                            setStatusMsg("Couldn't save");
                            console.error("Failed to save project settings", e);
                          } finally {
                            setSaving(false);
                          }
                        };

                        const handleDelete = async () => {
                          const p = proj();
                          if (!p) return;
                          try {
                            const currentProjects = projects();
                            const currentIndex = currentProjects.findIndex((candidate) => candidate.id === p.id);
                            const remainingProjects = currentProjects.filter((candidate) => candidate.id !== p.id);
                            await deleteProject(p.id);
                            setProjectSettingsOpen(false);

                            if (remainingProjects.length === 0) {
                              setProjects([]);
                              window.location.hash = "#new";
                              return;
                            }

                            const fallbackIndex = currentIndex < 0
                              ? 0
                              : Math.min(currentIndex, remainingProjects.length - 1);
                            setProjects(remainingProjects);
                            window.location.hash = `#project/${remainingProjects[fallbackIndex].id}`;
                            void listProjects().then(setProjects).catch((error) => {
                              console.error("Failed to refresh projects after delete", error);
                            });
                          } catch (e) {
                            console.error("Failed to delete project", e);
                          }
                        };

                        const createdAt = () => {
                          const p = proj();
                          if (!p?.created_at) return null;
                          try { return new Date(p.created_at).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" }); }
                          catch { return null; }
                        };

                        return (
                          <div class="mx-auto max-w-2xl px-8 py-8">
                            <Show when={createdAt()}>
                              <div class="mb-6">
                                <div class="font-mono text-[10px] uppercase tracking-[0.14em] text-muted-foreground/30">
                                  Created {createdAt()}
                                </div>
                              </div>
                            </Show>

                            {/* ── General section ── */}
                            <div class="space-y-5">
                              <div class="chassis-label mb-3">General</div>

                              <div class="space-y-1.5">
                                <label class="text-xs font-medium text-foreground">Name</label>
                                <input
                                  type="text"
                                  class="z-input w-full"
                                  value={nameVal()}
                                  onInput={(e) => setNameVal(e.currentTarget.value)}
                                  onKeyDown={(e) => { if (e.key === "Enter") void handleSave(); }}
                                />
                              </div>

                            </div>

                            {/* ── Workspaces section ── */}
                            {(() => {
                              const [addingWs, setAddingWs] = createSignal(false);
                              const [wsKind, setWsKind] = createSignal<"local" | "git">("local");
                              const [wsValue, setWsValue] = createSignal("");
                              const [wsLabel, setWsLabel] = createSignal("");
                              const [wsSaving, setWsSaving] = createSignal(false);
                              const [wsError, setWsError] = createSignal("");

                              const workspaces = () => proj()?.workspaces ?? [];

                              const handleAddWorkspace = async () => {
                                const p = proj();
                                if (!p) return;
                                const val = wsValue().trim();
                                if (!val) return;
                                setWsSaving(true);
                                setWsError("");
                                try {
                                  const ws: Omit<ProjectWorkspaceEntry, "id"> = {
                                    kind: wsKind(),
                                    label: wsLabel().trim() || val.split("/").filter(Boolean).pop() || val,
                                    path: wsKind() === "local" ? val : null,
                                    url: wsKind() === "git" ? val : null,
                                    branch: null,
                                  };
                                  await addProjectWorkspace(p.id, ws);
                                  const updated = await listProjects();
                                  setProjects(updated);
                                  setAddingWs(false);
                                  setWsValue("");
                                  setWsLabel("");
                                } catch (e) {
                                  setWsError(e instanceof Error ? e.message : "Failed to add workspace");
                                } finally {
                                  setWsSaving(false);
                                }
                              };

                              const handleRemoveWorkspace = async (wsId: string) => {
                                const p = proj();
                                if (!p) return;
                                try {
                                  await removeProjectWorkspace(p.id, wsId);
                                  const updated = await listProjects();
                                  setProjects(updated);
                                } catch (e) {
                                  console.error("Failed to remove workspace", e);
                                }
                              };

                              return (
                                <div class="mt-10">
                                  <div class="chassis-label mb-3">Workspaces</div>

                                  <Show when={workspaces().length === 0 && !addingWs()}>
                                    <div class="border border-dashed border-border px-4 py-6 text-center">
                                      <p class="text-xs text-muted-foreground">No workspaces yet</p>
                                      <p class="mt-1 text-[11px] text-muted-foreground/70">Attach a local directory or a git repository so agents know where to work.</p>
                                      <button
                                        type="button"
                                        class="mt-3 text-xs text-brand transition-colors hover:text-foreground"
                                        onClick={() => setAddingWs(true)}
                                      >
                                        Add workspace
                                      </button>
                                    </div>
                                  </Show>

                                  <Show when={workspaces().length > 0}>
                                    <div class="space-y-1">
                                      <For each={workspaces()}>
                                        {(ws) => (
                                          <div class="group flex items-center gap-3 border border-border px-3 py-2">
                                            <span class="font-mono text-[10px] uppercase tracking-wider text-muted-foreground/70 w-8 shrink-0">{ws.kind}</span>
                                            <div class="min-w-0 flex-1">
                                              <div class="text-xs font-medium text-foreground truncate" title={ws.label}>{ws.label}</div>
                                              <div class="font-mono text-[10px] text-muted-foreground truncate" title={ws.path ?? ws.url ?? ""}>{ws.path || ws.url}</div>
                                            </div>
                                            <button
                                              type="button"
                                              class="text-[10px] text-muted-foreground/60 opacity-0 transition-all group-hover:opacity-100 hover:text-signal-red"
                                              onClick={() => void handleRemoveWorkspace(ws.id)}
                                              aria-label={`Remove workspace ${ws.label}`}
                                            >
                                              remove
                                            </button>
                                          </div>
                                        )}
                                      </For>
                                    </div>
                                  </Show>

                                  <Show when={addingWs()}>
                                    <div class="mt-2 border border-border px-4 py-4 space-y-3">
                                      <div class="flex gap-2">
                                        <button
                                          type="button"
                                          class={cn(
                                            "px-2.5 py-1 text-[11px] font-medium border transition-colors",
                                            wsKind() === "local" ? "border-foreground/20 bg-foreground/5 text-foreground" : "border-transparent text-muted-foreground hover:text-foreground",
                                          )}
                                          onClick={() => setWsKind("local")}
                                        >
                                          Local directory
                                        </button>
                                        <button
                                          type="button"
                                          class={cn(
                                            "px-2.5 py-1 text-[11px] font-medium border transition-colors",
                                            wsKind() === "git" ? "border-foreground/20 bg-foreground/5 text-foreground" : "border-transparent text-muted-foreground hover:text-foreground",
                                          )}
                                          onClick={() => setWsKind("git")}
                                        >
                                          Git repository
                                        </button>
                                      </div>

                                      <div class="space-y-1.5">
                                        <label class="text-[11px] font-medium text-foreground">
                                          {wsKind() === "local" ? "Path" : "URL"}
                                        </label>
                                        <input
                                          type="text"
                                          class="z-input w-full text-xs"
                                          placeholder={wsKind() === "local" ? "/home/sam/code/myapp" : "git@github.com:org/repo.git"}
                                          value={wsValue()}
                                          onInput={(e) => setWsValue(e.currentTarget.value)}
                                          autofocus
                                        />
                                      </div>

                                      <div class="space-y-1.5">
                                        <label class="text-[11px] font-medium text-foreground">
                                          Label <span class="text-muted-foreground/40">optional</span>
                                        </label>
                                        <input
                                          type="text"
                                          class="z-input w-full text-xs"
                                          placeholder="Short name for this workspace"
                                          value={wsLabel()}
                                          onInput={(e) => setWsLabel(e.currentTarget.value)}
                                        />
                                      </div>

                                      <Show when={wsError()}>
                                        <p class="text-[11px] text-signal-red">{wsError()}</p>
                                      </Show>

                                      <div class="flex items-center gap-2 pt-1">
                                        <button
                                          type="button"
                                          class="z-button-variant-default z-button-size-xs inline-flex items-center"
                                          disabled={!wsValue().trim() || wsSaving()}
                                          onClick={() => void handleAddWorkspace()}
                                        >
                                          {wsSaving() ? "Adding..." : "Add"}
                                        </button>
                                        <button
                                          type="button"
                                          class="text-[11px] text-muted-foreground transition-colors hover:text-foreground"
                                          onClick={() => { setAddingWs(false); setWsError(""); }}
                                        >
                                          Cancel
                                        </button>
                                      </div>
                                    </div>
                                  </Show>

                                  <Show when={workspaces().length > 0 && !addingWs()}>
                                    <button
                                      type="button"
                                      class="mt-2 text-[11px] text-muted-foreground/50 transition-colors hover:text-foreground"
                                      onClick={() => setAddingWs(true)}
                                    >
                                      + Add workspace
                                    </button>
                                  </Show>
                                </div>
                              );
                            })()}

                            {/* ── Save bar ── */}
                            <div class={cn(
                              "mt-8 flex items-center gap-3 border-t border-border pt-5 transition-opacity",
                              isDirty() ? "opacity-100" : "opacity-40",
                            )}>
                              <button
                                type="button"
                                class="z-button-variant-default z-button-size-sm inline-flex items-center gap-1.5"
                                disabled={saving() || !nameVal().trim() || !isDirty()}
                                onClick={() => void handleSave()}
                              >
                                <Show when={saving()} fallback={<>Save changes</>}>
                                  <svg class="h-3 w-3 animate-spin-arc" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2">
                                    <path d="M8 2a6 6 0 1 0 6 6" />
                                  </svg>
                                  Saving
                                </Show>
                              </button>
                              <Show when={statusMsg()}>
                                <span class={cn(
                                  "font-mono text-[10px] transition-colors",
                                  statusMsg() === "Saved" ? "text-signal-green" : "text-signal-red",
                                )}>
                                  {statusMsg()}
                                </span>
                              </Show>
                              <Show when={isDirty() && !statusMsg()}>
                                <span class="font-mono text-[10px] text-muted-foreground/40">Unsaved changes</span>
                              </Show>
                            </div>

                            {/* ── Danger zone ── */}
                            <div class="mt-12 border border-signal-red/15 bg-signal-red/[0.02]">
                              <div class="flex items-center gap-2 border-b border-signal-red/15 px-4 py-2.5">
                                <svg viewBox="0 0 16 16" class="h-3 w-3 text-signal-red/50" fill="none" stroke="currentColor" stroke-width="1.5">
                                  <path d="M8 2L1.5 13h13z" />
                                  <path d="M8 7v3" />
                                  <circle cx="8" cy="12" r="0.5" fill="currentColor" />
                                </svg>
                                <span class="font-mono text-[10px] uppercase tracking-[0.14em] text-signal-red/60">Danger Zone</span>
                              </div>
                              <div class="px-4 py-4">
                                <div class="flex items-center justify-between">
                                  <div>
                                    <div class="text-xs font-medium text-foreground">Delete this project</div>
                                    <p class="mt-0.5 text-[11px] text-muted-foreground">Removes all threads, conversation history, and knowledge graph entries. Workspaces themselves stay untouched. This can't be undone.</p>
                                  </div>
                                  <Show
                                    when={!confirmDelete()}
                                    fallback={
                                      <div class="flex items-center gap-2 shrink-0">
                                        <button
                                          type="button"
                                          class="z-button-variant-destructive z-button-size-sm inline-flex items-center"
                                          onClick={() => void handleDelete()}
                                        >
                                          Delete {proj()?.name ?? "project"}
                                        </button>
                                        <button
                                          type="button"
                                          class="text-[11px] text-muted-foreground transition-colors hover:text-foreground"
                                          onClick={() => setConfirmDelete(false)}
                                        >
                                          Cancel
                                        </button>
                                      </div>
                                    }
                                  >
                                    <button
                                      type="button"
                                      class="shrink-0 border border-signal-red/30 px-3 py-1.5 text-xs text-signal-red/70 transition-colors hover:bg-signal-red/10 hover:text-signal-red"
                                      onClick={() => setConfirmDelete(true)}
                                    >
                                      Delete project
                                    </button>
                                  </Show>
                                </div>
                              </div>
                            </div>
                          </div>
                        );
                      })()}
                    </Show>
                  </div>
                }
              >
                <Show
                  when={!props.threadId && !props.librarianView}
                  fallback={
                    <>
                      <ChatTranscript
                        title={activeTitle()}
                        threadId={props.threadId}
                        librarianView={props.librarianView}
                        loaded={props.threadId ? !!threadDetail() : !!project()}
                        messages={activeMessages()}
                        liveTurn={activeLiveTurn()}
                        runtimeError={visibleRuntimeError()}
                        scanning={scanning()}
                        stickToBottom={stickToBottom()}
                        onTranscriptRef={(element) => {
                          transcriptRef = element;
                        }}
                        onScroll={updateStickinessFromScroll}
                        onScrollToBottom={() => {
                          setStickToBottom(true);
                          if (transcriptRef) transcriptRef.scrollTop = transcriptRef.scrollHeight;
                        }}
                        onKnowledgeScan={() => void handleKnowledgeScan()}
                        onOpenSettings={() => setSettingsOpen(true)}
                        onDismissRuntimeError={(raw) => setDismissedRuntimeErrorRaw(raw)}
                        onSuggestion={useSuggestion}
                      />

                      <div class="relative shrink-0 border-t border-border/40 bg-card">
                        <div class="mx-auto max-w-2xl px-3 pb-3 pt-2">
                          <ChatComposer
                            projectId={props.projectId}
                            threadId={props.threadId}
                            value={input()}
                            running={isRunning()}
                            justStopped={justStopped()}
                            focusNonce={composerFocusNonce()}
                            onValueChange={setInput}
                            onSubmit={() => void handleSubmit()}
                            onStop={() => void handleStop()}
                          />
                        </div>
                      </div>
                    </>
                  }
                >
                  {/* Canvas view — the project home */}
                  <Suspense
                    fallback={
                      <div class="flex h-full items-center justify-center text-xs font-mono text-muted-foreground">
                        loading canvas...
                      </div>
                    }
                  >
                    <CanvasView
                      projectId={props.projectId}
                      refreshNonce={canvasReloadNonce()}
                      onOpenThread={(threadId) => {
                        window.location.hash = `#thread/${props.projectId}/${threadId}`;
                      }}
                    />
                  </Suspense>
                </Show>
              </Show>
            </main>

            <Show when={inspectorOpen()}>
              <Show when={!inspectorFullscreen() && !compactViewport()}>
                <div
                  class="inspector-resize-handle"
                  data-dragging={inspectorDragging()}
                  onPointerDown={(event) => {
                    event.preventDefault();
                    setInspectorDragging(true);
                    const startX = event.clientX;
                    const startW = inspectorWidth();
                    const onMove = (moveEvent: PointerEvent) => {
                      const delta = startX - moveEvent.clientX;
                      const sideW = sidebarCollapsed() ? 0 : sidebarWidth();
                      const maxW = Math.min(
                        Math.floor(window.innerWidth * 0.58),
                        window.innerWidth - sideW - MAIN_MIN,
                      );
                      const next = Math.max(INSPECTOR_MIN, Math.min(maxW, startW + delta));
                      setInspectorWidth(next);
                    };
                    const onUp = () => {
                      setInspectorDragging(false);
                      localStorage.setItem(INSPECTOR_WIDTH_KEY, String(inspectorWidth()));
                      window.removeEventListener("pointermove", onMove);
                      window.removeEventListener("pointerup", onUp);
                    };
                    window.addEventListener("pointermove", onMove);
                    window.addEventListener("pointerup", onUp);
                  }}
                />
              </Show>

              <aside
                class={cn(
                  "flex flex-col bg-card canvas-enter",
                  inspectorFullscreen() || compactViewport()
                    ? "absolute inset-0 z-40 shadow-lift"
                    : "shrink-0",
                )}
                style={
                  inspectorFullscreen() || compactViewport()
                    ? undefined
                    : { width: `${inspectorWidth()}px` }
                }
              >
                <div class="inspector-tab-bar">
                  <div class="inspector-tab-btn" data-active="true">
                    <span class="tab-icon">{filesIcon()}</span>
                    Files
                  </div>
                  <div class="inspector-controls">
                    <Show when={!compactViewport()}>
                      <button
                        type="button"
                        class="flex h-8 w-8 items-center justify-center text-muted-foreground/70 transition-colors hover:text-foreground"
                        title={inspectorFullscreen() ? "Exit fullscreen" : "Fullscreen"}
                        aria-label={inspectorFullscreen() ? "Exit fullscreen" : "Fullscreen"}
                        onClick={() => setInspectorFullscreen((current) => !current)}
                      >
                        <Show
                          when={inspectorFullscreen()}
                          fallback={
                            <svg viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                              <path d="M4 9V4h5" />
                              <path d="M20 9V4h-5" />
                              <path d="M4 15v5h5" />
                              <path d="M20 15v5h-5" />
                            </svg>
                          }
                        >
                          <svg viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                            <path d="M9 4v5H4" />
                            <path d="M15 4v5h5" />
                            <path d="M9 20v-5H4" />
                            <path d="M15 20v-5h5" />
                          </svg>
                        </Show>
                      </button>
                    </Show>
                    <button
                      type="button"
                      class="flex h-8 w-8 items-center justify-center text-muted-foreground/70 transition-colors hover:text-foreground"
                      onClick={() => {
                        setInspectorFullscreen(false);
                        setInspectorOpen(false);
                      }}
                      title="Close panel"
                      aria-label="Close panel"
                    >
                      <svg viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M6 6l12 12" />
                        <path d="M18 6L6 18" />
                      </svg>
                    </button>
                  </div>
                </div>

                <div class="flex-1 overflow-hidden bg-background">
                  <Show when={inspectorTab() === "files"}>
                    <Suspense
                      fallback={
                        <div class="flex h-full items-center justify-center text-xs font-mono text-muted-foreground">
                          loading files...
                        </div>
                      }
                    >
                      <WorkspaceBrowser projectId={props.projectId} threadId={props.threadId} />
                    </Suspense>
                  </Show>
                </div>
              </aside>
            </Show>
          </div>
        </div>

        {/* Terminal bottom panel */}
        <Show when={terminalOpen()}>
          {/* Terminal resize handle */}
          <div
            class="terminal-resize-handle"
            onPointerDown={(event) => {
              event.preventDefault();
              const startY = event.clientY;
              const startH = terminalHeight();
              const onMove = (moveEvent: PointerEvent) => {
                const next = startH + (startY - moveEvent.clientY);
                setTerminalHeight(Math.max(120, Math.min(window.innerHeight * 0.6, next)));
              };
              const onUp = () => {
                localStorage.setItem("hirsel_terminal_height", String(terminalHeight()));
                window.removeEventListener("pointermove", onMove);
                window.removeEventListener("pointerup", onUp);
              };
              window.addEventListener("pointermove", onMove);
              window.addEventListener("pointerup", onUp);
            }}
          />
          <div
            class="shrink-0 border-t border-border"
            style={{ height: `${terminalHeight()}px` }}
          >
            <Suspense
              fallback={
                <div class="flex h-full items-center justify-center text-xs font-mono text-muted-foreground">
                  loading terminal...
                </div>
              }
            >
              <TerminalPanel
                projectId={props.projectId}
                threads={sortedThreads().map((t) => ({ id: t.thread.id, title: t.thread.title }))}
                onClose={() => setTerminalOpen(false)}
              />
            </Suspense>
          </div>
        </Show>
    </div>
  );
};

export default WorkspacePage;
