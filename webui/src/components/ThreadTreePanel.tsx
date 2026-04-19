import {
  type Component,
  For,
  Show,
  createMemo,
  createResource,
  createSignal,
} from "solid-js";
import {
  discardThread,
  inspectThread,
  listThreads,
  mergeThread,
  mergeThreadRetry,
  spawnThread,
} from "@/lib/api/threads";
import type { ShepherdThread, ThreadInspection } from "@/lib/api/types";
import { cn } from "@/lib/cn";

interface ThreadTreePanelProps {
  projectId: number;
  /** Called when the user clicks a thread row; upstream can navigate. */
  onOpenThread?: (threadId: string) => void;
}

type PresetKey = "coder" | "reviewer" | "researcher" | "planner" | "coordinator" | "custom";

const PRESETS: Record<Exclude<PresetKey, "custom">, string[]> = {
  coder: ["workspace_read", "workspace_write", "shell"],
  reviewer: ["workspace_read", "graph_read"],
  researcher: ["workspace_read", "graph_read", "web_search"],
  planner: ["graph_read", "graph_edit", "spawn_thread"],
  coordinator: ["spawn_thread", "graph_read"],
};

const ALL_CAPS = [
  "workspace_read",
  "workspace_write",
  "graph_read",
  "graph_edit",
  "shell",
  "web_search",
  "spawn_thread",
  "mcp",
];

interface TreeNode {
  thread: ShepherdThread;
  children: TreeNode[];
}

function buildTree(threads: ShepherdThread[]): TreeNode[] {
  const byId = new Map<string, TreeNode>();
  for (const t of threads) byId.set(t.id, { thread: t, children: [] });
  const roots: TreeNode[] = [];
  for (const node of byId.values()) {
    const pid = node.thread.parent_id ?? null;
    if (pid && byId.has(pid)) {
      byId.get(pid)!.children.push(node);
    } else {
      roots.push(node);
    }
  }
  const sortByActivity = (a: TreeNode, b: TreeNode) =>
    b.thread.last_activity_at.localeCompare(a.thread.last_activity_at);
  const walk = (nodes: TreeNode[]) => {
    nodes.sort(sortByActivity);
    for (const n of nodes) walk(n.children);
  };
  walk(roots);
  return roots;
}

