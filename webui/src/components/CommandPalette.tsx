import {
  type Component,
  For,
  Show,
  createEffect,
  createMemo,
  createResource,
  createSignal,
  onCleanup,
  onMount,
} from "solid-js";
import { getCanvas } from "@/lib/api/canvas";
import type { CanvasNode } from "@/lib/api/types";
import { cn } from "@/lib/cn";

type ActionId =
  | "create-task"
  | "create-document"
  | "create-goal"
  | "create-decision"
  | "open-librarian"
  | "reset-layout";

interface PaletteItem {
  id: string;
  kind: "node" | "action";
  label: string;
  hint: string;
  score: number;
  node?: CanvasNode;
  action?: ActionId;
}

const NODE_KIND_COLOR: Record<string, string> = {
  task: "oklch(var(--signal-amber))",
  thread: "oklch(var(--brand))",
  component: "oklch(var(--signal-green))",
  entity: "oklch(var(--signal-blue))",
  convention: "oklch(0.72 0.1 300)",
  decision: "oklch(var(--signal-amber))",
  fact: "oklch(var(--muted-foreground))",
  goal: "oklch(var(--signal-green))",
  document: "oklch(var(--signal-blue))",
};

const ACTION_ENTRIES: Array<{ id: ActionId; label: string; hint: string; aliases: string[] }> = [
  { id: "create-task", label: "Create task", hint: "New actionable item", aliases: ["new", "add"] },
  { id: "create-document", label: "Create document", hint: "Free-form note", aliases: ["new", "note", "add"] },
  { id: "create-goal", label: "Create goal", hint: "Objective or outcome", aliases: ["new", "add"] },
  { id: "create-decision", label: "Create decision", hint: "Captured choice", aliases: ["new", "add"] },
  { id: "open-librarian", label: "Open librarian", hint: "Project-wide chat", aliases: ["chat"] },
  { id: "reset-layout", label: "Reset canvas layout", hint: "Forget positions", aliases: ["clear"] },
];

function scoreMatch(query: string, target: string): number {
  if (!query) return 0;
  const q = query.toLowerCase();
  const t = target.toLowerCase();
  if (t === q) return 1000;
  if (t.startsWith(q)) return 500 + (100 - Math.min(100, t.length));
  const idx = t.indexOf(q);
  if (idx === 0) return 400;
  if (idx > 0) return 200 - idx;
  // fuzzy: all query chars appear in order
  let pos = 0;
  for (const ch of q) {
    const found = t.indexOf(ch, pos);
    if (found === -1) return -1;
    pos = found + 1;
  }
  return 50 - pos;
}

interface CommandPaletteProps {
  projectId: number;
  open: boolean;
  onClose: () => void;
  onSelectThread: (threadId: string) => void;
  onFocusNode: (nodeKey: string) => void;
  onAction: (action: ActionId) => void;
  onQueryChange?: (q: string) => void;
}

