import { type Component, For, Show } from "solid-js";
import ChatMessage from "@/components/ChatMessage";
import type { ChatMessage as ApiChatMessage, LiveTurn } from "@/lib/api";
import { cn } from "@/lib/cn";

export interface ChatRuntimeBanner {
  message: string;
  action: "open-settings" | null;
  raw: string;
}

interface ChatTranscriptProps {
  title: string;
  threadId?: string;
  loaded: boolean;
  messages: ApiChatMessage[];
  liveTurn: LiveTurn | null;
  runtimeError: ChatRuntimeBanner | null;
  stickToBottom: boolean;
  onTranscriptRef?: (element: HTMLDivElement) => void;
  onScroll: () => void;
  onScrollToBottom: () => void;
  onOpenSettings: () => void;
  onDismissRuntimeError: (raw: string) => void;
  onSuggestion?: (text: string) => void;
}

const SHEPHERD_SUGGESTIONS = [
  "Walk me through this project",
  "Explore the code and tell me what you find",
  "What should we build next?",
];

const THREAD_SUGGESTIONS = [
  "What's the objective of this thread?",
  "Pick up where we left off",
];

const ChatTranscript: Component<ChatTranscriptProps> = (props) => {
  const hasContent = () => props.messages.length > 0 || !!props.liveTurn;

  const emptyTitle = () => props.title;

  const emptyBody = () => {
    if (props.threadId) {
      return "A thread is a focused branch of work — one task, one conversation, one agent. The shepherd oversees.";
    }
    return "The shepherd is your main collaborator. Ask questions, start tasks, and spin off threads when something becomes its own piece of work.";
  };

  const suggestions = () => {
    if (props.threadId) return THREAD_SUGGESTIONS;
    return SHEPHERD_SUGGESTIONS;
  };

  return (
    <div class="relative flex min-h-0 flex-1 flex-col overflow-hidden">
      <div
        ref={(element) => props.onTranscriptRef?.(element)}
        class="flex-1 overflow-y-auto chassis-scroll"
        role="log"
        aria-live="polite"
        onScroll={props.onScroll}
      >
        <div class="mx-auto flex max-w-3xl flex-col px-6 py-6">
          <Show
            when={props.loaded}
            fallback={
              <div class="flex flex-col items-center justify-center gap-3 py-28">
                <div class="flex items-center gap-1.5">
                  <span class="h-1 w-1 rounded-full bg-muted-foreground/30 animate-pulse" />
                  <span class="h-1 w-1 rounded-full bg-muted-foreground/30 animate-pulse [animation-delay:150ms]" />
                  <span class="h-1 w-1 rounded-full bg-muted-foreground/30 animate-pulse [animation-delay:300ms]" />
                </div>
              </div>
            }
          >
            <Show
              when={hasContent()}
              fallback={
                <div class="mx-auto flex w-full max-w-xl flex-col items-start gap-8 py-20">
                  {/* Engraved scope label — positioned like a typewriter slug */}
                  <div class="font-mono text-[10px] uppercase tracking-[0.18em] text-muted-foreground/40 flex items-center gap-2">
                    <span class="inline-block h-px w-6 bg-muted-foreground/30" />
                    <span>{props.threadId ? "Thread" : "Shepherd"}</span>
                  </div>

                  {/* Display name in display serif, no italics — feels like a chapter heading */}
                  <h3 class="font-display text-3xl font-normal tracking-tight text-foreground">
                    {emptyTitle()}
                  </h3>

                  {/* Body copy in proper reading width */}
                  <p class="max-w-md text-[13px] leading-[1.7] text-muted-foreground/70">{emptyBody()}</p>
                  <Show when={props.onSuggestion}>
                    <div class="mt-2 w-full">
                      <div class="font-mono text-[10px] uppercase tracking-[0.18em] text-muted-foreground/40 mb-3">
                        Try
                      </div>
                      <div class="flex flex-col">
                        <For each={suggestions()}>
                          {(suggestion, index) => (
                            <button
                              type="button"
                              class="group/sug flex items-center gap-3 border-t border-border/30 py-2.5 text-left text-[13px] text-muted-foreground transition-colors hover:text-foreground last:border-b"
                              onClick={() => props.onSuggestion?.(suggestion)}
                            >
                              <span class="font-mono text-[10px] tabular-nums text-muted-foreground/30 group-hover/sug:text-brand">
                                {String(index() + 1).padStart(2, "0")}
                              </span>
                              <span class="flex-1">{suggestion}</span>
                              <span class="font-mono text-[11px] text-muted-foreground/20 transition-all group-hover/sug:translate-x-1 group-hover/sug:text-brand">→</span>
                            </button>
                          )}
                        </For>
                      </div>
                    </div>
                  </Show>
                </div>
              }
            >
              <Show when={props.runtimeError}>
                {(banner) => (
                  <div
                    class="flex items-start justify-between gap-3 border border-signal-red/20 bg-signal-red/[0.05] px-4 py-3 text-sm text-signal-red"
                    title={banner().raw}
                  >
                    <span>{banner().message}</span>
                    <div class="flex shrink-0 items-center gap-3">
                      <Show when={banner().action === "open-settings"}>
                        <button
                          type="button"
                          class="text-xs font-medium text-signal-red transition-colors hover:text-signal-red/80"
                          onClick={() => props.onOpenSettings()}
                        >
                          Open Settings
                        </button>
                      </Show>
                      <button
                        type="button"
                        class="text-signal-red/50 transition-colors hover:text-signal-red"
                        onClick={() => props.onDismissRuntimeError(banner().raw)}
                        aria-label="Dismiss runtime error"
                        title="Dismiss"
                      >
                        <svg
                          viewBox="0 0 24 24"
                          class="h-3.5 w-3.5"
                          fill="none"
                          stroke="currentColor"
                          stroke-width="2"
                        >
                          <path d="M6 6l12 12" />
                          <path d="M18 6L6 18" />
                        </svg>
                      </button>
                    </div>
                  </div>
                )}
              </Show>

              <For each={props.messages}>
                {(message) => (
                  <ChatMessage
                    messageId={message.id}
                    role={message.role}
                    messageKind={message.message_kind}
                    previewText={message.preview_text}
                    collapsedByDefault={message.collapsed_by_default}
                    chunksJson={message.chunks_json}
                    timestamp={message.timestamp}
                  />
                )}
              </For>

              <Show when={props.liveTurn}>
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
          </Show>
        </div>
      </div>

      <Show when={!props.stickToBottom && hasContent()}>
        <button
          type="button"
          class="absolute bottom-20 right-6 z-10 flex h-8 items-center gap-1.5 border border-border bg-card px-3 font-mono text-[10px] uppercase tracking-wider text-muted-foreground shadow-sm transition-colors hover:border-brand/40 hover:text-foreground"
          onClick={props.onScrollToBottom}
          title="Scroll to the latest message"
          aria-label="Scroll to latest message"
        >
          <svg viewBox="0 0 24 24" class="h-3 w-3" fill="none" stroke="currentColor" stroke-width="1.8">
            <polyline points="6 9 12 15 18 9" />
          </svg>
          Latest
        </button>
      </Show>
    </div>
  );
};

export default ChatTranscript;