const ThreadTreePanel: Component<ThreadTreePanelProps> = (props) => {
  const [threads, { refetch }] = createResource(
    () => props.projectId,
    async (projectId) => {
      const summaries = await listThreads(projectId);
      return summaries.map((s) => s.thread);
    },
  );
  const tree = createMemo(() => buildTree(threads() ?? []));
  const [showSpawn, setShowSpawn] = createSignal(false);
  const [spawnParent, setSpawnParent] = createSignal<string | null>(null);
  const [inspection, setInspection] = createSignal<ThreadInspection | null>(null);
  const [error, setError] = createSignal<string | null>(null);

  const openSpawn = (parentId: string | null) => {
    setSpawnParent(parentId);
    setShowSpawn(true);
  };

  const onSpawned = async () => {
    setShowSpawn(false);
    await refetch();
  };

  const doInspect = async (threadId: string) => {
    try {
      setInspection(await inspectThread(props.projectId, threadId));
    } catch (err) {
      setError(String(err));
    }
  };

  const doMerge = async (threadId: string, retry = false) => {
    try {
      const outcome = retry
        ? await mergeThreadRetry(props.projectId, threadId)
        : await mergeThread(props.projectId, threadId);
      await refetch();
      if (outcome.state === "conflict") {
        setInspection(null);
        setError(`Merge conflict: ${outcome.files.join(", ")}`);
      }
    } catch (err) {
      setError(String(err));
    }
  };

  const doDiscard = async (threadId: string) => {
    try {
      await discardThread(props.projectId, threadId);
      await refetch();
    } catch (err) {
      setError(String(err));
    }
  };

  return (
    <div class="flex flex-col gap-3 p-3 text-sm">
      <div class="flex items-center justify-between">
        <div class="font-mono text-xs uppercase tracking-wider text-muted-foreground">
          Threads
        </div>
        <button
          type="button"
          class="px-2 py-1 text-xs border border-border rounded bg-secondary hover:bg-accent"
          onClick={() => openSpawn(null)}
        >
          + Spawn
        </button>
      </div>

      <Show when={error()}>
        <div class="px-2 py-1 text-xs text-signal-red border border-signal-red/30 bg-signal-red/5 rounded">
          {error()}
          <button
            class="ml-2 underline"
            type="button"
            onClick={() => setError(null)}
          >
            dismiss
          </button>
        </div>
      </Show>

      <div class="flex flex-col gap-0.5">
        <For each={tree()}>
          {(node) => (
            <ThreadNodeRow
              node={node}
              depth={0}
              onOpenThread={props.onOpenThread}
              onSpawnChild={(id) => openSpawn(id)}
              onInspect={doInspect}
              onMerge={(id) => doMerge(id, false)}
              onMergeRetry={(id) => doMerge(id, true)}
              onDiscard={doDiscard}
            />
          )}
        </For>
        <Show when={(threads() ?? []).length === 0}>
          <div class="text-xs text-muted-foreground px-2 py-4">
            No threads yet. Spawn one to begin.
          </div>
        </Show>
      </div>

      <Show when={showSpawn()}>
        <SpawnDialog
          projectId={props.projectId}
          parentId={spawnParent()}
          onCancel={() => setShowSpawn(false)}
          onSpawned={onSpawned}
        />
      </Show>

      <Show when={inspection()}>
        <InspectionPanel
          data={inspection()!}
          onClose={() => setInspection(null)}
          onMerge={() => doMerge(inspection()!.thread_id, false)}
          onMergeRetry={() => doMerge(inspection()!.thread_id, true)}
          onDiscard={() => doDiscard(inspection()!.thread_id)}
        />
      </Show>
    </div>
  );
};

interface ThreadNodeRowProps {
  node: TreeNode;
  depth: number;
  onOpenThread?: (threadId: string) => void;
  onSpawnChild: (parentId: string) => void;
  onInspect: (threadId: string) => void;
  onMerge: (threadId: string) => void;
  onMergeRetry: (threadId: string) => void;
  onDiscard: (threadId: string) => void;
}

const ThreadNodeRow: Component<ThreadNodeRowProps> = (props) => {
  const thread = () => props.node.thread;
  const indent = () => props.depth * 16;

  const canMerge = () =>
    ["unmerged", "conflict"].includes(thread().merge_status ?? "");
  const isConflict = () => thread().merge_status === "conflict";
  const hasWorkspace = () => !!thread().workspace_path;

  return (
    <div class="flex flex-col">
      <div
        class="group flex items-center gap-2 px-2 py-1 rounded hover:bg-accent/40 cursor-pointer"
        style={{ "padding-left": `${indent() + 8}px` }}
        onClick={() => props.onOpenThread?.(thread().id)}
      >
        <StatusDot status={thread().status} />
        <span class="flex-1 truncate">{thread().title}</span>
        <MergeStatusBadge status={thread().merge_status ?? "none"} />
        <div class="opacity-0 group-hover:opacity-100 flex gap-1 transition-opacity">
          <button
            type="button"
            title="Inspect"
            class="text-xs px-1.5 py-0.5 border border-border rounded bg-background hover:bg-secondary"
            onClick={(e) => {
              e.stopPropagation();
              props.onInspect(thread().id);
            }}
          >
            ⓘ
          </button>
          <Show when={canMerge() && hasWorkspace()}>
            <button
              type="button"
              title={isConflict() ? "Retry merge" : "Merge"}
              class="text-xs px-1.5 py-0.5 border border-border rounded bg-background hover:bg-secondary"
              onClick={(e) => {
                e.stopPropagation();
                isConflict()
                  ? props.onMergeRetry(thread().id)
                  : props.onMerge(thread().id);
              }}
            >
              ⤴
            </button>
          </Show>
          <Show when={hasWorkspace()}>
            <button
              type="button"
              title="Discard workspace copy"
              class="text-xs px-1.5 py-0.5 border border-border rounded bg-background hover:bg-secondary"
              onClick={(e) => {
                e.stopPropagation();
                props.onDiscard(thread().id);
              }}
            >
              ✕
            </button>
          </Show>
          <button
            type="button"
            title="Spawn child"
            class="text-xs px-1.5 py-0.5 border border-border rounded bg-background hover:bg-secondary"
            onClick={(e) => {
              e.stopPropagation();
              props.onSpawnChild(thread().id);
            }}
          >
            +
          </button>
        </div>
      </div>
      <Show when={hasWorkspace() && thread().workspace_path}>
        <div
          class="text-[10px] text-muted-foreground font-mono pl-1 truncate"
          style={{ "padding-left": `${indent() + 32}px` }}
        >
          {thread().workspace_path}
        </div>
      </Show>
      <For each={props.node.children}>
        {(child) => (
          <ThreadNodeRow
            node={child}
            depth={props.depth + 1}
            onOpenThread={props.onOpenThread}
            onSpawnChild={props.onSpawnChild}
            onInspect={props.onInspect}
            onMerge={props.onMerge}
            onMergeRetry={props.onMergeRetry}
            onDiscard={props.onDiscard}
          />
        )}
      </For>
    </div>
  );
};

