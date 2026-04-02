import { type Component, type JSX, createMemo, For, Index, Show } from "solid-js";
import { createStore, produce } from "solid-js/store";
import { cn } from "@/lib/cn";
import { renderMarkdown } from "@/lib/markdown";

/* ── Module-level expand state (survives component re-creation from polling) ── */

const [expandedTools, setExpandedTools] = createStore<Record<string, boolean>>({});
function toggle(id: string) { setExpandedTools(produce((s) => { s[id] = !s[id]; })); }
function isExpanded(id: string): boolean { return !!expandedTools[id]; }

/* ── Chunk types (backend ShepherdMessageChunk) ── */

interface ToolChunk {
  type: "tool";
  id: string;
  title: string;
  kind?: string;
  status: string;
  input?: string;
  output?: string;
}

type Chunk =
  | { type: "text"; content: string }
  | { type: "thinking"; content: string }
  | ToolChunk
  | { type: "image"; mimeType: string; dataBase64: string; name?: string };

/* ── Render blocks ── */

type RenderBlock =
  | { kind: "text"; content: string }
  | { kind: "thinking"; content: string }
  | { kind: "exploration"; tools: ToolChunk[] }
  | { kind: "tool"; tool: ToolChunk }
  | { kind: "image"; src: string; name?: string };

/* ── Tool classification ── */

const EXPLORATION = new Set([
  "list_workspace", "read_workspace_file", "grep_workspace",
  "read_file", "grep", "glob", "ls",
  "read_project_focus_view", "read_canvas", "Canvas",
  "read_project_retained_context", "Retained Context",
]);

/* ── Batch flattening ── */

function flattenBatch(batch: ToolChunk): ToolChunk[] {
  let calls: any[] = [];
  let results: any[] = [];
  try { if (batch.input) calls = JSON.parse(batch.input).tool_calls ?? []; } catch { /* */ }
  try { if (batch.output) results = JSON.parse(batch.output).results ?? []; } catch { /* */ }
  if (results.length > 0) {
    return results.map((r: any, i: number) => ({
      type: "tool" as const, id: `${batch.id}_${i}`,
      title: r.tool ?? calls[i]?.tool ?? "tool", kind: batch.kind,
      status: r.success === false ? "failed" : "done",
      input: calls[i]?.parameters ? JSON.stringify(calls[i].parameters) : undefined,
      output: (r.success ? r.result : r.error) !== undefined ? JSON.stringify(r.success ? r.result : r.error) : undefined,
    }));
  }
  if (calls.length > 0) {
    return calls.map((c: any, i: number) => ({
      type: "tool" as const, id: `${batch.id}_${i}`, title: c.tool, kind: batch.kind,
      status: "running",
      input: c.parameters ? JSON.stringify(c.parameters) : undefined, output: undefined,
    }));
  }
  return [batch];
}

/* ── Block builder: interleave tools with text, merge explorations ── */

function buildBlocks(chunks: Chunk[]): RenderBlock[] {
  const blocks: RenderBlock[] = [];
  let explBatch: ToolChunk[] = [];

  const flushExpl = () => {
    if (explBatch.length > 0) {
      blocks.push({ kind: "exploration", tools: [...explBatch] });
      explBatch = [];
    }
  };

  const pushTool = (tool: ToolChunk) => {
    if (EXPLORATION.has(tool.title)) { explBatch.push(tool); }
    else { flushExpl(); blocks.push({ kind: "tool", tool }); }
  };

  for (const chunk of chunks) {
    switch (chunk.type) {
      case "tool":
        if (chunk.title === "batch") { for (const c of flattenBatch(chunk)) pushTool(c); }
        else pushTool(chunk);
        break;
      case "text":
        flushExpl();
        if (chunk.content.trim()) blocks.push({ kind: "text", content: chunk.content });
        break;
      case "thinking":
        flushExpl();
        if (chunk.content.trim()) blocks.push({ kind: "thinking", content: chunk.content });
        break;
      case "image":
        flushExpl();
        blocks.push({ kind: "image", src: `data:${chunk.mimeType};base64,${chunk.dataBase64}`, name: chunk.name });
        break;
    }
  }
  flushExpl();
  return blocks;
}

