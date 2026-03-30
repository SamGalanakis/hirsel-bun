import {
  type Component,
  createEffect,
  createSignal,
  For,
  on,
  Show,
} from "solid-js";
import { cn } from "@/lib/cn";
import Button from "@/components/ui/button";
import ChatMessageComponent from "@/components/ChatMessage";
import type { ChatMessage, LiveTurn } from "@/lib/api";

interface ChatPanelProps {
  messages: ChatMessage[];
  liveTurn: LiveTurn | null;
  isRunning: boolean;
  hasQueued: boolean;
  onSend: (content: string) => void;
  onStop: () => void;
}

const ChatPanel: Component<ChatPanelProps> = (props) => {
  const [input, setInput] = createSignal("");
  let bottomRef!: HTMLDivElement;
  let inputRef!: HTMLTextAreaElement;

  // Auto-scroll on new messages or live turn updates
  createEffect(
    on(
      () => [props.messages.length, props.liveTurn?.updated_at],
      () => {
        bottomRef?.scrollIntoView({ behavior: "smooth" });
      },
    ),
  );

  const handleSubmit = (e: Event) => {
    e.preventDefault();
    const content = input().trim();
    if (!content) return;
    props.onSend(content);
    setInput("");
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSubmit(e);
    }
  };

  const placeholder = () =>
    props.isRunning ? "Queue a follow-up..." : "Send a message...";

  const buttonLabel = () => {
    if (props.isRunning) return "Queue";
    return "Send";
  };

  const isEmpty = () => props.messages.length === 0 && !props.liveTurn;

  return (
    <div class="flex flex-col h-full">
      {/* Message list */}
      <div class="flex-1 overflow-y-auto min-h-0">
        <Show
          when={!isEmpty()}
          fallback={
            <div class="flex items-center justify-center h-full text-muted-foreground text-sm">
              Send a message to get started
            </div>
          }
        >
          <div class="divide-y divide-border">
            <For each={props.messages}>
              {(msg) => (
                <ChatMessageComponent
                  role={msg.role}
                  chunksJson={msg.chunks_json}
                  timestamp={msg.timestamp}
                />
              )}
            </For>

            {/* Live turn (streaming) */}
            <Show when={props.liveTurn}>
              {(turn) => (
                <ChatMessageComponent
                  role="assistant"
                  chunksJson={turn().chunks_json}
                  timestamp={turn().updated_at}
                />
              )}
            </Show>
          </div>
        </Show>
        <div ref={bottomRef} />
      </div>

      {/* Status bar */}
      <Show when={props.isRunning}>
        <div class="flex items-center gap-2 px-4 py-2 border-t border-border bg-muted/30">
          <span class="h-2 w-2 rounded-full bg-signal-green animate-pulse-dot" />
          <span class="text-xs text-muted-foreground font-body">Working</span>
          <span class="ml-auto text-[10px] text-muted-foreground font-mono">
            Esc stop &middot; Enter queue
          </span>
        </div>
      </Show>

      {/* Input form */}
      <form
        class="shrink-0 border-t border-border bg-background px-4 py-3"
        onSubmit={handleSubmit}
      >
        <div class="flex items-end gap-2">
          <textarea
            ref={inputRef}
            class={cn(
              "flex-1 resize-none bg-transparent text-sm text-foreground font-body",
              "placeholder:text-muted-foreground focus:outline-none",
              "min-h-[38px] max-h-[160px] py-2",
            )}
            placeholder={placeholder()}
            value={input()}
            onInput={(e) => setInput(e.currentTarget.value)}
            onKeyDown={handleKeyDown}
            rows={1}
          />
          <Show
            when={!props.isRunning}
            fallback={
              <Button
                variant="destructive"
                size="sm"
                type="button"
                onClick={() => props.onStop()}
              >
                Stop
              </Button>
            }
          >
            <Button variant="primary" size="sm" type="submit" disabled={!input().trim()}>
              {buttonLabel()}
            </Button>
          </Show>
        </div>
      </form>
    </div>
  );
};

export default ChatPanel;