const StatusDot: Component<{ status: string }> = (props) => {
  const tone = () => {
    switch (props.status) {
      case "running":
      case "active":
      case "starting":
        return "bg-signal-green";
      case "todo":
      case "queued":
        return "bg-signal-amber";
      case "blocked":
      case "failed":
        return "bg-signal-red";
      case "done":
      case "archived":
        return "bg-muted-foreground";
      default:
        return "bg-muted-foreground/50";
    }
  };
  return (
    <span
      class={cn("inline-block h-2 w-2 rounded-full", tone())}
      title={props.status}
    />
  );
};

const MergeStatusBadge: Component<{ status: string }> = (props) => {
  if (props.status === "none") return null;
  const classes = () => {
    switch (props.status) {
      case "merged":
        return "border-signal-green/40 text-signal-green bg-signal-green/5";
      case "conflict":
        return "border-signal-red/40 text-signal-red bg-signal-red/5";
      case "discarded":
        return "border-muted-foreground/30 text-muted-foreground";
      case "orphaned":
        return "border-signal-amber/40 text-signal-amber bg-signal-amber/5";
      default:
        return "border-border text-muted-foreground";
    }
  };
  return (
    <span
      class={cn(
        "px-1.5 py-[1px] text-[10px] uppercase tracking-wider font-mono border rounded",
        classes(),
      )}
    >
      {props.status}
    </span>
  );
};

interface SpawnDialogProps {
  projectId: number;
  parentId: string | null;
  onCancel: () => void;
  onSpawned: () => void;
}

