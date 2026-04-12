export interface ToolChunk {
  type: "tool";
  id: string;
  title: string;
  kind?: string;
  status: string;
  input?: string;
  output?: string;
}

export type ChatChunk =
  | { type: "text"; content: string }
  | { type: "notice"; tone: string; title?: string; content: string }
  | { type: "thinking"; content: string }
  | ToolChunk
  | { type: "image"; mimeType: string; dataBase64: string; name?: string }
  | { type: "skill"; name: string; description?: string; path: string }
  | {
      type: "fileRef";
      rootId: string;
      path: string;
      lineStart?: number;
      lineEnd?: number;
    };

export type RenderBlock =
  | { kind: "text"; content: string }
  | { kind: "notice"; tone: string; title?: string; content: string }
  | { kind: "thinking"; content: string }
  | { kind: "exploration"; tools: ToolChunk[] }
  | { kind: "tool"; tool: ToolChunk }
  | { kind: "image"; src: string; name?: string }
  | { kind: "skill"; name: string; description?: string }
  | {
      kind: "fileRef";
      rootId: string;
      path: string;
      lineStart?: number;
      lineEnd?: number;
    };

export type ToolDisplayKind =
  | "exploration"
  | "plan"
  | "shell"
  | "patch"
  | "web-search"
  | "fetch"
  | "thread"
  | "canvas"
  | "context"
  | "preview"
  | "generic";

const EXPLORATION_TOOLS = new Set([
  "list_workspace",
  "read_workspace_file",
  "grep_workspace",
  "read_file",
  "grep",
  "glob",
  "ls",
  "read_project_retained_context",
  "Retained Context",
]);

const THREAD_TOOLS = new Set([
  "list_threads",
  "create_thread",
  "rename_thread",
  "set_thread_status",
  "archive_thread",
  "delete_thread",
  "send_thread_message",
  "read_thread_updates",
  "Threads",
  "Create Thread",
  "Rename Thread",
  "Thread Status",
  "Archive Thread",
  "Delete Thread",
  "Message Thread",
  "Thread Updates",
]);

const CANVAS_TOOLS = new Set<string>([]);

const CONTEXT_TOOLS = new Set([
  "read_project_retained_context",
  "update_project_retained_context",
  "Retained Context",
  "Retained Context Update",
]);

const PREVIEW_TOOLS = new Set([
  "forward_port",
  "list_port_forwards",
  "close_port_forward",
  "Forward Port",
  "Port Forwards",
  "Close Port Forward",
]);

const TOOL_LABELS: Record<string, string> = {
  graph_surql: "Knowledge Graph Query",
  KnowledgeGraphQuery: "Knowledge Graph Query",
  edit_graph_node_text: "Knowledge Graph Text Patch",
  patch_canvas_document: "Canvas Patch",
  list_threads: "List threads",
  create_thread: "Create thread",
  rename_thread: "Rename thread",
  set_thread_status: "Set status",
  archive_thread: "Archive thread",
  delete_thread: "Delete thread",
  send_thread_message: "Message thread",
  read_thread_updates: "Read thread",
  list_workspace: "List",
  read_workspace_file: "Read",
  grep_workspace: "Search",
  read_file: "Read",
  grep: "Search",
  glob: "Glob",
  ls: "List",
};

export function parseToolInput(tool: ToolChunk): any {
  try {
    return tool.input ? JSON.parse(tool.input) : null;
  } catch {
    return null;
  }
}

export function parseToolOutput(tool: ToolChunk): any {
  try {
    return tool.output ? JSON.parse(tool.output) : null;
  } catch {
    return null;
  }
}

export function formatToolJson(raw: string): string {
  try {
    return JSON.stringify(JSON.parse(raw), null, 2);
  } catch {
    return raw;
  }
}

function inlineText(value: string): string {
  return value.split(/\s+/).join(" ");
}

export function snippetText(value: string, max: number): string {
  const compact = inlineText(value);
  return compact.length > max ? `${compact.slice(0, max)}...` : compact;
}

export function displayUrl(value: string): string {
  try {
    const parsed = new URL(value);
    return parsed.hostname + (parsed.pathname.length > 1 ? parsed.pathname : "");
  } catch {
    return value;
  }
}

export function toolLabel(name: string): string {
  return TOOL_LABELS[name] ?? name.replace(/_/g, " ");
}

function toolHasTerminalOutput(tool: ToolChunk): boolean {
  const output = parseToolOutput(tool);
  if (!output || typeof output !== "object") return false;
  if (typeof output.error === "string" && output.error.trim()) return true;
  if (typeof output.message === "string" && output.message.trim()) return true;
  if (typeof output.statement_count === "number") return true;
  if (Array.isArray(output.results)) return true;
  return false;
}

