import {
  type Component,
  createEffect,
  createSignal,
  For,
  on,
  onMount,
  Show,
} from "solid-js";
import Button from "@/components/ui/button";
import Textarea from "@/components/ui/textarea";
import ChatMessageComponent from "@/components/ChatMessage";
import type { ChatMessage, LiveTurn } from "@/lib/api";

interface ChatPanelProps {
  messages: ChatMessage[];
  liveTurn: LiveTurn | null;
  isRunning: boolean;
  onSend: (content: string) => void;
  onStop: () => void;
}

const ChatPanel: Component<ChatPanelProps> = (props) => {
  const [input, setInput] = createSignal("");
  const [stickToBottom, setStickToBottom] = createSignal(true);
  let scrollRef!: HTMLDivElement;
  let inputRef!: HTMLTextAreaElement;

  const isNearBottom = () => {
    if (!scrollRef) return true;
    const { scrollTop, scrollHeight, clientHeight } = scrollRef;
    return scrollHeight - scrollTop - clientHeight < 80;
  };

  const scrollToBottom = () => {
    if (!scrollRef) return;
    scrollRef.scrollTop = scrollRef.scrollHeight;
  };

  const updateStickinessFromScroll = () => {
    setStickToBottom(isNearBottom());
  };

  onMount(() => {
    scrollToBottom();
    setStickToBottom(true);
  });

  createEffect(
    on(
      () => [
        props.messages.length,
        props.messages.at(-1)?.id ?? null,
        props.liveTurn?.chunks_json ?? null,
        props.liveTurn?.status ?? null,
      ],
      () => {
        if (stickToBottom()) {
          scrollToBottom();
        }
      },
    ),
  );

  createEffect(
    on(
      () => props.messages[0]?.id ?? null,
      () => {
        scrollToBottom();
        setStickToBottom(true);
      },
    ),
  );

  // Auto-grow textarea
  const autoGrow = () => {
    if (!inputRef) return;
    inputRef.style.height = "auto";
    inputRef.style.height = `${Math.min(inputRef.scrollHeight, 160)}px`;
  };

  const handleSubmit = (e: Event) => {
    e.preventDefault();
    const content = input().trim();
    if (!content) return;
    setStickToBottom(true);
    props.onSend(content);
    setInput("");
    if (inputRef) { inputRef.style.height = "auto"; }
    scrollToBottom();
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSubmit(e);
    }
  };

  const handleStop = () => {
    props.onStop();
  };

  const placeholder = () =>
    props.isRunning ? "Send a follow-up..." : "Send a message...";

  const isEmpty = () => props.messages.length === 0 && !props.liveTurn;

  return (
    <div class="flex flex-1 min-h-0 flex-col overflow-hidden">
      {/* Message list */}
      <div
        ref={scrollRef}
        class="flex flex-1 min-h-0 flex-col overflow-y-auto"
        onScroll={updateStickinessFromScroll}
      >
        <Show
          when={!isEmpty()}
          fallback={
            <div class="flex flex-1 items-center justify-center text-muted-foreground text-sm">
              Send a message to get started
            </div>
          }
        >
          <div class="w-full divide-y divide-border">
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
      </div>

      {/* Status bar */}
      <Show when={props.isRunning}>
        <div class="flex items-center gap-2 px-4 py-2 border-t border-border bg-muted/30">
          <span class="h-2 w-2 rounded-full bg-signal-green animate-pulse-dot" />
          <span class="text-xs text-muted-foreground font-body">Working</span>
          <span class="ml-auto text-[10px] text-muted-foreground font-mono">
            Esc to stop &middot; Enter to send follow-up
          </span>
        </div>
      </Show>

      {/* Input form */}
      <form
        class="shrink-0 border-t border-border bg-background px-4 py-3"
        onSubmit={handleSubmit}
      >
        <div class="flex items-end gap-2">
          <Textarea
            ref={inputRef}
            class="min-h-[38px] max-h-[160px] flex-1 resize-none border-0 bg-transparent py-2 shadow-none focus-visible:ring-0 focus-visible:ring-offset-0"
            placeholder={placeholder()}
            value={input()}
            onInput={(e) => { setInput(e.currentTarget.value); autoGrow(); }}
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
                onClick={handleStop}
              >
                Stop
              </Button>
            }
          >
            <Button variant="primary" size="sm" type="submit" disabled={!input().trim()}>
              Send
            </Button>
          </Show>
        </div>
      </form>
    </div>
  );
};

export default ChatPanel;