const SpawnDialog: Component<SpawnDialogProps> = (props) => {
  const [preset, setPreset] = createSignal<PresetKey>("coder");
  const [title, setTitle] = createSignal("");
  const [objective, setObjective] = createSignal("");
  const [customCaps, setCustomCaps] = createSignal<Set<string>>(
    new Set(PRESETS.coder),
  );
  const [submitting, setSubmitting] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const effectiveCaps = () => {
    if (preset() === "custom") return Array.from(customCaps());
    return PRESETS[preset()];
  };

  const submit = async () => {
    const obj = objective().trim();
    if (!obj) {
      setError("Objective is required");
      return;
    }
    setSubmitting(true);
    setError(null);
    try {
      await spawnThread(props.projectId, {
        objective: obj,
        title: title().trim() || undefined,
        parent_id: props.parentId ?? undefined,
        capabilities: effectiveCaps(),
      });
      props.onSpawned();
    } catch (err) {
      setError(String(err));
    } finally {
      setSubmitting(false);
    }
  };

  const toggleCap = (cap: string) => {
    setCustomCaps((prev) => {
      const next = new Set(prev);
      if (next.has(cap)) next.delete(cap);
      else next.add(cap);
      return next;
    });
    setPreset("custom");
  };

  return (
    <div
      class="fixed inset-0 z-50 bg-background/80 backdrop-blur-sm flex items-center justify-center p-4"
      onClick={props.onCancel}
    >
      <div
        class="w-full max-w-lg bg-background border border-border rounded-lg p-5 flex flex-col gap-3"
        onClick={(e) => e.stopPropagation()}
      >
        <div class="flex items-center justify-between">
          <div class="font-mono uppercase text-xs tracking-wider text-muted-foreground">
            Spawn Thread
          </div>
          <Show when={props.parentId}>
            <div class="text-[10px] font-mono text-muted-foreground">
              parent: {props.parentId?.slice(0, 8)}…
            </div>
          </Show>
        </div>

        <label class="flex flex-col gap-1">
          <span class="text-xs text-muted-foreground">Preset</span>
          <select
            class="bg-secondary border border-border rounded px-2 py-1 text-sm"
            value={preset()}
            onChange={(e) => {
              const key = e.currentTarget.value as PresetKey;
              setPreset(key);
              if (key !== "custom") setCustomCaps(new Set(PRESETS[key]));
            }}
          >
            <option value="coder">coder</option>
            <option value="reviewer">reviewer</option>
            <option value="researcher">researcher</option>
            <option value="planner">planner</option>
            <option value="coordinator">coordinator</option>
            <option value="custom">custom</option>
          </select>
        </label>

        <div class="flex flex-wrap gap-1">
          <For each={ALL_CAPS}>
            {(cap) => (
              <button
                type="button"
                class={cn(
                  "px-2 py-0.5 text-[10px] font-mono border rounded",
                  effectiveCaps().includes(cap)
                    ? "bg-accent/40 border-accent text-foreground"
                    : "bg-background border-border text-muted-foreground",
                )}
                onClick={() => toggleCap(cap)}
              >
                {cap}
              </button>
            )}
          </For>
        </div>

        <label class="flex flex-col gap-1">
          <span class="text-xs text-muted-foreground">Title (optional)</span>
          <input
            class="bg-secondary border border-border rounded px-2 py-1 text-sm"
            value={title()}
            onInput={(e) => setTitle(e.currentTarget.value)}
            placeholder="derived from objective if empty"
          />
        </label>

        <label class="flex flex-col gap-1">
          <span class="text-xs text-muted-foreground">Objective</span>
          <textarea
            class="bg-secondary border border-border rounded px-2 py-1 text-sm min-h-[120px] font-mono"
            value={objective()}
            onInput={(e) => setObjective(e.currentTarget.value)}
            placeholder="what should this thread accomplish?"
          />
        </label>

        <Show when={error()}>
          <div class="text-xs text-signal-red">{error()}</div>
        </Show>

        <div class="flex gap-2 justify-end pt-2">
          <button
            type="button"
            class="px-3 py-1 text-sm border border-border rounded hover:bg-accent/20"
            onClick={props.onCancel}
            disabled={submitting()}
          >
            Cancel
          </button>
          <button
            type="button"
            class="px-3 py-1 text-sm bg-accent text-background rounded hover:bg-accent/80"
            onClick={submit}
            disabled={submitting()}
          >
            {submitting() ? "Spawning…" : "Spawn"}
          </button>
        </div>
      </div>
    </div>
  );
};

interface InspectionPanelProps {
  data: ThreadInspection;
  onClose: () => void;
  onMerge: () => void;
  onMergeRetry: () => void;
  onDiscard: () => void;
}