/* ── JSON helpers ── */

function parseInput(tool: ToolChunk): any {
  try { return tool.input ? JSON.parse(tool.input) : null; } catch { return null; }
}

function parseOutput(tool: ToolChunk): any {
  try { return tool.output ? JSON.parse(tool.output) : null; } catch { return null; }
}

function formatJson(raw: string): string {
  try { return JSON.stringify(JSON.parse(raw), null, 2); } catch { return raw; }
}

function inlineText(s: string): string {
  return s.split(/\s+/).join(" ");
}

function snippet(s: string, max: number): string {
  const compact = inlineText(s);
  return compact.length > max ? compact.slice(0, max) + "..." : compact;
}

function displayUrl(url: string): string {
  try {
    const u = new URL(url);
    return u.hostname + (u.pathname.length > 1 ? u.pathname : "");
  } catch { return url; }
}

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

/* ── Shared styles ── */

const toolTrigger = "flex w-full items-center gap-2 px-1.5 py-1 -mx-1.5 text-left text-muted-foreground transition-colors hover:bg-secondary/30";
const toolPanel = "mt-1 ml-5 border border-border/40 bg-background/60 overflow-hidden";

/* ── Expandable detail panel ── */

const DetailPanel: Component<{ tool: ToolChunk; borderColor?: string }> = (props) => (
  <div class={cn(toolPanel, props.borderColor && `border-l-2 ${props.borderColor}`)}>
    <Show when={props.tool.input}>
      <div class="px-2.5 pt-2">
        <span class="chassis-label">Input</span>
        <pre class="mt-1 font-mono text-[11px] text-muted-foreground whitespace-pre-wrap break-all max-h-32 overflow-y-auto">
          {formatJson(props.tool.input!)}
        </pre>
      </div>
    </Show>
    <Show when={props.tool.output}>
      <div class={props.tool.input ? "border-t border-border/30 px-2.5 py-2" : "px-2.5 py-2"}>
        <span class="chassis-label">Output</span>
        <pre class="mt-1 font-mono text-[11px] text-muted-foreground whitespace-pre-wrap break-all max-h-48 overflow-y-auto">
          {formatJson(props.tool.output!)}
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
  const inp = () => parseInput(props.tool);
  const out = () => parseOutput(props.tool);
  const cmd = () => inp()?.cmd ?? inp()?.command ?? "command";
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
        <span class="font-mono text-xs truncate">{snippet(cmd(), 80)}</span>
        <Show when={exitCode() !== null && exitCode() !== 0}>
          <span class="font-mono text-[11px] text-signal-red shrink-0">exit {exitCode()}</span>
        </Show>
        <span class="ml-auto"><Chevron open={expanded()} /></span>
      </button>
      <Show when={expanded() && shellOutput()}>
        <div class={cn(toolPanel, failed() && "border-l-2 border-l-signal-red/40")}>
          <pre class="p-2.5 font-mono text-[11px] text-muted-foreground whitespace-pre-wrap break-all max-h-48 overflow-y-auto">
            {shellOutput()}
          </pre>
        </div>
      </Show>
    </div>
  );
};

/* ── Patch / edit (apply_patch) ── */

