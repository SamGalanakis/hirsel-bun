import { type Component, type JSX, createMemo, For, Index, Show } from "solid-js";
import { createStore, produce } from "solid-js/store";
import { cn } from "@/lib/cn";
import { Collapsible, CollapsibleTrigger, CollapsibleContent } from "@/components/ui/collapsible";
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

/* ── Module-level expand state (survives component re-creation from polling) ── */

const [expandedTools, setExpandedTools] = createStore<Record<string, boolean>>({});
function toggle(id: string) { setExpandedTools(produce((s) => { s[id] = !s[id]; })); }
function isExpanded(id: string): boolean { return !!expandedTools[id]; }

/* ── Status rendering ── */

function statusDot(status: string | undefined): string {
  switch ((status ?? "").toLowerCase()) {
    case "running": case "active": return "bg-signal-amber animate-pulse-dot";
    case "done": case "completed": case "success": return "bg-signal-green";
    case "failed": case "error": return "bg-signal-red";
    default: return "bg-muted-foreground/40";
  }
}

function isFailed(status: string | undefined): boolean {
  const s = (status ?? "").toLowerCase();
  return s === "failed" || s === "error";
}

/* ── SVG tool icons (12px display, 16px viewBox) ── */

const svgBase = "h-3 w-3";
const svgAttrs = { viewBox: "0 0 16 16", fill: "none", stroke: "currentColor", "stroke-width": "1.5", "stroke-linecap": "round", "stroke-linejoin": "round" } as const;

function IcoFile() {
  return <svg class={svgBase} {...svgAttrs}><path d="M4 2h5l3 3v9H4z"/><path d="M9 2v3h3"/></svg>;
}
function IcoSearch() {
  return <svg class={svgBase} {...svgAttrs}><circle cx="7" cy="7" r="3.5"/><path d="M10 10l3 3"/></svg>;
}
function IcoList() {
  return <svg class={svgBase} {...svgAttrs}><path d="M3 4h10M3 8h10M3 12h6"/></svg>;
}
function IcoPatch() {
  return <svg class={svgBase} {...svgAttrs}><path d="M3 13l1-4L12 1l2 2-8 8z"/></svg>;
}
function IcoGlobe() {
  return <svg class={svgBase} {...svgAttrs}><circle cx="8" cy="8" r="5.5"/><path d="M2.5 8h11"/><ellipse cx="8" cy="8" rx="2.5" ry="5.5"/></svg>;
}
function IcoLink() {
  return <svg class={svgBase} {...svgAttrs}><path d="M5 11L11 5M7 5h4v4"/></svg>;
}
function IcoThread() {
  return <svg class={svgBase} {...svgAttrs}><path d="M3 3h10v7H7l-3 3v-3H3z"/></svg>;
}
function IcoPencil() {
  return <svg class={svgBase} {...svgAttrs}><path d="M10 3l3 3-8 8H2v-3z"/></svg>;
}
function IcoChecklist() {
  return <svg class={svgBase} {...svgAttrs}><rect x="2" y="2" width="12" height="12"/><path d="M5 8l2 2 4-4"/></svg>;
}
function IcoFrame() {
  return <svg class={svgBase} {...svgAttrs}><rect x="2" y="2" width="12" height="12"/><path d="M2 6h12M6 2v12"/></svg>;
}
function IcoLayers() {
  return <svg class={svgBase} {...svgAttrs}><path d="M8 2l6 3.5L8 9 2 5.5z"/><path d="M2 8l6 3.5L14 8"/><path d="M2 11l6 3 6-3"/></svg>;
}
function IcoCircle() {
  return <svg class={svgBase} viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="8" cy="8" r="4.5"/></svg>;
}
function IcoCompass() {
  return <svg class={svgBase} {...svgAttrs}><circle cx="8" cy="8" r="5.5"/><path d="M10.5 5.5l-2 3-3 1.5 2-3z" fill="currentColor"/></svg>;
}
function IcoSlash() {
  return <svg class={svgBase} {...svgAttrs}><path d="M11.5 2.5L4.5 13.5"/></svg>;
}