const InspectionPanel: Component<InspectionPanelProps> = (props) => {
  const d = () => props.data;
  const copyWorkspacePath = async () => {
    if (!d().workspace_path) return;
    try {
      await navigator.clipboard.writeText(d().workspace_path!);
    } catch {
      /* ignore */
    }
  };

  return (
    <div
      class="fixed inset-0 z-50 bg-background/80 backdrop-blur-sm flex items-center justify-center p-4"
      onClick={props.onClose}
    >
      <div
        class="w-full max-w-2xl bg-background border border-border rounded-lg p-5 flex flex-col gap-3"
        onClick={(e) => e.stopPropagation()}
      >
        <div class="flex items-center justify-between">
          <div class="font-mono uppercase text-xs tracking-wider text-muted-foreground">
            Inspect thread
          </div>
          <button
            type="button"
            class="text-xs text-muted-foreground hover:text-foreground"
            onClick={props.onClose}
          >
            ✕
          </button>
        </div>

        <div class="grid grid-cols-2 gap-2 text-xs">
          <div>
            <span class="text-muted-foreground">Thread </span>
            <span class="font-mono">{d().thread_id.slice(0, 8)}…</span>
          </div>
          <div>
            <span class="text-muted-foreground">Status </span>
            <span>{d().status}</span>
          </div>
          <div>
            <span class="text-muted-foreground">Merge </span>
            <MergeStatusBadge status={d().merge_status} />
          </div>
          <div>
            <span class="text-muted-foreground">Parent </span>
            <span class="font-mono">
              {d().parent_id ? `${d().parent_id!.slice(0, 8)}…` : "—"}
            </span>
          </div>
        </div>

        <Show when={d().workspace_path}>
          <div class="flex items-center gap-2 text-xs bg-secondary/40 border border-border rounded p-2">
            <code class="flex-1 truncate font-mono">{d().workspace_path}</code>
            <button
              type="button"
              class="px-2 py-0.5 text-[10px] border border-border rounded hover:bg-accent/20"
              onClick={copyWorkspacePath}
            >
              copy path
            </button>
          </div>
        </Show>

        <Show when={d().diff}>
          <div class="border border-border rounded overflow-hidden">
            <div class="flex items-center justify-between px-2 py-1 text-xs bg-secondary/40 border-b border-border">
              <span>
                {d().diff!.files.length} files changed
              </span>
              <span class="font-mono">
                +{d().diff!.total_additions} −{d().diff!.total_deletions}
              </span>
            </div>
            <div class="max-h-64 overflow-auto text-xs font-mono">
              <For each={d().diff!.files}>
                {(f) => (
                  <div class="flex items-center gap-2 px-2 py-0.5 border-b border-border/40">
                    <span
                      class={cn(
                        "w-14 text-[10px] uppercase",
                        f.status === "added" && "text-signal-green",
                        f.status === "deleted" && "text-signal-red",
                        f.status === "modified" && "text-signal-amber",
                      )}
                    >
                      {f.status}
                    </span>
                    <span class="flex-1 truncate">{f.path}</span>
                    <span class="text-muted-foreground text-[10px]">
                      +{f.additions} −{f.deletions}
                    </span>
                  </div>
                )}
              </For>
            </div>
          </div>
        </Show>

        <Show when={d().final_output}>
          <div class="border border-border rounded overflow-hidden">
            <div class="px-2 py-1 text-xs bg-secondary/40 border-b border-border">
              final output
            </div>
            <pre class="p-2 text-xs max-h-48 overflow-auto whitespace-pre-wrap">
              {d().final_output}
            </pre>
          </div>
        </Show>

        <div class="flex gap-2 justify-end pt-1">
          <Show when={d().workspace_path}>
            <button
              type="button"
              class="px-3 py-1 text-sm border border-border rounded hover:bg-accent/20"
              onClick={props.onDiscard}
            >
              Discard
            </button>
            <Show
              when={d().merge_status === "conflict"}
              fallback={
                <button
                  type="button"
                  class="px-3 py-1 text-sm bg-signal-green text-background rounded hover:bg-signal-green/80"
                  onClick={props.onMerge}
                >
                  Merge
                </button>
              }
            >
              <button
                type="button"
                class="px-3 py-1 text-sm bg-signal-amber text-background rounded hover:bg-signal-amber/80"
                onClick={props.onMergeRetry}
              >
                Retry merge
              </button>
            </Show>
          </Show>
        </div>
      </div>
    </div>
  );
};

export default ThreadTreePanel;