const PatchBlock: Component<{ tool: ToolChunk }> = (props) => {
  const out = () => parseOutput(props.tool);
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
                  <pre class="px-2.5 py-1.5 font-mono text-[11px] whitespace-pre-wrap break-all max-h-40 overflow-y-auto">
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
  const inp = () => parseInput(props.tool);
  const out = () => parseOutput(props.tool);
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
        <span class="font-body text-xs">searched "{snippet(query(), 50)}"</span>
        <span class="ml-auto"><Chevron open={expanded()} /></span>
      </button>
      <Show when={expanded()}>
        <div class={cn(toolPanel, "p-2.5 space-y-1")}>
          <Show when={answer()}>
            <div class="text-[11px] text-muted-foreground leading-relaxed">{snippet(answer()!, 200)}</div>
          </Show>
          <For each={results()}>
            {(r) => (
              <div class="flex items-baseline gap-1.5 text-[11px]">
                <span class="truncate text-foreground/80">{r.title ? snippet(r.title, 48) : ""}</span>
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
  const inp = () => parseInput(props.tool);
  const out = () => parseOutput(props.tool);
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
            {snippet(text()!, 2000)}
          </pre>
        </div>
      </Show>
    </div>
  );
};

/* ── Thread tools ── */

const ThreadBlock: Component<{ tool: ToolChunk }> = (props) => {
  const m = () => TOOL_META[props.tool.title] ?? { icon: IcoCircle, label: props.tool.title };
  const expanded = () => isExpanded(props.tool.id);
  const hasDetail = () => !!(props.tool.input || props.tool.output);
  const out = () => parseOutput(props.tool);
  const inp = () => parseInput(props.tool);
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
        <span class="flex w-3.5 items-center justify-center shrink-0">{m().icon()}</span>
        <span class="font-body text-xs">{m().label}</span>
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

/* ── Tool metadata (for exploration + thread blocks) ── */

const TOOL_META: Record<string, { icon: () => JSX.Element; label: string }> = {
  list_threads:       { icon: IcoThread, label: "List threads" },
  create_thread:      { icon: IcoThread, label: "Create thread" },
  rename_thread:      { icon: IcoPencil, label: "Rename thread" },
  set_thread_status:  { icon: IcoThread, label: "Set status" },
  archive_thread:     { icon: IcoThread, label: "Archive thread" },
  delete_thread:      { icon: IcoThread, label: "Delete thread" },
  send_thread_message:{ icon: IcoThread, label: "Message thread" },
  read_thread_updates:{ icon: IcoThread, label: "Read thread" },
  list_workspace:     { icon: IcoList,   label: "List" },
  read_workspace_file:{ icon: IcoFile,   label: "Read" },
  grep_workspace:     { icon: IcoSearch, label: "Search" },
  read_file:          { icon: IcoFile,   label: "Read" },
  grep:               { icon: IcoSearch, label: "Search" },
  glob:               { icon: IcoSearch, label: "Glob" },
  ls:                 { icon: IcoList,   label: "List" },
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
      const inp = parseInput(tool);
      const subj = inp?.path ?? inp?.pattern ?? ".";
      const name = tool.title;
      if (name === "read_file" || name === "read_workspace_file") reads.push(subj);
      else if (name === "grep" || name === "grep_workspace") searches.push(inp?.pattern ? `"${inp.pattern}"${inp.path ? ` in ${inp.path}` : ""}` : subj);
      else if (name === "glob") globs.push(subj);
      else if (name === "ls" || name === "list_workspace") lists.push(subj);
      else reads.push(subj); // canvas/context reads
    }
    const lines: string[] = [];
    if (searches.length) lines.push(`Search ${dedupe(searches).join(", ")}`);
    if (reads.length) lines.push(`Read ${dedupe(reads).join(", ")}`);
    if (globs.length) lines.push(`Glob ${dedupe(globs).join(", ")}`);
    if (lists.length) lines.push(`List ${dedupe(lists).join(", ")}`);
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
  const m = () => TOOL_META[props.tool.title] ?? { icon: IcoFile, label: props.tool.title };
  const inp = () => parseInput(props.tool);
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
        <span class="flex w-3.5 items-center justify-center shrink-0 text-muted-foreground/60">{m().icon()}</span>
        <span class="font-body text-xs">{m().label}</span>
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
            <pre class="p-2 font-mono text-[11px] text-muted-foreground whitespace-pre-wrap break-all max-h-32 overflow-y-auto">
              {formatJson(props.tool.output!)}
            </pre>
          </Show>
        </div>
      </Show>
    </div>
  );
};

/* ── Tool block dispatcher ── */

const THREAD_TOOLS = new Set([
  "list_threads", "create_thread", "rename_thread", "set_thread_status",
  "archive_thread", "delete_thread", "send_thread_message", "read_thread_updates",
  // Mapped titles from backend
  "Threads", "Create Thread", "Rename Thread", "Thread Status",
  "Archive Thread", "Delete Thread", "Message Thread", "Thread Updates",
]);