const CommandPalette: Component<CommandPaletteProps> = (props) => {
  const [query, setQuery] = createSignal("");
  const [activeIdx, setActiveIdx] = createSignal(0);
  let inputRef: HTMLInputElement | undefined;

  const [view, { refetch }] = createResource(
    () => (props.open ? props.projectId : undefined),
    async (pid) => (pid != null ? await getCanvas(pid) : undefined),
  );

  createEffect(() => {
    if (props.open) {
      setQuery("");
      setActiveIdx(0);
      void refetch();
      props.onQueryChange?.("");
      queueMicrotask(() => inputRef?.focus());
    } else {
      props.onQueryChange?.("");
    }
  });

  const items = createMemo<PaletteItem[]>(() => {
    const q = query().trim();
    const nodes = view()?.nodes ?? [];
    const results: PaletteItem[] = [];

    // Nodes
    for (const n of nodes) {
      if (n.kind === "thread" && (n.status === "archived" || n.status === "done")) continue;
      const labelScore = scoreMatch(q, n.label);
      const contentScore = n.content ? Math.max(0, scoreMatch(q, n.content) - 100) : 0;
      const kindScore = scoreMatch(q, n.kind);
      const score = q
        ? Math.max(labelScore, contentScore, kindScore)
        : 0;
      if (q && score < 0) continue;
      results.push({
        id: `node:${n.kind}:${n.id}`,
        kind: "node",
        label: n.label,
        hint: n.kind,
        score,
        node: n,
      });
    }

    // Actions
    for (const a of ACTION_ENTRIES) {
      const labelScore = scoreMatch(q, a.label);
      const aliasScore = Math.max(0, ...a.aliases.map((al) => scoreMatch(q, al)));
      const hintScore = Math.max(0, scoreMatch(q, a.hint) - 50);
      const score = q ? Math.max(labelScore, aliasScore, hintScore) : 10;
      if (q && score < 0) continue;
      results.push({
        id: `action:${a.id}`,
        kind: "action",
        label: a.label,
        hint: a.hint,
        score,
        action: a.id,
      });
    }

    return results.sort((a, b) => b.score - a.score).slice(0, 40);
  });

  // Ensure activeIdx in bounds
  createEffect(() => {
    const n = items().length;
    if (activeIdx() >= n) setActiveIdx(Math.max(0, n - 1));
  });

  const select = (item: PaletteItem) => {
    if (item.kind === "node" && item.node) {
      const n = item.node;
      if (n.kind === "thread") {
        props.onSelectThread(n.id);
      } else {
        props.onFocusNode(`${n.kind}:${n.id}`);
      }
    } else if (item.action) {
      props.onAction(item.action);
    }
    props.onClose();
  };

  const handleKey = (e: KeyboardEvent) => {
    if (!props.open) return;
    if (e.key === "Escape") {
      e.preventDefault();
      props.onClose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setActiveIdx((i) => Math.min(items().length - 1, i + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIdx((i) => Math.max(0, i - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const item = items()[activeIdx()];
      if (item) select(item);
    }
  };

  let paletteRef: HTMLDivElement | undefined;
  const handleOutsideClick = (e: MouseEvent) => {
    if (!props.open) return;
    const target = e.target as HTMLElement | null;
    if (paletteRef && target && !paletteRef.contains(target)) {
      props.onClose();
    }
  };

  onMount(() => {
    window.addEventListener("keydown", handleKey);
    window.addEventListener("mousedown", handleOutsideClick);
    onCleanup(() => window.removeEventListener("keydown", handleKey));
    onCleanup(() => window.removeEventListener("mousedown", handleOutsideClick));
  });

  return (
    <Show when={props.open}>
      <div class="command-palette-overlay">
        <div
          ref={paletteRef}
          class="command-palette"
          role="dialog"
          aria-modal="false"
          aria-label="Command palette"
        >
          <div class="command-palette-input-row">
            <svg
              width="14"
              height="14"
              viewBox="0 0 16 16"
              fill="none"
              stroke="currentColor"
              stroke-width="1.5"
              class="command-palette-icon"
            >
              <circle cx="7" cy="7" r="4.5" />
              <path d="M11 11L14 14" />
            </svg>
            <input
              ref={inputRef}
              class="command-palette-input"
              type="text"
              value={query()}
              placeholder="Jump to anything…"
              onInput={(e) => {
                const v = e.currentTarget.value;
                setQuery(v);
                props.onQueryChange?.(v);
              }}
            />
            <span class="command-palette-hint">↑↓ ↵ esc</span>
          </div>
          <div class="command-palette-body">
            <Show
              when={items().length > 0}
              fallback={
                <div class="command-palette-empty">
                  {view.loading ? "loading…" : "No matches"}
                </div>
              }
            >
              <For each={items()}>
                {(item, idx) => (
                  <button
                    type="button"
                    class={cn(
                      "command-palette-item",
                      idx() === activeIdx() && "is-active",
                    )}
                    onMouseEnter={() => setActiveIdx(idx())}
                    onClick={() => select(item)}
                  >
                    <Show
                      when={item.kind === "node"}
                      fallback={
                        <span class="command-palette-item-icon" aria-hidden="true">
                          <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.5">
                            <path d="M6 2v8M2 6h8" />
                          </svg>
                        </span>
                      }
                    >
                      <span
                        class="command-palette-item-dot"
                        style={{
                          background:
                            NODE_KIND_COLOR[item.node?.kind ?? "task"] ??
                            "oklch(var(--muted-foreground))",
                        }}
                      />
                    </Show>
                    <span class="command-palette-item-kind">{item.hint}</span>
                    <span class="command-palette-item-label">{item.label}</span>
                  </button>
                )}
              </For>
            </Show>
          </div>
        </div>
      </div>
    </Show>
  );
};

export default CommandPalette;
