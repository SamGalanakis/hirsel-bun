import { type Component, type JSX, For, Show, createMemo } from "solid-js";
import { createStore, produce } from "solid-js/store";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { cn } from "@/lib/cn";
import {
  displayUrl,
  formatToolJson,
  getToolDisplayKind,
  parseChatBlocks,
  parseToolInput,
  parseToolOutput,
  snippetText,
  toolLabel,
  type RenderBlock,
  type ToolChunk,
} from "@/lib/chat-message-view";
import { renderMarkdown } from "@/lib/markdown";
import { openUrl } from "@/lib/open-url";

const [expandedState, setExpandedState] = createStore<Record<string, boolean>>({});

function isExpanded(id: string): boolean {
  return !!expandedState[id];
}

function toggleExpanded(id: string) {
  setExpandedState(produce((draft) => {
    draft[id] = !draft[id];
  }));
}

function setExpanded(id: string, next: boolean) {
  setExpandedState(produce((draft) => {
    draft[id] = next;
  }));
}

function formatTime(timestamp: string): string {
  try {
    return new Date(timestamp).toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    });
  } catch {
    return "";
  }
}

function isSyncMessage(messageKind?: string): boolean {
  return messageKind === "shepherd_sync" || messageKind === "sync_prompt";
}

function liveStatusLabel(status?: string): string {
  switch ((status ?? "").toLowerCase()) {
    case "queued":
      return "Queued";
    case "starting":
      return "Starting";
    case "starting_container":
      return "Starting runtime";
    case "waiting_for_socket":
      return "Waiting for runtime";
    case "missing_artifact":
      return "Image missing";
    case "interrupting":
      return "Stopping";
    case "done":
    case "completed":
    case "success":
      return "Done";
    case "failed":
    case "error":
      return "Failed";
    default:
      return "Running";
  }
}

function toolStatusColor(status?: string): string {
  switch ((status ?? "").toLowerCase()) {
    case "running":
    case "active":
    case "queued":
    case "starting":
      return "text-signal-amber";
    case "done":
    case "completed":
    case "success":
      return "text-signal-green";
    case "failed":
    case "error":
      return "text-signal-red";
    default:
      return "text-muted-foreground";
  }
}

function toolKindLabel(tool: ToolChunk): string {
  switch (getToolDisplayKind(tool.title)) {
    case "shell":
      return "Terminal";
    case "patch":
      return "Edit";
    case "web-search":
      return "Web";
    case "fetch":
      return "Fetch";
    case "thread":
      return "Threads";
    case "task":
      return "Task";
    case "preview":
      return "Preview";
    case "plan":
      return "Plan";
    case "context":
      return "Context";
    case "exploration":
      return "Explore";
    case "canvas":
      return "Canvas";
    default:
      return "Tool";
  }
}

