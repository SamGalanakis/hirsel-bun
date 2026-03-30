import { type Component, createMemo, For, Show } from "solid-js";
import { cn } from "@/lib/cn";

interface Chunk {
  type: string;
  text?: string;
  tool_name?: string;
  tool_id?: string;
  [key: string]: unknown;
}

interface ChatMessageProps {
  role: string;
  chunksJson: string;
  timestamp: string;
}

function formatTime(ts: string): string {
  try {
    const d = new Date(ts);
    return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
  } catch {
    return "";
  }
}

/** Collapse consecutive tool-use chunks into a summary. */
function groupChunks(chunks: Chunk[]): Array<{ kind: "text"; html: string } | { kind: "tools"; count: number; names: string[] }> {
  const groups: Array<{ kind: "text"; html: string } | { kind: "tools"; count: number; names: string[] }> = [];
  let toolBatch: string[] = [];

  const flushTools = () => {
    if (toolBatch.length > 0) {
      groups.push({ kind: "tools", count: toolBatch.length, names: [...toolBatch] });
      toolBatch = [];
    }
  };

  for (const chunk of chunks) {
    const isTool =
      chunk.type === "tool_use" ||
      chunk.type === "tool_result" ||
      chunk.type === "server_tool_use";

    // Skip batch/orchestration tools
    if (isTool && chunk.tool_name?.startsWith("batch")) continue;

    if (isTool) {
      if (chunk.type === "tool_use" && chunk.tool_name) {
        toolBatch.push(chunk.tool_name);
      }
    } else {
      flushTools();
      const text = chunk.text ?? "";
      if (text) {
        groups.push({ kind: "text", html: text });
      }
    }
  }
  flushTools();
  return groups;
}

const ChatMessage: Component<ChatMessageProps> = (props) => {
  const parsed = createMemo(() => {
    try {
      return groupChunks(JSON.parse(props.chunksJson) as Chunk[]);
    } catch {
      return [{ kind: "text" as const, html: props.chunksJson }];
    }
  });

  const isUser = () => props.role === "user";
  const time = () => formatTime(props.timestamp);

  return (
    <div
      class={cn(
        "group relative px-4 py-3",
        isUser() && "bg-muted/40",
        !isUser() && "border-l-2 border-signal-amber",
      )}
    >
      <div class="flex items-baseline gap-2 mb-1">
        <span class="chassis-label">
          {isUser() ? "you" : "shepherd"}
        </span>
        <span
          class={cn(
            "text-[10px] text-muted-foreground",
            isUser() ? "ml-auto" : "",
          )}
        >
          {time()}
        </span>
      </div>

      <div class="space-y-1">
        <For each={parsed()}>
          {(group) => (
            <Show
              when={group.kind === "text"}
              fallback={
                <div class="flex items-center gap-1.5 text-xs text-muted-foreground py-0.5">
                  <svg class="h-3 w-3 shrink-0" viewBox="0 0 16 16" fill="none">
                    <path
                      d="M4 6l2 2 4-4M14 8A6 6 0 112 8a6 6 0 0112 0z"
                      stroke="currentColor"
                      stroke-width="1.5"
                    />
                  </svg>
                  <span>
                    Used {(group as { count: number }).count} tool
                    {(group as { count: number }).count === 1 ? "" : "s"}
                  </span>
                </div>
              }
            >
              <div
                class="markdown-body text-sm text-foreground leading-relaxed"
                innerHTML={(group as { html: string }).html}
              />
            </Show>
          )}
        </For>
      </div>
    </div>
  );
};

export default ChatMessage;
