import {
  type ChatMessage,
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
import ChatComposer from "@/components/ChatComposer";
import ChatMessage from "@/components/ChatMessage";
import ProjectPreparationScreen from "@/components/ProjectPreparationScreen";
import SettingsForm from "@/components/SettingsForm";
import KnowledgeGraphView from "@/components/KnowledgeGraphView";
import TerminalPanel from "@/components/TerminalPanel";
import WorkspaceBrowser from "@/components/WorkspaceBrowser";
import { matchesAction } from "@/lib/keybindings";
import {
  type Project,
  type ProjectPreparation,
  type ProjectSurface,
  type ScopeActivity,
  type LiveTurn,
  type LiveUpdateEvent,
  type ThreadDetail,
  type ThreadSummary,
  getProject,
  getProjectActivity,
  getProjectHistory,
  getProjectPreparation,
  getProjectSurface,
  getThread,
  getThreadHistory,
  listProjects,
  listThreads,
  retryProjectPreparation,
  sendChatMessage,
  sendThreadMessage,
  stopChat,
  stopThreadChat,
  subscribeProjectEvents,
  triggerKnowledgeScan,
  getLibrarianActivity,
  getLibrarianHistory,
  sendLibrarianMessage,
  stopLibrarianChat,
} from "@/lib/api";

interface WorkspacePageProps {
  projectId: number;
  threadId?: string;
  librarianView?: boolean;
}

const ROOT_CHANNEL_LABEL = "Shepherd";
const MOBILE_MEDIA = "(max-width: 900px)";
const INSPECTOR_WIDTH_KEY = "hirsel_workspace_inspector_width";
const INSPECTOR_MIN = 300;
const SIDEBAR_WIDTH_KEY = "hirsel_workspace_sidebar_width";
const SIDEBAR_MIN = 180;
const SIDEBAR_MAX = 480;
const SIDEBAR_DEFAULT = 260;

type WorkspaceBannerError = {
  message: string;
  action: "open-settings" | null;
  raw: string;
};

function statusDotClass(status: string): string {
  switch (status) {
    case "starting":
    case "running":
    case "active":
      return "bg-signal-green";
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
      message:
        "Codex is selected as the active provider, but it is not connected. Open Settings -> Provider to connect Codex or switch providers.",
      action: "open-settings",
      raw: trimmed,
    };
  }
  if (lower.includes("openrouter api key not configured")) {
    return {
      message:
        "OpenRouter is selected as the active provider, but no API key is configured. Open Settings -> Provider to add one or switch providers.",
      action: "open-settings",
      raw: trimmed,
    };
  }
  if (lower.includes("lock is already locked by another process")) {
    return {
      message:
        "The local worker crashed while opening the shared app database. Restart Hirsel so the worker runtime is rebuilt with the latest code.",
      action: null,
      raw: trimmed,
    };
  }
  if (lower.includes("rpc connection closed")) {
    return {
      message: "The local worker session disconnected unexpectedly. Restart Hirsel and try again.",
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

function canvasIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
      <rect x="3" y="4" width="18" height="16" rx="1" />
      <path d="M3 10h18" />
      <path d="M10 10v10" />
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
  const [projectHistory, setProjectHistory] = createSignal<ChatMessage[]>([]);
  const [projectSurface, setProjectSurface] = createSignal<ProjectSurface | null>(null);
  const [threads, setThreads] = createSignal<ThreadSummary[]>([]);
  const [threadDetail, setThreadDetail] = createSignal<ThreadDetail | null>(null);
  const [threadHistory, setThreadHistory] = createSignal<ChatMessage[]>([]);
  const [librarianActivity, setLibrarianActivity] = createSignal<ScopeActivity | null>(null);
  const [librarianHistory, setLibrarianHistory] = createSignal<ChatMessage[]>([]);
  const [preparation, setPreparation] = createSignal<ProjectPreparation | null>(null);
  const [error, setError] = createSignal("");
  const [connectionOk, setConnectionOk] = createSignal(true);
  const [retryingPreparation, setRetryingPreparation] = createSignal(false);
  const [mobileSidebarOpen, setMobileSidebarOpen] = createSignal(false);
  const [compactViewport, setCompactViewport] = createSignal(
    window.matchMedia(MOBILE_MEDIA).matches,
  );
  const [inspectorOpen, setInspectorOpen] = createSignal(window.innerWidth >= 1180);
  const [inspectorWidth, setInspectorWidth] = createSignal(
    Math.min(
      Math.floor(window.innerWidth * 0.46),
      Math.max(INSPECTOR_MIN, Number(localStorage.getItem(INSPECTOR_WIDTH_KEY)) || 520),
    ),
  );
  const [inspectorDragging, setInspectorDragging] = createSignal(false);
  const [inspectorFullscreen, setInspectorFullscreen] = createSignal(false);
  const [inspectorTab, setInspectorTab] = createSignal<"files" | "canvas" | "library">("files");
  const [sidebarWidth, setSidebarWidth] = createSignal(
    Math.max(SIDEBAR_MIN, Math.min(SIDEBAR_MAX, Number(localStorage.getItem(SIDEBAR_WIDTH_KEY)) || SIDEBAR_DEFAULT)),
  );
  const [sidebarCollapsed, setSidebarCollapsed] = createSignal(false);
  const [sidebarDragging, setSidebarDragging] = createSignal(false);
  const [settingsOpen, setSettingsOpen] = createSignal(false);
  const [input, setInput] = createSignal("");
  const [optimisticLiveTurn, setOptimisticLiveTurn] = createSignal<LiveTurn | null>(null);
  const [optimisticTurnStartedAt, setOptimisticTurnStartedAt] = createSignal<string | null>(null);
  const [composerFocusNonce, setComposerFocusNonce] = createSignal(0);
  const [stickToBottom, setStickToBottom] = createSignal(true);
  const [justStopped, setJustStopped] = createSignal(false);
  const [expandedProjects, setExpandedProjects] = createSignal<Set<number>>(new Set());
  const [otherProjectThreads, setOtherProjectThreads] = createSignal<Map<number, ThreadSummary[]>>(new Map());
  const [dismissedRuntimeErrorRaw, setDismissedRuntimeErrorRaw] = createSignal<string | null>(null);
  const [scanning, setScanning] = createSignal(false);
  const [knowledgeGraphReloadToken, setKnowledgeGraphReloadToken] = createSignal(0);
  const [terminalOpen, setTerminalOpen] = createSignal(false);
  const [terminalHeight, setTerminalHeight] = createSignal(
    Math.max(150, Number(localStorage.getItem("hirsel_terminal_height")) || 250),
  );
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

  const loadProjectShell = async (): Promise<boolean> => {
    try {
      const [projectList, currentProject, currentThreads] = await Promise.all([
        listProjects(),
        getProject(props.projectId),
        listThreads(props.projectId),
      ]);
      setProjects(projectList);
      setProject(currentProject);
      setThreads(currentThreads);
      setConnectionOk(true);
      return true;
    } catch (err) {
      setConnectionOk(false);
      setError(err instanceof Error ? err.message : "Failed to load project shell");
      return false;
    }
  };

  const loadProjectActivityResource = async (): Promise<boolean> => {
    try {
      const activity = await getProjectActivity(props.projectId);
      setProjectActivity(activity);
      setConnectionOk(true);
      return true;
    } catch (err) {
      setConnectionOk(false);
      setError(err instanceof Error ? err.message : "Failed to load project activity");
      return false;
    }
  };

  const loadProjectHistoryResource = async (): Promise<boolean> => {
    try {
      const history = await getProjectHistory(props.projectId, { limit: 100 });
      setProjectHistory(history);
      setConnectionOk(true);
      return true;
    } catch (err) {
      setConnectionOk(false);
      setError(err instanceof Error ? err.message : "Failed to load project history");
      return false;
    }
  };

  const loadProjectSurfaceResource = async (): Promise<boolean> => {
    try {
      const surface = await getProjectSurface(props.projectId);
      setProjectSurface(surface);
      setConnectionOk(true);
      return true;
    } catch (err) {
      setConnectionOk(false);
      setError(err instanceof Error ? err.message : "Failed to load project surface");
      return false;
    }
  };

  const loadThreadDetailResource = async (): Promise<boolean> => {
    if (!props.threadId) {
      setThreadDetail(null);
      return true;
    }
    try {
      const detail = await getThread(props.projectId, props.threadId);
      setThreadDetail(detail);
      setConnectionOk(true);
      return true;
    } catch (err) {
      setConnectionOk(false);
      setError(err instanceof Error ? err.message : "Failed to load thread detail");
      return false;
    }
  };

  const loadThreadHistoryResource = async (): Promise<boolean> => {
    if (!props.threadId) {
      setThreadHistory([]);
      return true;
    }
    try {
      const history = await getThreadHistory(props.projectId, props.threadId, { limit: 200 });
      setThreadHistory(history);
      setConnectionOk(true);
      return true;
    } catch (err) {
      setConnectionOk(false);
      setError(err instanceof Error ? err.message : "Failed to load thread history");
      return false;
    }
  };

  const loadLibrarianActivityResource = async (): Promise<boolean> => {
    try {
      const activity = await getLibrarianActivity(props.projectId);
      setLibrarianActivity(activity);
      setConnectionOk(true);
      return true;
    } catch (err) {
      setConnectionOk(false);
      if (props.librarianView) {
        setError(err instanceof Error ? err.message : "Failed to load librarian activity");
      }
      return false;
    }
  };

  const loadLibrarianHistoryResource = async (): Promise<boolean> => {
    if (!props.librarianView) {
      setLibrarianHistory([]);
      return true;
    }
    try {
      const history = await getLibrarianHistory(props.projectId, { limit: 200 });
      setLibrarianHistory(history);
      setConnectionOk(true);
      return true;
    } catch (err) {
      setConnectionOk(false);
      return false;
    }
  };

  const refreshWorkspace = async () => {
    const results = await Promise.all([
      loadProjectShell(),
      loadProjectActivityResource(),
      loadProjectHistoryResource(),
      loadProjectSurfaceResource(),
      loadThreadDetailResource(),
      loadThreadHistoryResource(),
      loadLibrarianActivityResource(),
      loadLibrarianHistoryResource(),
    ]);
    if (results.every(Boolean)) {
      setError("");
    }
  };

  const loadPreparation = async (): Promise<boolean> => {
    try {
      const state = await getProjectPreparation(props.projectId);
      setPreparation(state);
      setConnectionOk(true);
      setError("");
      if (state.status === "done") {
        await refreshWorkspace();
      }
      return true;
    } catch (err) {
      setConnectionOk(false);
      setError(err instanceof Error ? err.message : "Failed to load project setup");
      return false;
    }
  };

  const handleLiveUpdate = (event: LiveUpdateEvent) => {
    if (event.projectId !== props.projectId) return;

    if (showPreparation()) {
      if (
        event.kind === "project_preparation_changed" ||
        event.kind === "project_changed"
      ) {
        scheduleRefresh("preparation", loadPreparation, 40);
      }
      return;
    }

    switch (event.kind) {
      case "project_changed":
        scheduleRefresh("project-shell", loadProjectShell);
        break;
      case "project_surface_changed":
        scheduleRefresh("project-surface", loadProjectSurfaceResource);
        break;
      case "project_history_changed":
        scheduleRefresh("project-history", loadProjectHistoryResource, 60);
        break;
      case "project_activity_changed":
        scheduleRefresh("project-activity", loadProjectActivityResource, 60);
        break;
      case "librarian_history_changed":
        scheduleRefresh("librarian-history", loadLibrarianHistoryResource, 60);
        break;
      case "librarian_activity_changed":
        scheduleRefresh("librarian-activity", loadLibrarianActivityResource, 60);
        break;
      case "knowledge_graph_changed":
        setKnowledgeGraphReloadToken((current) => current + 1);
        break;
      case "threads_changed":
        scheduleRefresh("project-shell", loadProjectShell, 80);
        break;
      case "thread_changed":
        scheduleRefresh("project-shell", loadProjectShell, 80);
        if (event.threadId === props.threadId) {
          scheduleRefresh("thread-detail", loadThreadDetailResource, 60);
        }
        break;
      case "thread_history_changed":
        scheduleRefresh("project-shell", loadProjectShell, 90);
        if (event.threadId === props.threadId) {
          scheduleRefresh("thread-detail", loadThreadDetailResource, 60);
          scheduleRefresh("thread-history", loadThreadHistoryResource, 60);
        }
        break;
      case "thread_activity_changed":
        scheduleRefresh("project-shell", loadProjectShell, 80);
        if (event.threadId === props.threadId) {
          scheduleRefresh("thread-detail", loadThreadDetailResource, 60);
        }
        break;
      case "project_preparation_changed":
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
        setPreparation(null);
        setError("");
        setInput("");
        clearOptimisticTurn();
        setMobileSidebarOpen(false);
        setSettingsOpen(false);
        clearLiveUpdates();

        void (async () => {
          await loadPreparation();
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
        if (settingsOpen()) {
          event.preventDefault();
          setSettingsOpen(false);
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

    if (backendLiveTurn || backendHasActiveTurn) {
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

    onCleanup(() => {
      media.removeEventListener("change", handleMedia);
    });
  });

  const showPreparation = () => {
    const state = preparation();
    return state !== null && state.status !== "done";
  };

  const projectName = () =>
    project()?.name ?? preparation()?.project.name ?? `Project ${props.projectId}`;

  const activeThreadPanel = createMemo(
    () => threads().find((thread) => thread.thread.id === props.threadId) ?? null,
  );

  const activeTitle = () => {
    if (settingsOpen()) return "Settings";
    if (props.librarianView) return "Librarian";
    if (!props.threadId) return ROOT_CHANNEL_LABEL;
    return threadDetail()?.thread.title || activeThreadPanel()?.thread.title || "Untitled Thread";
  };

  const activeStatus = () => {
    if (props.librarianView) {
      return isRunning() ? "running" : "ready";
    }
    if (!props.threadId) {
      return isRunning() ? "running" : "ready";
    }
    if (optimisticLiveTurn()) {
      return "running";
    }
    return threadDetail()?.thread.status || activeThreadPanel()?.thread.status || "active";
  };

  const projectScopeStatus = () =>
    !props.threadId && !props.librarianView && isRunning() ? "running" : "ready";

  const activeMessages = () => {
    if (props.librarianView) return librarianHistory().filter((m) => m.role !== "system");
    return (props.threadId ? threadHistory() : projectHistory()).filter(
      (message) => message.role !== "system",
    );
  };

  const activeLiveTurn = () => {
    const backendLiveTurn = props.librarianView
      ? librarianActivity()?.live_turn ?? null
      : props.threadId
        ? threadDetail()?.activity.live_turn ?? null
        : projectActivity()?.live_turn ?? null;
    return backendLiveTurn ?? optimisticLiveTurn();
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
    const optimistic = optimisticLiveTurn();
    if (optimistic && ["starting", "running", "interrupting"].includes(optimistic.status)) {
      return true;
    }
    if (props.librarianView) return librarianActivity()?.has_active_turn ?? false;
    return props.threadId
      ? threadDetail()?.activity.has_active_turn ?? false
      : projectActivity()?.has_active_turn ?? false;
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
      return status === "running" || status === "active";
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
    return projectSurface()?.focus_html ?? "";
  };
  const focusSource = () => focusSourceLabel(projectSurface()?.focus_source);

  const handleRetryPreparation = async () => {
    setRetryingPreparation(true);
    try {
      const state = await retryProjectPreparation(props.projectId);
      setPreparation(state);
      setError("");
      if (state.status === "done") {
        await refreshWorkspace();
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to retry preparation");
    } finally {
      setRetryingPreparation(false);
    }
  };

  const handleStop = async () => {
    setOptimisticLiveTurn((current) =>
      current
        ? {
            ...current,
            status: "interrupting",
            updated_at: new Date().toISOString(),
          }
        : current,
    );
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
    try {
      await triggerKnowledgeScan(props.projectId);
      const [activityOk, historyOk] = await Promise.all([
        loadLibrarianActivityResource(),
        loadLibrarianHistoryResource(),
      ]);
      if (!activityOk || !historyOk) {
        setError("Started librarian scan, but failed to refresh librarian state.");
      }
    } catch (err) {
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
    const shouldStartOptimisticTurn = !isRunning();
    if (shouldStartOptimisticTurn) {
      beginOptimisticTurn("starting");
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

  const openInspectorTab = (tab: "files" | "canvas" | "library") => {
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
            class={cn(
              "flex h-8 w-8 items-center justify-center border border-border bg-background text-muted-foreground transition-colors hover:text-foreground",
              !compactViewport() && !sidebarCollapsed() && "hidden",
            )}
            onClick={() => {
              if (compactViewport()) {
                setMobileSidebarOpen(true);
              } else {
                setSidebarCollapsed(false);
              }
            }}
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
            <a
              href={`#project/${props.projectId}`}
              class="truncate text-sm font-medium text-foreground transition-opacity hover:opacity-75"
            >
              {projectName()}
            </a>
          </div>

          <Show when={props.threadId && !settingsOpen()}>
            <span class="hidden text-xs text-muted-foreground md:inline">/</span>
            <span class="hidden max-w-[220px] truncate text-xs text-ink-2 md:inline">
              {activeTitle()}
            </span>
          </Show>

          <div class="ml-auto flex items-center gap-2">
            <span
              class={cn(
                "h-2 w-2 rounded-full transition-colors",
                connectionOk() ? "bg-signal-green/60" : "bg-signal-red animate-pulse-dot",
              )}
              title={connectionOk() ? "Connected" : "Connection lost"}
            />
            <button
              type="button"
              class={cn(
                "inline-flex h-[34px] w-[34px] items-center justify-center border border-border bg-background text-muted-foreground transition-colors hover:text-foreground",
                settingsOpen() && "bg-secondary text-foreground",
              )}
              title="Settings"
              onClick={() => setSettingsOpen((v) => !v)}
            >
              <span class="h-[15px] w-[15px]">{settingsIcon()}</span>
            </button>
          </div>
        </header>

        <Show when={pageError()}>
          <div class="relative z-10 flex items-center justify-between border-b border-border bg-signal-red/10 px-4 py-2 text-xs text-signal-red">
            {(banner) => (
              <>
                <span>{banner().message}</span>
                <div class="ml-3 flex items-center gap-3">
                  <Show when={banner().action === "open-settings"}>
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
            )}
          </div>
        </Show>

        <div class="relative flex min-h-0 flex-1 overflow-hidden">
          <button
            class={cn(
              "absolute inset-0 z-20 transition-opacity duration-200 md:hidden",
              compactViewport() && mobileSidebarOpen()
                ? "bg-foreground/40 backdrop-blur-[2px] opacity-100"
                : "pointer-events-none opacity-0",
            )}
            onClick={() => setMobileSidebarOpen(false)}
            aria-label="Close threads"
          />

          <aside
            class={cn(
              "z-30 flex shrink-0 flex-col overflow-hidden bg-card/92 backdrop-blur",
              compactViewport()
                ? cn(
                    "absolute inset-y-0 left-0 w-[280px] shadow-lift transition-transform duration-200",
                    mobileSidebarOpen() ? "translate-x-0" : "-translate-x-full",
                  )
                : "relative transition-[width] duration-150",
            )}
            style={compactViewport() ? undefined : { width: sidebarCollapsed() ? "0px" : `${sidebarWidth()}px` }}
          >
            <Show when={compactViewport()}>
              <div class="flex items-center justify-between border-b border-border px-4 py-2">
                <span class="font-mono text-[11px] font-medium uppercase tracking-widest text-muted-foreground">Projects</span>
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
                  <span class="font-mono text-[11px] font-medium uppercase tracking-widest text-muted-foreground">Projects</span>
                  <a
                    href="#new"
                    class="flex h-5 w-5 items-center justify-center text-muted-foreground/60 transition-colors hover:text-foreground"
                    title="New project"
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
                        <div class="flex items-center">
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
                            onClick={() => { setMobileSidebarOpen(false); setSettingsOpen(false); }}
                          >
                            <svg viewBox="0 0 16 16" class="h-3 w-3 shrink-0 opacity-40" fill="none" stroke="currentColor" stroke-width="1.5">
                              <path d="M2 5V13H14V5" />
                              <path d="M2 5L7 2H9L14 5" />
                            </svg>
                            <span class="truncate">{project.name}</span>
                          </a>
                        </div>

                        {/* ── Expanded: project channels + threads ── */}
                        <Show when={expanded()}>
                          <div class="ml-[10px] border-l border-border/40 pl-1 pb-1">

                            {/* Current project: show Project root + Librarian */}
                            <Show when={isCurrent()}>
                              <a
                                href={`#project/${project.id}`}
                                class={cn(
                                  "group flex items-center gap-2 px-2 py-1.5 text-xs transition-colors",
                                  !props.threadId && !props.librarianView && !settingsOpen()
                                    ? "bg-background text-foreground shadow-sm border border-border"
                                    : "text-muted-foreground hover:text-foreground border border-transparent hover:border-border",
                                )}
                                onClick={() => { setMobileSidebarOpen(false); setSettingsOpen(false); }}
                              >
                                <span class={cn("h-1.5 w-1.5 shrink-0 rounded-full", statusDotClass(projectScopeStatus()))} />
                                <span class="truncate">{ROOT_CHANNEL_LABEL}</span>
                                <span class="ml-auto font-mono text-[10px] text-muted-foreground">{statusLabel(projectScopeStatus())}</span>
                              </a>

                              <a
                                href={`#librarian/${project.id}`}
                                class={cn(
                                  "group flex items-center gap-2 px-2 py-1.5 text-xs transition-colors",
                                  props.librarianView && !settingsOpen()
                                    ? "bg-background text-foreground shadow-sm border border-border"
                                    : "text-muted-foreground hover:text-foreground border border-transparent hover:border-border",
                                )}
                                onClick={() => { setMobileSidebarOpen(false); setSettingsOpen(false); }}
                              >
                                <span class={cn(
                                  "h-1.5 w-1.5 shrink-0 rounded-full",
                                  (props.librarianView && optimisticLiveTurn()) || librarianActivity()?.has_active_turn
                                    ? "bg-signal-amber animate-pulse-dot"
                                    : "bg-signal-amber/60",
                                )} />
                                <span class="truncate">Librarian</span>
                                <Show when={(props.librarianView && optimisticLiveTurn()) || librarianActivity()?.has_active_turn}>
                                  <span class="ml-auto font-mono text-[10px] text-muted-foreground">running</span>
                                </Show>
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

                                return (
                                  <a
                                    href={`#thread/${project.id}/${thread.thread.id}`}
                                    class={cn(
                                      "group flex items-center gap-2 px-2 py-1.5 text-xs transition-all",
                                      props.threadId === thread.thread.id && isCurrent() && !settingsOpen()
                                        ? "bg-background text-foreground shadow-sm border border-border"
                                        : "text-muted-foreground hover:text-foreground border border-transparent hover:border-border active:scale-[0.995]",
                                    )}
                                    onClick={() => { setMobileSidebarOpen(false); setSettingsOpen(false); }}
                                  >
                                    <span class={cn("h-1.5 w-1.5 shrink-0 rounded-full", statusDotClass(status()))} />
                                    <span class="flex-1 truncate">{thread.thread.title || "Untitled Thread"}</span>
                                    <span class="ml-auto shrink-0 font-mono text-[10px] text-muted-foreground">{meta()}</span>
                                  </a>
                                );
                              }}
                            </For>

                            <Show when={isCurrent() && projectThreads().length === 0}>
                              <div class="px-2 py-4 text-center text-[11px] text-muted-foreground/50">
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
          </aside>

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
                    setSidebarCollapsed(false);
                    setSidebarWidth(Math.max(SIDEBAR_MIN, Math.min(SIDEBAR_MAX, next)));
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
            <main class="relative flex min-w-0 flex-1 flex-col overflow-hidden">
              <div class="relative flex h-10 shrink-0 items-center gap-2 border-b border-border bg-card px-4">
                <span class="truncate text-sm font-medium text-foreground">
                  {activeTitle()}
                </span>
                <Show when={!settingsOpen()}>
                  <span class={cn("h-1.5 w-1.5 shrink-0 rounded-full", statusDotClass(activeStatus()))} />
                </Show>
                <Show when={!settingsOpen() && !inspectorOpen()}>
                  <div class="ml-auto flex shrink-0 items-center">
                    <button
                      type="button"
                      class="flex h-7 w-7 items-center justify-center text-muted-foreground/50 transition-colors hover:text-foreground"
                      onClick={() => openInspectorTab("files")}
                      title="Open inspector"
                    >
                      <span class="h-4 w-4">{filesIcon()}</span>
                    </button>
                  </div>
                </Show>
              </div>

              <Show
                when={!settingsOpen()}
                fallback={
                  <div class="flex-1 overflow-y-auto chassis-scroll">
                    <div class="mx-auto max-w-xl px-6 py-6">
                      <div class="mb-4 flex items-center justify-between">
                        <span class="text-sm font-medium text-foreground">Settings</span>
                        <button
                          type="button"
                          class="text-[11px] text-muted-foreground/50 transition-colors hover:text-foreground"
                          onClick={() => setSettingsOpen(false)}
                        >
                          Done
                        </button>
                      </div>
                      <SettingsForm onClose={() => setSettingsOpen(false)} />
                    </div>
                  </div>
                }
              >
                <div
                  ref={transcriptRef}
                  class="flex-1 overflow-y-auto chassis-scroll"
                  role="log"
                  aria-live="polite"
                  onScroll={updateStickinessFromScroll}
                >
                  <div class="mx-auto flex max-w-3xl flex-col gap-1 px-4 py-4">
                    <Show
                      when={
                        (props.threadId ? !!threadDetail() : !!project()) &&
                        (activeMessages().length > 0 || !!activeLiveTurn())
                      }
                      fallback={
                        <Show
                          when={props.threadId ? !!threadDetail() : !!project()}
                          fallback={
                            <div class="flex flex-col items-center justify-center gap-3 py-24">
                              <div class="flex items-center gap-2">
                                <div class="h-2 w-2 rounded-full bg-muted-foreground/30 animate-pulse-dot" />
                                <span class="text-xs text-muted-foreground font-mono">loading</span>
                              </div>
                            </div>
                          }
                        >
                          <div class="flex flex-col items-center justify-center gap-5 py-24 text-center">
                            <div class="flex h-14 w-14 items-center justify-center border border-border bg-card text-signal-amber shadow-sm">
                              <span class="font-mono text-lg">&#9678;</span>
                            </div>
                            <div class="space-y-2">
                              <h3 class="font-display text-2xl font-semibold tracking-tight text-foreground">
                                {activeTitle()}
                              </h3>
                              <p class="max-w-xl text-sm leading-6 text-muted-foreground">
                                {props.threadId
                                  ? "Use this thread for focused execution."
                                  : props.librarianView
                                    ? "Scan the workspace, capture architecture, and keep the project knowledge graph current."
                                    : "Direct the shepherd here, then spin work out into threads when it becomes concrete."}
                              </p>
                            </div>
                            <Show when={props.librarianView}>
                              <button
                                type="button"
                                class={cn(
                                  "mt-1 inline-flex h-8 items-center gap-2 border border-border bg-card px-3 text-xs text-muted-foreground transition-colors hover:text-signal-amber",
                                  scanning() && "text-signal-amber",
                                )}
                                disabled={scanning()}
                                onClick={() => void handleKnowledgeScan()}
                              >
                                <svg viewBox="0 0 24 24" class={cn("h-3.5 w-3.5", scanning() && "animate-spin")} fill="none" stroke="currentColor" stroke-width="2">
                                  <path d="M21 12a9 9 0 1 1-2.64-6.36" />
                                  <path d="M21 3v6h-6" />
                                </svg>
                                {scanning() ? "Scanning…" : "Scan Workspace"}
                              </button>
                            </Show>
                          </div>
                        </Show>
                      }
                    >
                      <Show when={visibleRuntimeError()}>
                        {(banner) => (
                          <div
                            class="mb-3 flex items-start justify-between gap-3 border border-signal-red/30 bg-signal-red/10 px-4 py-3 text-sm text-signal-red"
                            title={banner().raw}
                          >
                            <span>{banner().message}</span>
                            <div class="flex shrink-0 items-center gap-3">
                              <Show when={banner().action === "open-settings"}>
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
                                class="text-signal-red/60 transition-colors hover:text-signal-red"
                                onClick={() => setDismissedRuntimeErrorRaw(banner().raw)}
                                aria-label="Dismiss runtime error"
                                title="Dismiss"
                              >
                                <svg viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="2">
                                  <path d="M6 6l12 12" />
                                  <path d="M18 6L6 18" />
                                </svg>
                              </button>
                            </div>
                          </div>
                        )}
                      </Show>

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
                            liveStatus={turn().status}
                          />
                        )}
                      </Show>
                    </Show>
                  </div>
                </div>

                <Show when={!stickToBottom()}>
                  <button
                    type="button"
                    class="absolute bottom-20 right-6 z-10 flex h-8 w-8 items-center justify-center border border-border bg-card shadow-md text-muted-foreground transition-colors hover:text-foreground"
                    onClick={() => {
                      setStickToBottom(true);
                      if (transcriptRef) transcriptRef.scrollTop = transcriptRef.scrollHeight;
                    }}
                    title="Scroll to bottom"
                  >
                    <svg viewBox="0 0 24 24" class="h-4 w-4" fill="none" stroke="currentColor" stroke-width="2">
                      <polyline points="6 9 12 15 18 9" />
                    </svg>
                  </button>
                </Show>

                <div class="relative shrink-0 border-t border-border bg-card">
                  <div class="mx-auto max-w-3xl px-4 py-3">
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
              </Show>
            </main>

            <Show when={inspectorOpen() && !settingsOpen()}>
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
                      const maxW = Math.floor(window.innerWidth * 0.58);
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
                  <button
                    type="button"
                    class="inspector-tab-btn"
                    data-active={inspectorTab() === "files"}
                    onClick={() => setInspectorTab("files")}
                  >
                    <span class="tab-icon">{filesIcon()}</span>
                    Files
                  </button>
                  <button
                    type="button"
                    class="inspector-tab-btn"
                    data-active={inspectorTab() === "canvas"}
                    onClick={() => setInspectorTab("canvas")}
                  >
                    <span class="tab-icon">{canvasIcon()}</span>
                    Canvas
                  </button>
                  <button
                    type="button"
                    class="inspector-tab-btn"
                    data-active={inspectorTab() === "library"}
                    onClick={() => setInspectorTab("library")}
                  >
                    <span class="tab-icon">{libraryIcon()}</span>
                    Library
                  </button>
                  <div class="inspector-controls">
                    <Show when={!compactViewport()}>
                      <button
                        type="button"
                        class="inspector-tab-btn"
                        style={{ "text-transform": "none", "letter-spacing": "0", "font-family": "inherit" }}
                        onClick={() => setInspectorFullscreen((current) => !current)}
                      >
                        {inspectorFullscreen() ? "exit" : "full"}
                      </button>
                    </Show>
                    <button
                      type="button"
                      class="flex h-5 w-5 items-center justify-center text-muted-foreground/30 transition-colors hover:text-foreground"
                      onClick={() => {
                        setInspectorFullscreen(false);
                        setInspectorOpen(false);
                      }}
                      aria-label="Close"
                    >
                      <svg viewBox="0 0 24 24" class="h-2.5 w-2.5" fill="none" stroke="currentColor" stroke-width="2">
                        <path d="M6 6l12 12" />
                        <path d="M18 6L6 18" />
                      </svg>
                    </button>
                  </div>
                </div>

                <div class="flex-1 overflow-hidden bg-background">
                  <Show when={inspectorTab() === "files"}>
                    <WorkspaceBrowser projectId={props.projectId} threadId={props.threadId} />
                  </Show>
                  <Show when={inspectorTab() === "canvas"}>
                    <div class="h-full overflow-y-auto">
                      <Show
                        when={focusHtml().trim()}
                        fallback={
                          <div class="canvas-empty-state">
                            <svg class="canvas-empty-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                              <rect x="3" y="3" width="18" height="18" rx="0" />
                              <path d="M3 9h18" />
                              <path d="M9 21V9" />
                            </svg>
                            <div class="canvas-empty-title">No canvas content</div>
                            <div class="canvas-empty-hint">
                              Structured results from agent analysis will appear here — stats, code diffs, diagrams, and more.
                            </div>
                          </div>
                        }
                      >
                        <Show when={focusSource()}>
                          <div class="canvas-provenance">
                            <span class="canvas-provenance-dot" />
                            <span>{focusSource()}</span>
                          </div>
                        </Show>
                        <CanvasSurface html={focusHtml()} projectId={props.projectId} />
                      </Show>
                    </div>
                  </Show>
                  <Show when={inspectorTab() === "library"}>
                    <div class="relative h-full">
                      <KnowledgeGraphView
                        projectId={props.projectId}
                        reloadToken={knowledgeGraphReloadToken()}
                      />
                      <button
                        type="button"
                        class={cn(
                          "absolute bottom-3 right-3 flex h-7 items-center gap-1.5 bg-card/90 px-2.5 text-[11px] shadow-sm backdrop-blur transition-colors",
                          scanning() ? "text-signal-amber" : "text-muted-foreground hover:text-signal-amber",
                        )}
                        disabled={scanning()}
                        onClick={() => void handleKnowledgeScan()}
                        title="Full workspace scan"
                      >
                        <svg viewBox="0 0 24 24" class={cn("h-3 w-3", scanning() && "animate-spin")} fill="none" stroke="currentColor" stroke-width="2">
                          <path d="M21 12a9 9 0 1 1-2.64-6.36" />
                          <path d="M21 3v6h-6" />
                        </svg>
                        {scanning() ? "Scanning…" : "Scan"}
                      </button>
                    </div>
                  </Show>
                </div>
              </aside>
            </Show>
          </div>
        </div>

        {/* Terminal bottom panel */}
        <Show when={terminalOpen()}>
          <div
            class="shrink-0 border-t border-border"
            style={{ height: `${terminalHeight()}px` }}
          >
            <TerminalPanel
              projectId={props.projectId}
              threads={sortedThreads().map((t) => ({ id: t.thread.id, title: t.thread.title }))}
              onClose={() => setTerminalOpen(false)}
            />
          </div>
        </Show>
      </div>
    </Show>
  );
};

export default WorkspacePage;