function toolKindIcon(tool: ToolChunk): JSX.Element {
  const props = {
    width: "12",
    height: "12",
    viewBox: "0 0 12 12",
    fill: "none",
    stroke: "currentColor",
    "stroke-width": "1.5",
    "stroke-linecap": "round" as const,
    "stroke-linejoin": "round" as const,
    "aria-hidden": true,
  };
  switch (getToolDisplayKind(tool.title)) {
    case "shell":
      return (
        <svg {...props}>
          <path d="M2.5 3.5L5 6L2.5 8.5" />
          <path d="M6.5 8.5H9.5" />
        </svg>
      );
    case "patch":
      return (
        <svg {...props}>
          <path d="M8 2.5L9.5 4L4 9.5L2 10L2.5 8L8 2.5Z" />
        </svg>
      );
    case "web-search":
      return (
        <svg {...props}>
          <circle cx="6" cy="6" r="4" />
          <path d="M2 6H10" />
          <path d="M6 2C7.5 3.5 7.5 8.5 6 10C4.5 8.5 4.5 3.5 6 2Z" />
        </svg>
      );
    case "fetch":
      return (
        <svg {...props}>
          <path d="M3.5 8.5L8.5 3.5" />
          <path d="M4.5 3.5H8.5V7.5" />
        </svg>
      );
    case "thread":
      return (
        <svg {...props}>
          <circle cx="3" cy="3" r="1" />
          <circle cx="3" cy="9" r="1" />
          <circle cx="9" cy="6" r="1" />
          <path d="M3 4V8" />
          <path d="M4 3H7C7.5 3 8 3.5 8 4V5.5" />
          <path d="M4 9H7C7.5 9 8 8.5 8 8V6.5" />
        </svg>
      );
    case "task":
      return (
        <svg {...props}>
          <rect x="1" y="1" width="10" height="10" rx="1" />
          <path d="M3.5 5.5L5 7L8.5 3.5" />
        </svg>
      );
    case "preview":
      return (
        <svg {...props}>
          <path d="M3.5 2.5L9 6L3.5 9.5V2.5Z" />
        </svg>
      );
    case "plan":
      return (
        <svg {...props}>
          <path d="M2.5 3.5L3.5 4.5L5 3" />
          <path d="M2.5 6.5L3.5 7.5L5 6" />
          <path d="M2.5 9.5L3.5 10.5L5 9" />
          <path d="M6.5 3.5H9.5" />
          <path d="M6.5 6.5H9.5" />
          <path d="M6.5 9.5H9.5" />
        </svg>
      );
    case "context":
      return (
        <svg {...props}>
          <path d="M3 2H7.5L9 3.5V10H3V2Z" />
          <path d="M7.5 2V3.5H9" />
          <path d="M4.5 5.5H7.5" />
          <path d="M4.5 7.5H7.5" />
        </svg>
      );
    case "exploration":
      return (
        <svg {...props}>
          <circle cx="6" cy="6" r="4" />
          <path d="M7.5 4.5L5.5 6.5L4.5 7.5L6.5 5.5L7.5 4.5Z" />
        </svg>
      );
    case "canvas":
      return (
        <svg {...props}>
          <rect x="2" y="2.5" width="8" height="7" rx="0.5" />
          <path d="M4 5H8" />
          <path d="M4 7H6.5" />
        </svg>
      );
    default:
      return (
        <svg {...props}>
          <circle cx="6" cy="6" r="2" />
        </svg>
      );
  }
}

function toolPreviewUrl(tool: ToolChunk): string | null {
  const output = parseToolOutput(tool);
  if (typeof output?.userUrl === "string") return output.userUrl;
  if (typeof output?.url === "string") return output.url;
  return null;
}

function summarizePatch(output: any): string {
  const files = Array.isArray(output?.files) ? output.files : [];
  if (files.length === 0) return "Updated files";
  if (files.length === 1) {
    const path = typeof files[0]?.path === "string" ? files[0].path : "1 file";
    return path;
  }
  return `${files.length} files updated`;
}

function toolSummary(tool: ToolChunk): string {
  const input = parseToolInput(tool);
  const output = parseToolOutput(tool);
  const displayKind = getToolDisplayKind(tool.title);

  switch (displayKind) {
    case "shell":
      return snippetText(input?.cmd ?? input?.command ?? toolLabel(tool.title), 120);
    case "patch":
      return summarizePatch(output);
    case "web-search":
      return snippetText(input?.q ?? input?.query ?? input?.url ?? toolLabel(tool.title), 120);
    case "fetch":
      return displayUrl(input?.url ?? output?.url ?? toolLabel(tool.title));
    case "thread":
      return snippetText(
        input?.thread_id ?? input?.title ?? input?.name ?? output?.thread_id ?? toolLabel(tool.title),
        120,
      );
    case "task":
      return snippetText(
        input?.task_id ?? input?.title ?? input?.summary ?? output?.task_id ?? toolLabel(tool.title),
        120,
      );
    case "preview": {
      const url = toolPreviewUrl(tool);
      return url ? displayUrl(url) : toolLabel(tool.title);
    }
    case "context":
      return snippetText(input?.path ?? input?.query ?? toolLabel(tool.title), 120);
    default:
      return snippetText(toolLabel(tool.title), 120);
  }
}