function iconForTool(name: string): () => JSX.Element {
  switch (name) {
    case "list_threads":
    case "create_thread":
    case "rename_thread":
    case "set_thread_status":
    case "archive_thread":
    case "delete_thread":
    case "send_thread_message":
    case "read_thread_updates":
      return IcoThread;
    case "list_workspace":
    case "ls":
      return IcoList;
    case "read_workspace_file":
    case "read_file":
      return IcoFile;
    case "grep_workspace":
    case "grep":
    case "glob":
      return IcoSearch;
    default: {
      switch (getToolDisplayKind(name)) {
        case "plan":
          return IcoChecklist;
        case "shell":
          return IcoCircle;
        case "patch":
          return IcoPatch;
        case "web-search":
        case "preview":
          return IcoGlobe;
        case "fetch":
          return IcoLink;
        case "canvas":
          return IcoFrame;
        case "context":
          return IcoLayers;
        default:
          return IcoCircle;
      }
    }
  }
}

function renderToolIcon(name: string): JSX.Element {
  const Icon = iconForTool(name);
  return <Icon />;
}

/* ── Shared styles ── */

const toolTrigger = "flex w-full items-center gap-2 px-1.5 py-1 -mx-1.5 text-left text-muted-foreground transition-colors hover:bg-secondary/30";
const toolPanel = "mt-1 ml-5 border border-border/40 bg-background/60 overflow-hidden";

/* ── Expandable detail panel ── */

const DetailPanel: Component<{ tool: ToolChunk; borderColor?: string }> = (props) => (
  <div class={cn(toolPanel, props.borderColor && `border-l-2 ${props.borderColor}`)}>
    <Show when={props.tool.input}>
      <div class="px-2.5 pt-2">
        <span class="chassis-label">Input</span>
        <pre class="mt-1 font-mono text-[11px] text-muted-foreground whitespace-pre-wrap break-all max-h-48 overflow-y-auto">
          {formatToolJson(props.tool.input!)}
        </pre>
      </div>
    </Show>
    <Show when={props.tool.output}>
      <div class={props.tool.input ? "border-t border-border/30 px-2.5 py-2" : "px-2.5 py-2"}>
        <span class="chassis-label">Output</span>
        <pre class="mt-1 font-mono text-[11px] text-muted-foreground whitespace-pre-wrap break-all max-h-80 overflow-y-auto">
          {formatToolJson(props.tool.output!)}
        </pre>
      </div>
    </Show>
  </div>
);

/* ── Chevron ── */

const Chevron: Component<{ open: boolean }> = (props) => (
  <svg
    class={cn("h-3 w-3 shrink-0 text-muted-foreground transition-transform", props.open && "rotate-180")}
    viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
  ><polyline points="6 9 12 15 18 9" /></svg>
);

/* ═══════════════════════════════════════
   Tool-specific renderers
   ═══════════════════════════════════════ */

/* ── Shell command (exec_command) ── */

const ShellBlock: Component<{ tool: ToolChunk }> = (props) => {
  const inp = () => parseToolInput(props.tool);
  const out = () => parseToolOutput(props.tool);
  const cmd = () => inp()?.cmd ?? inp()?.command ?? "command";
  const targetKind = () => out()?.target_kind ?? null;
  const threadId = () => out()?.thread_id ?? inp()?.thread_id ?? null;
  const exitCode = () => out()?.exit_code ?? null;
  const shellOutput = () => out()?.output ?? null;
  const failed = () => isFailed(props.tool.status) || (exitCode() !== null && exitCode() !== 0);
  const expanded = () => isExpanded(props.tool.id);

  return (
    <div class="text-xs">
      <button
        type="button"
        class={cn(toolTrigger, "hover:text-foreground")}
        onClick={() => toggle(props.tool.id)}
      >
        <span class={cn("h-1.5 w-1.5 rounded-full shrink-0", failed() ? "bg-signal-red" : statusDot(props.tool.status))} />
        <span class={cn("font-mono text-xs shrink-0", failed() ? "text-signal-red" : "text-muted-foreground")}>$</span>
        <span class="font-mono text-xs truncate">{snippetText(cmd(), 80)}</span>
        <Show when={targetKind() === "thread" && threadId()}>
          <span class="shrink-0 border border-signal-blue/30 bg-signal-blue/10 px-1.5 py-0.5 font-mono text-[10px] text-signal-blue">
            @{threadId()}
          </span>
        </Show>
        <Show when={exitCode() !== null && exitCode() !== 0}>
          <span class="font-mono text-[11px] text-signal-red shrink-0">exit {exitCode()}</span>
        </Show>
        <span class="ml-auto"><Chevron open={expanded()} /></span>
      </button>
      <Show when={expanded() && shellOutput()}>
        <div class={cn(toolPanel, failed() && "border-l-2 border-l-signal-red/40")}>
          <pre class="p-2.5 font-mono text-[11px] text-muted-foreground whitespace-pre-wrap break-all max-h-80 overflow-y-auto">
            {shellOutput()}
          </pre>
        </div>
      </Show>
    </div>
  );
};

