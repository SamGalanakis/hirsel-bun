import {
  type Component,
  createEffect,
  createSignal,
  on,
  onCleanup,
  Show,
} from "solid-js";
import ChatPanel from "@/components/ChatPanel";
import PlanPanel from "@/components/PlanPanel";
import ThreadSidebar from "@/components/ThreadSidebar";
import ThemeSwitcher from "@/components/ThemeSwitcher";
import {
  type ThreadPageData,
  type ProjectPageData,
  getProjectPage,
  getThreadPage,
  sendThreadMessage,
  stopThreadChat,
} from "@/lib/api";

interface ThreadDetailPageProps {
  projectId: number;
  threadId: string;
}

const ThreadDetailPage: Component<ThreadDetailPageProps> = (props) => {
  const [data, setData] = createSignal<ThreadPageData | null>(null);
  const [projectData, setProjectData] = createSignal<ProjectPageData | null>(null);
  const [error, setError] = createSignal("");

  const poll = async () => {
    try {
      const page = await getThreadPage(props.projectId, props.threadId);
      setData(page);
      setError("");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load thread");
    }
  };

  const pollProject = async () => {
    try {
      const page = await getProjectPage(props.projectId);
      setProjectData(page);
    } catch { /* thread sidebar is best-effort */ }
  };

  createEffect(
    on(
      () => [props.projectId, props.threadId],
      () => {
        poll();
        pollProject();
        const id = setInterval(poll, 2000);
        // Poll project less frequently (for thread sidebar updates)
        const projectId = setInterval(pollProject, 5000);
        onCleanup(() => { clearInterval(id); clearInterval(projectId); });
      },
    ),
  );

  createEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape" && isRunning()) {
        e.preventDefault();
        stopThreadChat(props.projectId, props.threadId);
      }
    };
    window.addEventListener("keydown", handler);
    onCleanup(() => window.removeEventListener("keydown", handler));
  });

  const isRunning = () => data()?.activity.has_active_turn ?? false;

  const handleSend = async (content: string) => {
    try {
      await sendThreadMessage(props.projectId, props.threadId, content);
      poll();
    } catch (err) {
      console.error("Send failed:", err);
    }
  };

  const handleStop = () => {
    stopThreadChat(props.projectId, props.threadId);
  };

  return (
    <div class="flex min-h-0 flex-1 flex-col bg-background overflow-hidden">
      <header class="flex items-center gap-3 px-4 h-[46px] shrink-0 border-b border-border bg-card">
        <a
          href={`#project/${props.projectId}`}
          class="font-display text-base text-foreground tracking-tight"
        >
          HIRSEL
        </a>

        <Show when={data()}>
          {(d) => (
            <div class="flex items-center gap-1.5">
              <span class="text-muted-foreground text-xs">/</span>
              <a
                href={`#project/${props.projectId}`}
                class="text-sm text-muted-foreground hover:text-foreground transition-colors truncate max-w-[200px]"
              >
                {d().project.name}
              </a>
              <span class="text-muted-foreground text-xs">/</span>
              <span class="text-sm text-foreground font-medium truncate max-w-[200px]">
                {d().thread.title || "Thread"}
              </span>
            </div>
          )}
        </Show>

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
        <div class="px-4 py-2 bg-signal-red/10 text-signal-red text-xs border-b border-border">
          {error()}
        </div>
      </Show>

      <div class="flex min-h-0 flex-1 overflow-hidden">
        <Show when={projectData()}>
          <div class="flex w-[220px] shrink-0 min-h-0 self-stretch overflow-hidden">
            <ThreadSidebar
              threads={projectData()?.threads ?? []}
              projectId={props.projectId}
              activeThreadId={props.threadId}
            />
          </div>
        </Show>

        <div class="flex min-h-0 flex-1 flex-col overflow-hidden">
          <Show when={data()?.thread}>
            {(thread) => (
              <Show when={thread().objective}>
                <div class="px-4 py-2 border-b border-border bg-muted/30">
                  <p class="text-xs text-muted-foreground">{thread().objective}</p>
                </div>
              </Show>
            )}
          </Show>

          <Show when={data()?.plan}>
            {(plan) => <PlanPanel plan={plan()} />}
          </Show>

          <div class="flex flex-1 min-h-0 overflow-hidden">
            <ChatPanel
              messages={data()?.history ?? []}
              liveTurn={data()?.activity.live_turn ?? null}
              isRunning={isRunning()}
              onSend={handleSend}
              onStop={handleStop}
            />
          </div>
        </div>
      </div>
    </div>
  );
};

export default ThreadDetailPage;