function blockPreview(blocks: RenderBlock[]): string {
  for (const block of blocks) {
    switch (block.kind) {
      case "text":
      case "notice":
      case "thinking":
        return snippetText(block.content, 180);
      case "tool":
        return `${toolKindLabel(block.tool)}: ${toolSummary(block.tool)}`;
      case "exploration":
        return `${block.tools.length} exploration step${block.tools.length === 1 ? "" : "s"}`;
      case "image":
        return block.name ? `Image: ${block.name}` : "Image";
      case "skill":
        return `Skill: ${block.name}`;
      case "fileRef":
        return block.path;
    }
  }
  return "";
}

function noticeToneClasses(tone: string): string {
  switch (tone.toLowerCase()) {
    case "error":
    case "danger":
      return "border border-signal-red/25 bg-signal-red/[0.06] text-signal-red";
    case "success":
      return "border border-signal-green/25 bg-signal-green/[0.06] text-signal-green";
    case "warning":
      return "border border-signal-amber/25 bg-signal-amber/[0.06] text-signal-amber";
    default:
      return "border border-border/50 bg-secondary/20 text-muted-foreground";
  }
}

const Chevron: Component<{ expanded: boolean }> = (props) => (
  <svg
    viewBox="0 0 16 16"
    class={cn("h-3.5 w-3.5 transition-transform", props.expanded && "rotate-180")}
    fill="none"
    stroke="currentColor"
    stroke-width="1.5"
    stroke-linecap="round"
    stroke-linejoin="round"
  >
    <path d="M4 6l4 4 4-4" />
  </svg>
);

const PlainTextBlock: Component<{ content: string; class?: string }> = (props) => (
  <div class={cn("whitespace-pre-wrap text-sm leading-relaxed", props.class)}>
    {props.content}
  </div>
);

const MarkdownBlock: Component<{ content: string }> = (props) => (
  <div
    class="markdown-body text-sm leading-relaxed"
    innerHTML={renderMarkdown(props.content)}
  />
);

const ToolDetails: Component<{ tool: ToolChunk }> = (props) => {
  const truncateJson = (raw: string, maxLines = 30) => {
    const formatted = formatToolJson(raw);
    const lines = formatted.split("\n");
    if (lines.length <= maxLines) return { text: formatted, overflow: 0 };
    return { text: lines.slice(0, maxLines).join("\n"), overflow: lines.length - maxLines };
  };

  return (
    <div class="grid gap-2 px-2.5 pb-2.5 pt-2 md:grid-cols-2">
      <Show when={props.tool.input}>
        {(input) => {
          const result = () => truncateJson(input());
          return (
            <div class="min-w-0">
              <div class="mb-1 font-mono text-[10px] uppercase tracking-[0.14em] text-muted-foreground/60">input</div>
              <div class="max-h-40 overflow-y-auto chassis-scroll">
                <pre class="whitespace-pre-wrap break-all bg-background/50 p-2 text-[10px] leading-[1.6] text-muted-foreground">
                  {result().text}
                </pre>
                <Show when={result().overflow > 0}>
                  <div class="px-2 pb-1 text-[9px] text-muted-foreground/40">
                    … {result().overflow} more lines
                  </div>
                </Show>
              </div>
            </div>
          );
        }}
      </Show>
      <Show when={props.tool.output}>
        {(output) => {
          const result = () => truncateJson(output());
          return (
            <div class="min-w-0">
              <div class="mb-1 font-mono text-[10px] uppercase tracking-[0.14em] text-muted-foreground/60">output</div>
              <div class="max-h-40 overflow-y-auto chassis-scroll">
                <pre class="whitespace-pre-wrap break-all bg-background/50 p-2 text-[10px] leading-[1.6] text-muted-foreground">
                  {result().text}
                </pre>
                <Show when={result().overflow > 0}>
                  <div class="px-2 pb-1 text-[9px] text-muted-foreground/40">
                    … {result().overflow} more lines
                  </div>
                </Show>
              </div>
            </div>
          );
        }}
      </Show>
    </div>
  );
};