/* ── Patch / edit (apply_patch) ── */

const PatchBlock: Component<{ tool: ToolChunk }> = (props) => {
  const out = () => parseToolOutput(props.tool);
  const expanded = () => isExpanded(props.tool.id);

  const files = (): Array<{ path: string; status: string; added: number; removed: number; diff?: string }> => {
    const o = out();
    if (!o?.files) return [];
    return o.files.map((f: any) => ({
      path: f.path ?? "file",
      status: f.status ?? "modified",
      added: f.added ?? 0,
      removed: f.removed ?? 0,
      diff: f.diff,
    }));
  };

  const summary = () => {
    const f = files();
    if (f.length === 0) return "apply patch";
    if (f.length === 1) {
      const fi = f[0];
      const verb = fi.status === "added" ? "Created" : fi.status === "deleted" ? "Deleted" : "Edited";
      return `${verb} ${fi.path} (+${fi.added} -${fi.removed})`;
    }
    const added = f.reduce((s, fi) => s + fi.added, 0);
    const removed = f.reduce((s, fi) => s + fi.removed, 0);
    return `Edited ${f.length} files (+${added} -${removed})`;
  };

  return (
    <div class="text-xs">
      <button
        type="button"
        class={cn(toolTrigger, "hover:text-foreground")}
        onClick={() => toggle(props.tool.id)}
      >
        <span class={cn("h-1.5 w-1.5 rounded-full shrink-0", statusDot(props.tool.status))} />
        <span class="shrink-0 text-signal-green"><IcoPatch /></span>
        <span class="font-body text-xs">{summary()}</span>
        <span class="ml-auto"><Chevron open={expanded()} /></span>
      </button>
      <Show when={expanded()}>
        <div class={cn(toolPanel, "border-l-2 border-l-signal-green/30")}>
          <For each={files()}>
            {(f) => (
              <div class="border-b border-border/30 last:border-b-0">
                <div class="flex items-center gap-2 bg-secondary/30 px-2.5 py-1.5">
                  <span class="font-mono text-[11px] font-medium text-foreground">{f.path}</span>
                  <span class="font-mono text-[11px] text-signal-green">+{f.added}</span>
                  <span class="font-mono text-[11px] text-signal-red">-{f.removed}</span>
                </div>
                <Show when={f.diff}>
                  <pre class="px-2.5 py-1.5 font-mono text-[11px] whitespace-pre-wrap break-all max-h-64 overflow-y-auto">
                    <For each={f.diff!.split("\n")}>
                      {(line) => {
                        const color = line.startsWith("+") ? "text-signal-green"
                          : line.startsWith("-") ? "text-signal-red"
                          : line.startsWith("@@") ? "text-signal-blue"
                          : "text-muted-foreground";
                        return <div class={color}>{line}</div>;
                      }}
                    </For>
                  </pre>
                </Show>
              </div>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
};

/* ── Web search (search_web) ── */

const WebSearchBlock: Component<{ tool: ToolChunk }> = (props) => {
  const inp = () => parseToolInput(props.tool);
  const out = () => parseToolOutput(props.tool);
  const query = () => inp()?.query ?? "web";
  const expanded = () => isExpanded(props.tool.id);
  const results = (): Array<{ title: string; url: string }> => {
    const o = out();
    if (!o?.results) return [];
    return o.results.slice(0, 5).map((r: any) => ({ title: r.title ?? "", url: r.url ?? "" }));
  };
  const answer = () => out()?.answer ?? null;

  return (
    <div class="text-xs">
      <button
        type="button"
        class={cn(toolTrigger, "hover:text-foreground")}
        onClick={() => toggle(props.tool.id)}
      >
        <span class={cn("h-1.5 w-1.5 rounded-full shrink-0", statusDot(props.tool.status))} />
        <span class="shrink-0"><IcoGlobe /></span>
        <span class="font-body text-xs">searched "{snippetText(query(), 50)}"</span>
        <span class="ml-auto"><Chevron open={expanded()} /></span>
      </button>
      <Show when={expanded()}>
        <div class={cn(toolPanel, "p-2.5 space-y-1")}>
          <Show when={answer()}>
            <div class="text-[11px] text-muted-foreground leading-relaxed">{snippetText(answer()!, 200)}</div>
          </Show>
          <For each={results()}>
            {(r) => (
              <div class="flex items-baseline gap-1.5 text-[11px]">
                <span class="truncate text-foreground/80">{r.title ? snippetText(r.title, 48) : ""}</span>
                <Show when={r.url}>
                  <span class="shrink-0 font-mono text-muted-foreground/50">{displayUrl(r.url)}</span>
                </Show>
              </div>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
};

/* ── Fetch URL (fetch_url) ── */

const FetchBlock: Component<{ tool: ToolChunk }> = (props) => {
  const inp = () => parseToolInput(props.tool);
  const out = () => parseToolOutput(props.tool);
  const url = () => inp()?.url ?? "url";
  const expanded = () => isExpanded(props.tool.id);
  const text = () => {
    const o = out();
    if (typeof o === "string") return o;
    return o?.text ?? o?.answer ?? o?.content ?? null;
  };

  return (
    <div class="text-xs">
      <button
        type="button"
        class={cn(toolTrigger, "hover:text-foreground")}
        onClick={() => toggle(props.tool.id)}
      >
        <span class={cn("h-1.5 w-1.5 rounded-full shrink-0", statusDot(props.tool.status))} />
        <span class="shrink-0"><IcoLink /></span>
        <span class="font-body text-xs">fetch {displayUrl(url())}</span>
        <Show when={text()}>
          <span class="ml-auto"><Chevron open={expanded()} /></span>
        </Show>
      </button>
      <Show when={expanded() && text()}>
        <div class={toolPanel}>
          <pre class="p-2.5 font-mono text-[11px] text-muted-foreground whitespace-pre-wrap break-all max-h-48 overflow-y-auto">
            {snippetText(text()!, 3000)}
          </pre>
        </div>
      </Show>
    </div>
  );
};

/* ── Thread tools ── */

const ThreadBlock: Component<{ tool: ToolChunk }> = (props) => {
  const expanded = () => isExpanded(props.tool.id);
  const hasDetail = () => !!(props.tool.input || props.tool.output);
  const out = () => parseToolOutput(props.tool);
  const inp = () => parseToolInput(props.tool);
  const threadTitle = () => out()?.thread?.title ?? inp()?.title ?? inp()?.new_title ?? null;
  const threadStatus = () => out()?.thread?.status ?? inp()?.status ?? null;

  return (
    <div class="text-xs">
      <button
        type="button"
        class={cn(toolTrigger, "text-signal-blue/80 hover:text-foreground")}
        onClick={() => hasDetail() && toggle(props.tool.id)}
        disabled={!hasDetail()}
      >
        <span class={cn("h-1.5 w-1.5 rounded-full shrink-0", statusDot(props.tool.status))} />
        <span class="flex w-3.5 items-center justify-center shrink-0">{renderToolIcon(props.tool.title)}</span>
        <span class="font-body text-xs">{toolLabel(props.tool.title)}</span>
        <Show when={threadTitle()}>
          <span class="font-medium text-foreground truncate">{threadTitle()}</span>
        </Show>
        <Show when={threadStatus()}>
          <span class="font-mono text-[11px] text-muted-foreground">{threadStatus()}</span>
        </Show>
        <Show when={hasDetail()}>
          <span class="ml-auto"><Chevron open={expanded()} /></span>
        </Show>
      </button>
      <Show when={expanded()}>
        <DetailPanel tool={props.tool} borderColor="border-l-signal-blue/20" />
      </Show>
    </div>
  );
};

/* ── Canvas update ── */

const CanvasBlock: Component<{ tool: ToolChunk }> = (props) => {
  const isUpdate = () => props.tool.title.startsWith("update");
  const expanded = () => isExpanded(props.tool.id);
  const hasDetail = () => !!(props.tool.input || props.tool.output);

  return (
    <div class="text-xs">
      <button
        type="button"
        class={cn(toolTrigger, "hover:text-foreground")}
        onClick={() => hasDetail() && toggle(props.tool.id)}
        disabled={!hasDetail()}
      >
        <span class={cn("h-1.5 w-1.5 rounded-full shrink-0", statusDot(props.tool.status))} />
        <span class="shrink-0"><IcoFrame /></span>
        <span class="font-body text-xs">{isUpdate() ? "Updated canvas" : "Read canvas"}</span>
        <Show when={hasDetail()}>
          <span class="ml-auto"><Chevron open={expanded()} /></span>
        </Show>
      </button>
      <Show when={expanded()}>
        <DetailPanel tool={props.tool} borderColor="border-l-signal-green/30" />
      </Show>
    </div>
  );
};

/* ── Context update ── */

const ContextBlock: Component<{ tool: ToolChunk }> = (props) => {
  const isUpdate = () => props.tool.title.startsWith("update");
  const expanded = () => isExpanded(props.tool.id);
  const hasDetail = () => !!(props.tool.input || props.tool.output);

  return (
    <div class="text-xs">
      <button
        type="button"
        class={cn(toolTrigger, "hover:text-foreground")}
        onClick={() => hasDetail() && toggle(props.tool.id)}
        disabled={!hasDetail()}
      >
        <span class={cn("h-1.5 w-1.5 rounded-full shrink-0", statusDot(props.tool.status))} />
        <span class="shrink-0"><IcoLayers /></span>
        <span class="font-body text-xs">{isUpdate() ? "Updated retained context" : "Read retained context"}</span>
        <Show when={hasDetail()}>
          <span class="ml-auto"><Chevron open={expanded()} /></span>
        </Show>
      </button>
      <Show when={expanded()}>
        <DetailPanel tool={props.tool} />
      </Show>
    </div>
  );
};

/* ── Preview forwarding ── */

const PreviewBlock: Component<{ tool: ToolChunk }> = (props) => {
  const out = () => parseToolOutput(props.tool);
  const expanded = () => isExpanded(props.tool.id);
  const forwards = () => {
    const output = out();
    if (Array.isArray(output?.forwards)) return output.forwards;
    if (output?.forward) return [output.forward];
    return [];
  };
  const isClose = () => props.tool.title === "close_port_forward" || props.tool.title === "Close Port Forward";
  const firstForward = () => forwards()[0];
  const forwardLabel = (forward: { label?: string }) => {
    const label = forward.label?.trim();
    return label && label.length > 0 ? label : "Preview";
  };
  const summary = () => {
    const items = forwards();
    if (isClose()) {
      if (!outputWasClosed()) return "Preview was already closed";
      return items.length === 1 ? `Closed ${forwardLabel(firstForward())}` : "Closed preview";
    }
    if (items.length === 0) return "No active previews";
    if (items.length === 1) {
      return `Forwarded ${forwardLabel(firstForward())}`;
    }
    return `${items.length} active previews`;
  };
  const outputWasClosed = () => !!out()?.closed;

  return (
    <div class="text-xs">
      <button
        type="button"
        class={cn(toolTrigger, "hover:text-foreground")}
        onClick={() => toggle(props.tool.id)}
      >
        <span class={cn("h-1.5 w-1.5 rounded-full shrink-0", statusDot(props.tool.status))} />
        <span class="shrink-0"><IcoGlobe /></span>
        <span class="font-body text-xs truncate">{summary()}</span>
        <span class="ml-auto"><Chevron open={expanded()} /></span>
      </button>
      <Show when={expanded()}>
        <div class={cn(toolPanel, "space-y-2 p-2.5")}>
          <Show when={forwards().length === 0}>
            <div class="text-[11px] text-muted-foreground">No forwarded previews.</div>
          </Show>
          <For each={forwards()}>
            {(forward) => (
              <div class="border border-border/40 bg-background/80 p-2">
                <div class="flex items-center gap-2">
                  <span class="border border-signal-blue/25 bg-signal-blue/10 px-1.5 py-0.5 font-mono text-[10px] text-signal-blue">
                    @{forward.threadId}
                  </span>
                  <span class="font-mono text-[11px] text-foreground">{forward.port}</span>
                  <span class="text-[11px] text-muted-foreground">{forwardLabel(forward)}</span>
                </div>
                <div class="mt-2 space-y-1">
                  <div class="flex items-center gap-2">
                    <span class="w-14 shrink-0 font-mono text-[10px] uppercase tracking-[0.18em] text-muted-foreground/70">User</span>
                    <button
                      type="button"
                      class="truncate text-left font-mono text-[11px] text-signal-blue hover:underline"
                      onClick={() => void openUrl(forward.userUrl)}
                    >
                      {forward.userUrl}
                    </button>
                  </div>
                  <div class="flex items-center gap-2">
                    <span class="w-14 shrink-0 font-mono text-[10px] uppercase tracking-[0.18em] text-muted-foreground/70">Shepherd</span>
                    <span class="truncate font-mono text-[11px] text-muted-foreground">
                      {forward.shepherdUrl}
                    </span>
                  </div>
                </div>
              </div>
            )}
          </For>
          <Show when={props.tool.input || props.tool.output}>
            <DetailPanel tool={props.tool} borderColor="border-l-signal-blue/20" />
          </Show>
        </div>
      </Show>
    </div>
  );
};

/* ── Generic tool fallback ── */

const GenericBlock: Component<{ tool: ToolChunk }> = (props) => {
  const expanded = () => isExpanded(props.tool.id);
  const hasDetail = () => !!(props.tool.input || props.tool.output);
  const label = () => props.tool.title.replace(/_/g, " ");

  return (
    <div class="text-xs">
      <button
        type="button"
        class={cn(toolTrigger, "hover:text-foreground")}
        onClick={() => hasDetail() && toggle(props.tool.id)}
        disabled={!hasDetail()}
      >
        <span class={cn("h-1.5 w-1.5 rounded-full shrink-0", statusDot(props.tool.status))} />
        <span class="shrink-0"><IcoCircle /></span>
        <span class="font-body text-xs">{label()}</span>
        <Show when={hasDetail()}>
          <span class="ml-auto"><Chevron open={expanded()} /></span>
        </Show>
      </button>
      <Show when={expanded()}>
        <DetailPanel tool={props.tool} />
      </Show>
    </div>
  );
};

/* ── Exploration group: merged consecutive reads/greps/lists ── */

const ExplorationGroup: Component<{ tools: ToolChunk[] }> = (props) => {
  const groupId = () => props.tools.map((t) => t.id).join(",");
  const expanded = () => isExpanded(groupId());
  const allDone = () => props.tools.every((t) =>
    ["done", "completed", "success"].includes((t.status ?? "").toLowerCase()),
  );

  const detailLines = () => {
    const reads: string[] = [];
    const searches: string[] = [];
    const globs: string[] = [];
    const lists: string[] = [];
    for (const tool of props.tools) {
      const inp = parseToolInput(tool);
      const subj = inp?.path ?? inp?.pattern ?? ".";
      const name = tool.title;
      if (name === "read_file" || name === "read_workspace_file") reads.push(subj);
      else if (name === "grep" || name === "grep_workspace") searches.push(inp?.pattern ? `"${inp.pattern}"${inp.path ? ` in ${inp.path}` : ""}` : subj);
      else if (name === "glob") globs.push(subj);
      else if (name === "ls" || name === "list_workspace") lists.push(subj);
      else reads.push(subj); // canvas/context reads
    }
    const fmt = (verb: string, items: string[]) => {
      const unique = dedupe(items);
      if (unique.length <= 3) return `${verb} ${unique.join(", ")}`;
      return `${verb} ${unique.slice(0, 3).join(", ")} +${unique.length - 3} more`;
    };
    const lines: string[] = [];
    if (searches.length) lines.push(fmt("Search", searches));
    if (reads.length) lines.push(fmt("Read", reads));
    if (globs.length) lines.push(fmt("Glob", globs));
    if (lists.length) lines.push(fmt("List", lists));
    return lines;
  };

  const summary = () => {
    const lines = detailLines();
    if (lines.length === 0) return `Explored ${props.tools.length} items`;
    if (lines.length === 1 && lines[0].length < 80) return lines[0];
    return lines.join(" · ");
  };

  return (
    <div class="text-xs">
      <button
        type="button"
        class={cn(toolTrigger, "hover:text-foreground")}
        onClick={() => toggle(groupId())}
      >
        <span class={cn("h-1.5 w-1.5 rounded-full shrink-0", allDone() ? "bg-signal-green" : "bg-signal-amber animate-pulse-dot")} />
        <span class="flex w-3.5 items-center justify-center shrink-0"><IcoCompass /></span>
        <span class="font-body text-xs truncate">{summary()}</span>
        <span class="ml-auto"><Chevron open={expanded()} /></span>
      </button>
      <Show when={expanded()}>
        <div class={cn(toolPanel, "py-0.5")}>
          <For each={props.tools}>
            {(tool) => <ExplorationLine tool={tool} />}
          </For>
        </div>
      </Show>
    </div>
  );
};

const ExplorationLine: Component<{ tool: ToolChunk }> = (props) => {
  const inp = () => parseToolInput(props.tool);
  const subj = () => inp()?.path ?? inp()?.pattern ?? null;
  const expanded = () => isExpanded(props.tool.id);
  const hasDetail = () => !!(props.tool.input || props.tool.output);

  return (
    <div>
      <button
        type="button"
        class="flex w-full items-center gap-2 px-2.5 py-1 text-left text-muted-foreground transition-colors hover:bg-secondary/30 text-xs"
        onClick={() => hasDetail() && toggle(props.tool.id)}
        disabled={!hasDetail()}
      >
        <span class="flex w-3.5 items-center justify-center shrink-0 text-muted-foreground/60">{renderToolIcon(props.tool.title)}</span>
        <span class="font-body text-xs">{toolLabel(props.tool.title)}</span>
        <Show when={subj()}>
          <span class="font-mono text-muted-foreground/60 truncate">{subj()}</span>
        </Show>
        <Show when={hasDetail()}>
          <span class="ml-auto"><Chevron open={expanded()} /></span>
        </Show>
      </button>
      <Show when={expanded()}>
        <div class="mx-2 mb-1 border border-border/30 bg-background/40 overflow-hidden">
          <Show when={props.tool.output}>
            <pre class="p-2 font-mono text-[11px] text-muted-foreground whitespace-pre-wrap break-all max-h-48 overflow-y-auto">
              {formatToolJson(props.tool.output!)}
            </pre>
          </Show>
        </div>
      </Show>
    </div>
  );
};

/* ── Plan update — compact summary (full plan lives in the panel above chat) ── */

const PlanBlock: Component<{ tool: ToolChunk }> = (props) => {
  const inp = () => parseToolInput(props.tool);
  const steps = (): Array<{ step: string; status: string }> => {
    const p = inp()?.plan;
    if (!Array.isArray(p)) return [];
    return p;
  };
  const completed = () => steps().filter((s) => s.status === "completed").length;
  const total = () => steps().length;
  const activeStep = () => steps().find((s) => s.status === "in_progress");

  return (
    <div class="text-xs">
      <div class={cn(toolTrigger, "cursor-default")}>
        <span class={cn("h-1.5 w-1.5 rounded-full shrink-0", statusDot(props.tool.status))} />
        <span class="shrink-0"><IcoChecklist /></span>
        <span class="font-body text-xs">Plan updated</span>
        <span class="font-mono text-[11px] text-muted-foreground/60">{completed()}/{total()}</span>
        <Show when={activeStep()}>
          <span class="text-xs text-foreground truncate">
            — {activeStep()!.step}
          </span>
        </Show>
      </div>
    </div>
  );
};

/* ── Tool block dispatcher ── */

function ToolBlock(props: { tool: ToolChunk }) {
  switch (getToolDisplayKind(props.tool.title)) {
    case "plan":
      return <PlanBlock tool={props.tool} />;
    case "shell":
      return <ShellBlock tool={props.tool} />;
    case "patch":
      return <PatchBlock tool={props.tool} />;
    case "web-search":
      return <WebSearchBlock tool={props.tool} />;
    case "fetch":
      return <FetchBlock tool={props.tool} />;
    case "thread":
      return <ThreadBlock tool={props.tool} />;
    case "canvas":
      return <CanvasBlock tool={props.tool} />;
    case "context":
      return <ContextBlock tool={props.tool} />;
    case "preview":
      return <PreviewBlock tool={props.tool} />;
    default:
      return <GenericBlock tool={props.tool} />;
  }
}

/* ── Utility ── */

function dedupe(arr: string[]): string[] {
  const seen = new Set<string>();
  return arr.filter((v) => { if (seen.has(v)) return false; seen.add(v); return true; });
}

function formatFileRefLabel(block: Extract<RenderBlock, { kind: "fileRef" }>): string {
  if (!block.lineStart) {
    return `@${block.path}`;
  }
  if (block.lineEnd && block.lineEnd !== block.lineStart) {
    return `@${block.path}:${block.lineStart}-${block.lineEnd}`;
  }
  return `@${block.path}:${block.lineStart}`;
}

function renderBlock(block: RenderBlock, isUser: boolean): JSX.Element {
  switch (block.kind) {
    case "text":
      return (
        <div
          class={cn(
            "markdown-body text-sm leading-relaxed",
            "text-foreground",
          )}
          innerHTML={renderMarkdown(block.content)}
        />
      );
    case "notice": {
      const toneClass = block.tone === "warning"
        ? "border-signal-amber/30 bg-signal-amber/10 text-signal-amber"
        : block.tone === "danger"
          ? "border-signal-red/30 bg-signal-red/10 text-signal-red"
          : block.tone === "success"
            ? "border-signal-green/30 bg-signal-green/10 text-signal-green"
            : "border-border/60 bg-background/60 text-foreground";
      return (
        <div class={cn("my-1 border px-3 py-2 text-xs", toneClass)}>
          <Show when={block.title}>
            <div class="mb-0.5 font-medium">{block.title}</div>
          </Show>
          <div class="leading-relaxed">{block.content}</div>
        </div>
      );
    }
    case "thinking":
      return (
        <Collapsible class="group/think">
          <CollapsibleTrigger class="flex cursor-pointer items-center gap-1.5 py-0.5 text-xs text-muted-foreground transition-colors hover:text-foreground">
            <svg class="h-3 w-3 transition-transform group-data-[expanded]/think:rotate-90" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <polyline points="9 6 15 12 9 18" />
            </svg>
            Thinking
          </CollapsibleTrigger>
          <CollapsibleContent class="mt-1 ml-[22px] border-l border-border/50 pl-3 text-xs text-muted-foreground leading-relaxed whitespace-pre-wrap overflow-hidden animate-accordion-down data-[closed]:animate-accordion-up">{block.content}</CollapsibleContent>
        </Collapsible>
      );
    case "exploration":
      return <ExplorationGroup tools={block.tools} />;
    case "tool":
      return <ToolBlock tool={block.tool} />;
    case "image":
      return <div class="my-1"><img src={block.src} alt={block.name ?? "image"} class="max-w-full max-h-80 border border-border" /></div>;
    case "skill":
      return (
        <div
          class={cn(
            "my-1 inline-flex min-w-[180px] max-w-full flex-col border px-2.5 py-2 text-left",
            isUser
              ? "border-signal-blue/25 bg-signal-blue/8"
              : "border-border/60 bg-background/60",
          )}
        >
          <div class="flex items-center gap-2">
            <span class="text-signal-blue"><IcoSlash /></span>
            <span class="font-mono text-[11px] text-foreground">/{block.name}</span>
          </div>
          <Show when={block.description}>
            <div class="mt-1 text-[11px] leading-relaxed text-muted-foreground">
              {block.description}
            </div>
          </Show>
        </div>
      );
    case "fileRef":
      return (
        <div
          class={cn(
            "my-1 inline-flex min-w-[180px] max-w-full items-start gap-2 border px-2.5 py-2 text-left",
            isUser
              ? "border-signal-green/25 bg-signal-green/8"
              : "border-border/60 bg-background/60",
          )}
        >
          <span class="mt-0.5 text-signal-green"><IcoFile /></span>
          <div class="min-w-0">
            <div class="font-mono text-[11px] text-foreground">
              {formatFileRefLabel(block)}
            </div>
            <div class="mt-1 text-[11px] text-muted-foreground">
              {block.rootId}
            </div>
          </div>
        </div>
      );
  }
}

const LiveStatusRow: Component<{ status?: string }> = (props) => {
  const label = createMemo(() => {
    switch (props.status) {
      case "queued":
        return "Queued";
      case "interrupting":
        return "Stopping";
      default:
        return "Thinking";
    }
  });

  return (
    <div class="flex items-center gap-2 py-1 text-xs text-muted-foreground">
      <span class={cn(
        "h-1.5 w-1.5 rounded-full",
        props.status === "interrupting"
          ? "bg-signal-amber animate-pulse-dot"
          : "bg-signal-amber animate-pulse-dot",
      )} />
      <span>{label()}</span>
    </div>
  );
};

/* ── Main component ── */

function formatTime(ts: string): string {
  try { return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false }); }
  catch { return ""; }
}

const ChatMessage: Component<{ role: string; chunksJson: string; timestamp: string; liveStatus?: string }> = (props) => {
  const blocks = createMemo(() => parseChatBlocks(props.chunksJson));
  const isUser = () => props.role === "user";
  const showLiveStatus = () =>
    !isUser()
    && !!props.liveStatus
    && (blocks().length === 0 || props.liveStatus === "interrupting" || props.liveStatus === "starting");

  return (
    <div
      class={cn(
        "group relative",
        isUser()
          ? "ml-20 border border-border/50 bg-secondary/40 px-4 py-3"
          : "border-l-2 border-l-muted-foreground/15 pl-4 py-2",
      )}
    >
      {/* Role + timestamp header */}
      <div class={cn(
        "mb-1.5 flex items-center gap-2",
        isUser() && "justify-end",
      )}>
        <span class={cn(
          "font-mono text-[10px] uppercase tracking-[0.14em]",
          isUser() ? "text-foreground/50" : "text-muted-foreground/60",
        )}>
          {isUser() ? "You" : "Assistant"}
        </span>
        <span class="text-[10px] text-muted-foreground/30">
          {formatTime(props.timestamp)}
        </span>
      </div>

      <div class={cn("space-y-1", !isUser() && "space-y-0.5")}>
        <Show when={showLiveStatus()}>
          <LiveStatusRow status={props.liveStatus} />
        </Show>
        <Index each={blocks()}>
          {(block) => renderBlock(block(), isUser())}
        </Index>
      </div>
    </div>
  );
};

export default ChatMessage;
