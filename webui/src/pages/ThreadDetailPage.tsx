import {
  type Component,
  createEffect,
  createSignal,
  on,
  onCleanup,
  Show,
} from "solid-js";
import ChatPanel from "@/components/ChatPanel";
import ThemeSwitcher from "@/components/ThemeSwitcher";
import {
  type ThreadPageData,
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

  createEffect(
    on(
      () => [props.projectId, props.threadId],
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
        stopThreadChat(props.projectId, props.threadId);
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
    <div class="flex flex-col h-screen bg-background">
      {/* Titlebar */}
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
              <span class="text-sm text-foreground font-medium truncate max-w-[200px]">
                {d().project.name}
              </span>
              <span class="text-muted-foreground text-xs">/</span>
              <span class="text-sm text-foreground truncate max-w-[200px]">
                {d().thread.title || "Thread"}
              </span>
            </div>
          )}
        </Show>

        <div class="ml-auto flex items-center gap-2">
          <ThemeSwitcher />
          <a
            href={`#project/${props.projectId}`}
            class="text-xs text-muted-foreground hover:text-foreground transition-colors"
          >
            Back
          </a>
        </div>
      </header>

      {/* Error banner */}
      <Show when={error()}>
        <div class="px-4 py-2 bg-signal-red/10 text-signal-red text-xs border-b border-border">
          {error()}
        </div>
      </Show>

      {/* Thread header */}
      <Show when={data()?.thread}>
        {(thread) => (
          <Show when={thread().objective}>
            <div class="px-4 py-3 border-b border-border bg-muted/30">
              <p class="text-xs text-muted-foreground">{thread().objective}</p>
            </div>
          </Show>
        )}
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
  );
};

export default ThreadDetailPage;