const ToolCard: Component<{ tool: ToolChunk }> = (props) => {
  const cardId = createMemo(() => `tool:${props.tool.id}`);
  const open = createMemo(() => isExpanded(cardId()));
  const link = createMemo(() => toolPreviewUrl(props.tool));
  const hasDetails = createMemo(() => !!props.tool.input || !!props.tool.output);
  const isRunning = createMemo(() => {
    const s = props.tool.status.toLowerCase();
    return s === "running" || s === "active";
  });

  return (
    <Collapsible open={open()} onOpenChange={(next) => setExpanded(cardId(), next)}>
      <div class={cn(
        "border transition-colors",
        isRunning() ? "border-border/60" : "border-border/40",
      )}>
        <CollapsibleTrigger
          class={cn(
            "flex w-full items-center gap-2 px-2.5 py-1.5 text-left transition-colors",
            hasDetails() ? "hover:bg-secondary/40 cursor-pointer" : "cursor-default",
          )}
        >
          {/* Kind icon */}
          <span class="inline-flex h-3.5 w-3.5 shrink-0 items-center justify-center text-muted-foreground/70">
            <Show
              when={!isRunning()}
              fallback={
                <span class="block h-1.5 w-1.5 animate-pulse rounded-full bg-signal-amber" />
              }
            >
              {toolKindIcon(props.tool)}
            </Show>
          </span>

          {/* Kind label */}
          <span class={cn("shrink-0 font-mono text-[10px]", toolStatusColor(props.tool.status))}>
            {toolKindLabel(props.tool)}
          </span>

          {/* Summary */}
          <span class="min-w-0 flex-1 truncate text-xs text-foreground/80">
            {toolSummary(props.tool)}
          </span>

          {/* Open link + expand chevron */}
          <span class="ml-auto flex shrink-0 items-center gap-1.5">
            <Show when={link()}>
              {(previewUrl) => (
                <button
                  type="button"
                  class="font-mono text-[10px] text-muted-foreground/50 transition-colors hover:text-foreground"
                  onClick={(e) => { e.stopPropagation(); void openUrl(previewUrl()); }}
                >
                  open
                </button>
              )}
            </Show>
            <Show when={hasDetails()}>
              <span class={cn(
                "text-muted-foreground/40 transition-transform",
                open() && "rotate-180",
              )}>
                <Chevron expanded={open()} />
              </span>
            </Show>
          </span>
        </CollapsibleTrigger>
        <Show when={hasDetails()}>
          <CollapsibleContent class="overflow-hidden border-t border-border/30">
            <ToolDetails tool={props.tool} />
          </CollapsibleContent>
        </Show>
      </div>
    </Collapsible>
  );
};

const ExplorationBlock: Component<{ tools: ToolChunk[] }> = (props) => {
  const blockId = createMemo(() => `exploration:${props.tools[0]?.id ?? "batch"}`);
  const collapsed = createMemo(() => !isExpanded(blockId()));

  return (
    <div class="border border-border/40">
      <button
        type="button"
        class="flex w-full items-center gap-2 px-2.5 py-1.5 text-left transition-colors hover:bg-secondary/40"
        onClick={() => toggleExpanded(blockId())}
      >
        <span
          class="inline-flex h-3.5 w-3.5 shrink-0 items-center justify-center text-[9px] leading-none text-muted-foreground"
        >◇</span>
        <span class="font-mono text-[10px] text-muted-foreground">
          {props.tools.length} exploration step{props.tools.length === 1 ? "" : "s"}
        </span>
        <span class={cn(
          "ml-auto text-muted-foreground/40 transition-transform",
          !collapsed() && "rotate-180",
        )}>
          <Chevron expanded={!collapsed()} />
        </span>
      </button>
      <Show when={!collapsed()}>
        <div class="space-y-0.5 border-t border-border/30 p-1">
          <For each={props.tools}>{(tool) => <ToolCard tool={tool} />}</For>
        </div>
      </Show>
    </div>
  );
};