const CANVAS_TOOLS = new Set([
  "read_project_focus_view", "read_canvas", "update_project_focus_view", "update_canvas",
  "Canvas", "Canvas Update",
]);

const CONTEXT_TOOLS = new Set([
  "read_project_retained_context", "update_project_retained_context",
  "Retained Context", "Retained Context Update",
]);

/* ── Plan update — compact summary (full plan lives in the panel above chat) ── */

const PlanBlock: Component<{ tool: ToolChunk }> = (props) => {
  const inp = () => parseInput(props.tool);
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
  const name = props.tool.title;
  if (name === "update_plan" || name === "Plan Update") return <PlanBlock tool={props.tool} />;
  if (name === "exec_command" || name === "write_stdin") return <ShellBlock tool={props.tool} />;
  if (name === "apply_patch") return <PatchBlock tool={props.tool} />;
  if (name === "search_web") return <WebSearchBlock tool={props.tool} />;
  if (name === "fetch_url") return <FetchBlock tool={props.tool} />;
  if (THREAD_TOOLS.has(name)) return <ThreadBlock tool={props.tool} />;
  if (CANVAS_TOOLS.has(name)) return <CanvasBlock tool={props.tool} />;
  if (CONTEXT_TOOLS.has(name)) return <ContextBlock tool={props.tool} />;
  return <GenericBlock tool={props.tool} />;
}

/* ── Utility ── */

function dedupe(arr: string[]): string[] {
  const seen = new Set<string>();
  return arr.filter((v) => { if (seen.has(v)) return false; seen.add(v); return true; });
}

/* ── Main component ── */

function formatTime(ts: string): string {
  try { return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false }); }
  catch { return ""; }
}

const ChatMessage: Component<{ role: string; chunksJson: string; timestamp: string }> = (props) => {
  const blocks = createMemo(() => {
    try { return buildBlocks(JSON.parse(props.chunksJson) as Chunk[]); }
    catch { return [{ kind: "text" as const, content: props.chunksJson }]; }
  });
  const isUser = () => props.role === "user";

  return (
    <div
      class={cn(
        "group relative px-4 py-3 shadow-sm transition-colors",
        isUser() ? "ml-10 bg-foreground text-background" : "border border-border bg-card",
      )}
    >
      <div class="flex items-baseline gap-2 mb-1">
        <span class={cn("chassis-label", isUser() && "text-background/65")}>
          {isUser() ? "you" : "shepherd"}
        </span>
        <span class={cn("text-[11px] text-muted-foreground", isUser() ? "ml-auto text-background/50" : "")}>
          {formatTime(props.timestamp)}
        </span>
      </div>
      <div class="space-y-1">
        <Index each={blocks()}>
          {(block) => {
            const b = block();
            switch (b.kind) {
              case "text":
                return (
                  <div
                    class={cn(
                      "markdown-body text-sm leading-relaxed",
                      isUser() ? "text-background" : "text-foreground",
                    )}
                    innerHTML={renderMarkdown(b.content)}
                  />
                );
              case "thinking":
                return (
                  <details class="group/think">
                    <summary class="flex cursor-pointer items-center gap-1.5 py-0.5 text-xs text-muted-foreground transition-colors hover:text-foreground">
                      <svg class="h-3 w-3 transition-transform group-open/think:rotate-90" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <polyline points="9 6 15 12 9 18" />
                      </svg>
                      Thinking
                    </summary>
                    <div class="mt-1 ml-[22px] border-l border-border/50 pl-3 text-xs text-muted-foreground leading-relaxed whitespace-pre-wrap">{b.content}</div>
                  </details>
                );
              case "exploration":
                return <ExplorationGroup tools={b.tools} />;
              case "tool":
                return <ToolBlock tool={b.tool} />;
              case "image":
                return <div class="my-1"><img src={b.src} alt={b.name ?? "image"} class="max-w-full max-h-80 border border-border" /></div>;
            }
          }}
        </Index>
      </div>
    </div>
  );
};

export default ChatMessage;