export function getToolDisplayKind(name: string): ToolDisplayKind {
  if (EXPLORATION_TOOLS.has(name)) return "exploration";
  if (name === "update_plan" || name === "Plan Update") return "plan";
  if (name === "exec_command" || name === "write_stdin") return "shell";
  if (name === "apply_patch") return "patch";
  if (name === "search_web") return "web-search";
  if (name === "fetch_url") return "fetch";
  if (THREAD_TOOLS.has(name)) return "thread";
  if (CANVAS_TOOLS.has(name)) return "canvas";
  if (CONTEXT_TOOLS.has(name)) return "context";
  if (PREVIEW_TOOLS.has(name)) return "preview";
  return "generic";
}

function flattenBatch(batch: ToolChunk): ToolChunk[] {
  let calls: any[] = [];
  let results: any[] = [];
  try {
    if (batch.input) calls = JSON.parse(batch.input).tool_calls ?? [];
  } catch {}
  try {
    if (batch.output) results = JSON.parse(batch.output).results ?? [];
  } catch {}
  if (results.length > 0) {
    return results.map((result: any, index: number) => ({
      type: "tool" as const,
      id: `${batch.id}_${index}`,
      title: result.tool ?? calls[index]?.tool ?? "tool",
      kind: batch.kind,
      status: result.success === false ? "failed" : "done",
      input: calls[index]?.parameters ? JSON.stringify(calls[index].parameters) : undefined,
      output:
        (result.success ? result.result : result.error) !== undefined
          ? JSON.stringify(result.success ? result.result : result.error)
          : undefined,
    }));
  }
  if (calls.length > 0) {
    return calls.map((call: any, index: number) => ({
      type: "tool" as const,
      id: `${batch.id}_${index}`,
      title: call.tool,
      kind: batch.kind,
      status: "running",
      input: call.parameters ? JSON.stringify(call.parameters) : undefined,
      output: undefined,
    }));
  }
  return [batch];
}

export function buildBlocks(chunks: ChatChunk[], live = false): RenderBlock[] {
  const blocks: RenderBlock[] = [];
  let explorationBatch: ToolChunk[] = [];

  // When a message is in history (not live), tools stuck in "running" were
  // interrupted — normalize them so the UI doesn't show a perpetual spinner.
  const normalizeTool = (tool: ToolChunk): ToolChunk => {
    const s = tool.status.toLowerCase();
    if (s === "running" || s === "active" || s === "queued" || s === "starting") {
      if (toolHasTerminalOutput(tool)) {
        return { ...tool, status: "done" };
      }
    }
    if (live) return tool;
    if (s === "running" || s === "active") {
      return { ...tool, status: "done" };
    }
    return tool;
  };

  const flushExploration = () => {
    if (explorationBatch.length === 0) return;
    blocks.push({ kind: "exploration", tools: [...explorationBatch] });
    explorationBatch = [];
  };

  const pushTool = (tool: ToolChunk) => {
    const normalized = normalizeTool(tool);
    if (getToolDisplayKind(normalized.title) === "exploration") {
      explorationBatch.push(normalized);
      return;
    }
    flushExploration();
    blocks.push({ kind: "tool", tool: normalized });
  };

  for (const chunk of chunks) {
    switch (chunk.type) {
      case "tool":
        if (chunk.title === "batch") {
          for (const nested of flattenBatch(chunk)) pushTool(nested);
        } else {
          pushTool(chunk);
        }
        break;
      case "text":
        flushExploration();
        if (chunk.content.trim()) {
          blocks.push({ kind: "text", content: chunk.content });
        }
        break;
      case "notice":
        flushExploration();
        if (chunk.content.trim()) {
          blocks.push({
            kind: "notice",
            tone: chunk.tone,
            title: chunk.title,
            content: chunk.content,
          });
        }
        break;
      case "thinking":
        flushExploration();
        if (chunk.content.trim()) {
          blocks.push({ kind: "thinking", content: chunk.content });
        }
        break;
      case "image":
        flushExploration();
        blocks.push({
          kind: "image",
          src: `data:${chunk.mimeType};base64,${chunk.dataBase64}`,
          name: chunk.name,
        });
        break;
      case "skill":
        flushExploration();
        blocks.push({
          kind: "skill",
          name: chunk.name,
          description: chunk.description,
        });
        break;
      case "fileRef":
        flushExploration();
        blocks.push({
          kind: "fileRef",
          rootId: chunk.rootId,
          path: chunk.path,
          lineStart: chunk.lineStart,
          lineEnd: chunk.lineEnd,
        });
        break;
    }
  }

  flushExploration();
  return blocks;
}

export function parseChatBlocks(chunksJson: string, live = false): RenderBlock[] {
  try {
    return buildBlocks(JSON.parse(chunksJson) as ChatChunk[], live);
  } catch {
    return [{ kind: "text", content: chunksJson }];
  }
}