function renderBlock(block: RenderBlock, mode: "user" | "assistant" | "system"): JSX.Element {
  switch (block.kind) {
    case "text":
      if (mode === "assistant") return <MarkdownBlock content={block.content} />;
      if (mode === "user") return <PlainTextBlock content={block.content} class="text-foreground" />;
      return <PlainTextBlock content={block.content} class="text-foreground" />;
    case "notice":
      return (
        <div class={cn("px-3 py-2 text-sm leading-relaxed", noticeToneClasses(block.tone))}>
          <Show when={block.title}>
            <div class="mb-0.5 font-mono text-[10px] uppercase tracking-wider">{block.title}</div>
          </Show>
          <PlainTextBlock content={block.content} class="text-current text-xs" />
        </div>
      );
    case "thinking": {
      const thinkId = `thinking:${block.content.slice(0, 32)}`;
      const thinkOpen = isExpanded(thinkId);
      return (
        <div>
          <button
            type="button"
            class="flex items-center gap-1.5 font-mono text-[10px] text-muted-foreground/50 transition-colors hover:text-muted-foreground"
            onClick={() => toggleExpanded(thinkId)}
          >
            <Chevron expanded={thinkOpen} />
            <span>reasoning</span>
            <Show when={!thinkOpen}>
              <span class="max-w-[200px] truncate text-muted-foreground/30">
                {snippetText(block.content, 60)}
              </span>
            </Show>
          </button>
          <Show when={thinkOpen}>
            <div class="mt-1.5 pl-4 text-muted-foreground/70">
              <PlainTextBlock content={block.content} class="text-xs leading-relaxed" />
            </div>
          </Show>
        </div>
      );
    }
    case "exploration":
      return <ExplorationBlock tools={block.tools} />;
    case "tool":
      return <ToolCard tool={block.tool} />;
    case "image":
      return (
        <div class="overflow-hidden border border-border/40">
          <img src={block.src} alt={block.name ?? "image"} class="max-h-[28rem] w-full object-contain" />
        </div>
      );
    case "skill":
      return (
        <div class="flex items-center gap-2 border border-signal-blue/20 bg-signal-blue/[0.04] px-2.5 py-1.5 font-mono text-[10px]">
          <span class="text-signal-blue">/{block.name}</span>
          <Show when={block.description}>
            <span class="text-muted-foreground/50">{block.description}</span>
          </Show>
        </div>
      );
    case "fileRef": {
      const location = [block.lineStart, block.lineEnd].filter((value) => typeof value === "number");
      const suffix =
        location.length === 0
          ? ""
          : location.length === 1
            ? `:${location[0]}`
            : `:${location[0]}-${location[1]}`;
      return (
        <div class="font-mono text-xs text-muted-foreground">
          {block.path}{suffix}
        </div>
      );
    }
  }
}

const ChatMessage: Component<{
  messageId?: number | string;
  role: string;
  messageKind?: string;
  previewText?: string | null;
  collapsedByDefault?: boolean;
  chunksJson: string;
  timestamp: string;
  liveStatus?: string;
}> = (props) => {
  const blocks = createMemo(() => parseChatBlocks(props.chunksJson, !!props.liveStatus));
  const systemMessage = createMemo(() => isSyncMessage(props.messageKind));
  const userMessage = createMemo(() => props.role === "user" && !systemMessage());
  const mode = createMemo<"user" | "assistant" | "system">(() => {
    if (systemMessage()) return "system";
    if (userMessage()) return "user";
    return "assistant";
  });
  const messageKey = createMemo(() => {
    if (props.liveStatus) return null;
    if (props.messageId !== undefined && props.messageId !== null) return `message:${props.messageId}`;
    return `message:${props.role}:${props.timestamp}:${props.messageKind ?? "chat"}`;
  });
  const canCollapse = createMemo(
    () => !props.liveStatus && !!messageKey() && (!!props.collapsedByDefault || systemMessage()),
  );
  const collapsed = createMemo(() => {
    const key = messageKey();
    if (!key || !canCollapse()) return false;
    return !isExpanded(key);
  });
  const preview = createMemo(() => {
    const explicit = props.previewText?.trim();
    if (explicit) return explicit;
    return blockPreview(blocks());
  });
  const showStatusOnly = createMemo(
    () =>
      !!props.liveStatus &&
      (blocks().length === 0
        || ["queued", "starting", "starting_container", "waiting_for_socket", "missing_artifact"].includes(
          props.liveStatus,
        )),
  );

  // Role label — shown as a tiny engraved slug, not gutter mark
  const roleDisplay = createMemo(() => {
    if (systemMessage()) return "System";
    if (userMessage()) return "You";
    return "Shepherd";
  });

  const roleToneClass = createMemo(() => {
    if (systemMessage()) return "text-signal-amber/75";
    if (userMessage()) return "text-brand";
    return "text-muted-foreground/70";
  });

  return (
    <div
      class={cn(
        "group relative py-4",
        // subtle top border for turn separation, except first
        "border-t border-border/25 first:border-t-0",
      )}
    >
      {/* Engraved role slug — tiny uppercase meta at the top of the message */}
      <div class="mb-1.5 flex items-center gap-2 select-none">
        <span class={cn(
          "font-mono text-[10px] uppercase tracking-[0.16em]",
          roleToneClass(),
        )}>
          {roleDisplay()}
        </span>
        <Show when={props.liveStatus}>
          <span class="text-muted-foreground/20">·</span>
          <span class={cn("font-mono text-[10px] uppercase tracking-[0.12em]", toolStatusColor(props.liveStatus))}>
            {liveStatusLabel(props.liveStatus)}
          </span>
        </Show>
        <span class="ml-auto font-mono text-[10px] tabular-nums text-muted-foreground/25 opacity-0 transition-opacity group-hover:opacity-100">
          {formatTime(props.timestamp)}
        </span>
        <Show when={canCollapse()}>
          <button
            type="button"
            class="inline-flex items-center gap-0.5 text-muted-foreground/30 opacity-0 transition-all hover:text-foreground group-hover:opacity-100"
            onClick={() => {
              const key = messageKey();
              if (key) toggleExpanded(key);
            }}
          >
            <Chevron expanded={!collapsed()} />
          </button>
        </Show>
      </div>

      {/* Body */}
      <Show
        when={!collapsed()}
        fallback={
          <div class="truncate whitespace-pre-wrap text-xs leading-relaxed text-muted-foreground/50">
            {preview() || "No preview"}
          </div>
        }
      >
        <div class={cn(
          "space-y-2.5",
          userMessage() && "text-foreground font-medium",
          systemMessage() && "text-muted-foreground/80",
        )}>
          <Show when={showStatusOnly()}>
            <div class="flex items-center gap-1.5 py-1">
              <span class="h-1 w-1 animate-pulse rounded-full bg-brand" />
              <span class="h-1 w-1 animate-pulse rounded-full bg-brand [animation-delay:150ms]" />
              <span class="h-1 w-1 animate-pulse rounded-full bg-brand [animation-delay:300ms]" />
            </div>
          </Show>

          <Show when={blocks().length > 0} fallback={<div class="text-xs text-muted-foreground/50">No content</div>}>
            <For each={blocks()}>{(block) => renderBlock(block, mode())}</For>
          </Show>
        </div>
      </Show>
    </div>
  );
};

export default ChatMessage;
