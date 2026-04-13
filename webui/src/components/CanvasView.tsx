import {
  type Component,
  For,
  Show,
  createSignal,
  createEffect,
  createMemo,
  onMount,
  onCleanup,
  on,
} from "solid-js";
import type { CanvasNode, CanvasEdge, CanvasPosition } from "@/lib/api/types";
import { getCanvas, patchCanvasLayout, resetCanvasLayout } from "@/lib/api/canvas";
import {
  createTask,
  updateTask,
  deleteTask,
  dispatchTask,
  reviewAction,
} from "@/lib/api/tasks";
import { cn } from "@/lib/cn";
import { renderMarkdown } from "@/lib/markdown";

interface CanvasViewProps {
  projectId: number;
  refreshNonce?: number;
  onOpenThread?: (threadId: string) => void;
}

interface PositionedNode extends CanvasNode {
  x: number;
  y: number;
}

// Visual config per kind
const NODE_STYLES: Record<string, { color: string; width: number; height: number }> = {
  task: { color: "oklch(var(--signal-amber))", width: 200, height: 80 },
  thread: { color: "oklch(var(--brand))", width: 200, height: 80 },
  component: { color: "oklch(var(--signal-green))", width: 170, height: 70 },
  entity: { color: "oklch(var(--signal-blue))", width: 170, height: 70 },
  convention: { color: "oklch(0.72 0.1 300)", width: 170, height: 70 },
  decision: { color: "oklch(var(--signal-amber))", width: 170, height: 70 },
  fact: { color: "oklch(var(--muted-foreground))", width: 170, height: 70 },
  goal: { color: "oklch(var(--signal-green))", width: 170, height: 70 },
  document: { color: "oklch(var(--signal-blue))", width: 170, height: 70 },
};

const KINDS = [
  "task",
  "thread",
  "component",
  "entity",
  "decision",
  "fact",
  "goal",
  "convention",
  "document",
] as const;

const CanvasView: Component<CanvasViewProps> = (props) => {
  const [nodes, setNodes] = createSignal<PositionedNode[]>([]);
  const [edges, setEdges] = createSignal<CanvasEdge[]>([]);
  const [loading, setLoading] = createSignal(true);

  // Camera state
  const [panX, setPanX] = createSignal(0);
  const [panY, setPanY] = createSignal(0);
  const [zoom, setZoom] = createSignal(1);

  // Interaction state
  const [draggingId, setDraggingId] = createSignal<string | null>(null);
  const [isPanning, setIsPanning] = createSignal(false);
  const [expandedId, setExpandedId] = createSignal<string | null>(null);

  // Filters
  const [hiddenKinds, setHiddenKinds] = createSignal<Set<string>>(new Set());
  const [hiddenStatuses, setHiddenStatuses] = createSignal<Set<string>>(new Set(["done"]));
  const [searchText, setSearchText] = createSignal("");
  const [layoutMode, setLayoutMode] = createSignal<"free" | "grid" | "grouped" | "force" | "radial">("free");
  const [viewMenuOpen, setViewMenuOpen] = createSignal(false);
  const [filterMenuOpen, setFilterMenuOpen] = createSignal(false);

  let containerRef: HTMLDivElement | undefined;
  let surfaceRef: HTMLDivElement | undefined;

  const nodeKey = (n: CanvasNode) => `${n.kind}:${n.id}`;

  const loadCanvas = async () => {
    try {
      const view = await getCanvas(props.projectId);
      // Apply positions: use stored layout, fall back to auto-grid
      const positioned: PositionedNode[] = view.nodes.map((n, i) => {
        const key = nodeKey(n);
        const stored = view.layout[key];
        if (stored) {
          return { ...n, x: stored.x, y: stored.y };
        }
        // Grid fallback — spread out by index
        const col = i % 5;
        const row = Math.floor(i / 5);
        return { ...n, x: 200 + col * 250, y: 200 + row * 150 };
      });
      setNodes(positioned);
      setEdges(view.edges);
      setLoading(false);
    } catch (e) {
      console.error("Failed to load canvas", e);
      setLoading(false);
    }
  };

  createEffect(
    on(
      () => [props.projectId, props.refreshNonce],
      () => {
        void loadCanvas();
      },
    ),
  );

  // Filter logic
  const visibleNodes = createMemo(() => {
    const hidden = hiddenKinds();
    const hiddenSt = hiddenStatuses();
    const search = searchText().toLowerCase().trim();
    return nodes().filter((n) => {
      if (hidden.has(n.kind)) return false;
      if (n.status && hiddenSt.has(n.status)) return false;
      if (search) {
        const inLabel = n.label.toLowerCase().includes(search);
        const inContent = (n.content ?? "").toLowerCase().includes(search);
        if (!inLabel && !inContent) return false;
      }
      return true;
    });
  });

  const visibleEdges = createMemo(() => {
    const visibleKeys = new Set(visibleNodes().map(nodeKey));
    return edges().filter((e) => visibleKeys.has(e.from) && visibleKeys.has(e.to));
  });

  // Save layout (debounced)
  let saveTimeout: number | undefined;
  const scheduleSave = () => {
    if (saveTimeout) clearTimeout(saveTimeout);
    saveTimeout = window.setTimeout(() => {
      const positions: Record<string, CanvasPosition> = {};
      for (const n of nodes()) {
        positions[nodeKey(n)] = { x: n.x, y: n.y };
      }
      patchCanvasLayout(props.projectId, positions).catch((e) =>
        console.error("Failed to save layout", e),
      );
    }, 600);
  };

  // Screen-to-world coords
  const screenToWorld = (sx: number, sy: number) => {
    if (!containerRef) return { x: 0, y: 0 };
    const rect = containerRef.getBoundingClientRect();
    return {
      x: (sx - rect.left) / zoom() - panX() / zoom(),
      y: (sy - rect.top) / zoom() - panY() / zoom(),
    };
  };

  // Pointer handlers
  const handleSurfacePointerDown = (e: PointerEvent) => {
    if (e.button !== 0) return;
    const target = e.target as HTMLElement;
    // Don't start panning if click started on a node, toolbar, or any interactive UI
    if (
      target.closest("[data-node-id]") ||
      target.closest(".canvas-toolbar") ||
      target.closest(".canvas-view-dropdown") ||
      target.closest(".canvas-filter-dropdown") ||
      target.closest(".canvas-zoom-indicator")
    ) {
      return;
    }
    setIsPanning(true);
    containerRef?.setPointerCapture(e.pointerId);
    const startX = e.clientX;
    const startY = e.clientY;
    const startPanX = panX();
    const startPanY = panY();

    const onMove = (me: PointerEvent) => {
      setPanX(startPanX + (me.clientX - startX));
      setPanY(startPanY + (me.clientY - startY));
    };
    const onUp = () => {
      setIsPanning(false);
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  const handleNodePointerDown = (node: PositionedNode, e: PointerEvent) => {
    e.stopPropagation();

    // Middle click or double-click → focus
    if (e.button === 1) {
      e.preventDefault();
      openFocus(node);
      return;
    }

    if (e.button !== 0) return;

    const startX = e.clientX;
    const startY = e.clientY;
    const origX = node.x;
    const origY = node.y;
    let moved = false;
    setDraggingId(nodeKey(node));

    const onMove = (me: PointerEvent) => {
      const dx = (me.clientX - startX) / zoom();
      const dy = (me.clientY - startY) / zoom();
      if (Math.abs(dx) > 3 || Math.abs(dy) > 3) moved = true;
      setNodes((prev) =>
        prev.map((n) =>
          nodeKey(n) === nodeKey(node) ? { ...n, x: origX + dx, y: origY + dy } : n,
        ),
      );
    };
    const onUp = () => {
      setDraggingId(null);
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      if (moved) {
        scheduleSave();
      } else {
        // Click without drag → toggle expand
        setExpandedId((curr) => (curr === nodeKey(node) ? null : nodeKey(node)));
      }
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  const handleWheel = (e: WheelEvent) => {
    e.preventDefault();
    const factor = e.deltaY > 0 ? 0.9 : 1.1;
    const newZoom = Math.max(0.2, Math.min(2.5, zoom() * factor));
    // Zoom toward mouse position
    if (containerRef) {
      const rect = containerRef.getBoundingClientRect();
      const mx = e.clientX - rect.left;
      const my = e.clientY - rect.top;
      const worldX = (mx - panX()) / zoom();
      const worldY = (my - panY()) / zoom();
      setPanX(mx - worldX * newZoom);
      setPanY(my - worldY * newZoom);
    }
    setZoom(newZoom);
  };

  const openFocus = (node: PositionedNode) => {
    if (node.kind === "thread") {
      props.onOpenThread?.(node.id);
    } else {
      // Keep expanded inline for now; full focus mode TBD
      setExpandedId(nodeKey(node));
    }
  };

  const toggleKind = (kind: string) => {
    setHiddenKinds((prev) => {
      const next = new Set(prev);
      if (next.has(kind)) next.delete(kind);
      else next.add(kind);
      return next;
    });
  };

  const toggleStatus = (status: string) => {
    setHiddenStatuses((prev) => {
      const next = new Set(prev);
      if (next.has(status)) next.delete(status);
      else next.add(status);
      return next;
    });
  };

  // Count nodes per kind and status (from all nodes, not filtered)
  const kindCounts = createMemo(() => {
    const counts: Record<string, number> = {};
    for (const n of nodes()) {
      counts[n.kind] = (counts[n.kind] ?? 0) + 1;
    }
    return counts;
  });

  const statusCounts = createMemo(() => {
    const counts: Record<string, number> = {};
    for (const n of nodes()) {
      if (n.status) counts[n.status] = (counts[n.status] ?? 0) + 1;
    }
    return counts;
  });

  const activeFilterCount = createMemo(() => {
    let count = 0;
    if (hiddenKinds().size > 0) count += hiddenKinds().size;
    if (hiddenStatuses().size > 0) count += hiddenStatuses().size;
    if (searchText().trim()) count += 1;
    return count;
  });

  const handleResetLayout = async () => {
    await resetCanvasLayout(props.projectId);
    await loadCanvas();
  };

  const applyLayout = (mode: "free" | "grid" | "grouped" | "force" | "radial") => {
    setLayoutMode(mode);
    setViewMenuOpen(false);
    if (mode === "free") return;

    const current = nodes();
    if (current.length === 0) return;

    if (mode === "grid") {
      const cols = Math.max(1, Math.ceil(Math.sqrt(current.length)));
      const repositioned = current.map((n, i) => ({
        ...n,
        x: 80 + (i % cols) * 260,
        y: 80 + Math.floor(i / cols) * 160,
      }));
      setNodes(repositioned);
      scheduleSave();
    } else if (mode === "grouped") {
      const byKind: Record<string, PositionedNode[]> = {};
      for (const n of current) {
        (byKind[n.kind] ??= []).push(n);
      }
      const kinds = Object.keys(byKind).sort();
      const repositioned: PositionedNode[] = [];
      kinds.forEach((kind, col) => {
        byKind[kind].forEach((n, row) => {
          repositioned.push({ ...n, x: 80 + col * 260, y: 80 + row * 120 });
        });
      });
      setNodes(repositioned);
      scheduleSave();
    } else if (mode === "force") {
      runForceSimulation();
    } else if (mode === "radial") {
      applyRadialLayout();
    }
  };

  // Force-directed layout: iterate a spring/charge simulation until it settles
  const runForceSimulation = () => {
    const current = nodes();
    if (current.length === 0) return;

    // Use a copy so we can mutate freely
    const sim = current.map((n) => ({
      ...n,
      vx: 0,
      vy: 0,
    }));

    const edgeLookup = edges();
    const nodeByKey = new Map<string, (typeof sim)[number]>();
    for (const n of sim) nodeByKey.set(nodeKey(n), n);

    // Seed positions in a circle if they're all stacked
    const allSame =
      sim.every((n) => n.x === sim[0].x && n.y === sim[0].y) || sim.some((n) => !isFinite(n.x));
    if (allSame) {
      const r = 300;
      sim.forEach((n, i) => {
        const a = (i / sim.length) * Math.PI * 2;
        n.x = Math.cos(a) * r + 400;
        n.y = Math.sin(a) * r + 300;
      });
    }

    const REPULSION = 25000; // node-node repulsion strength
    const SPRING = 0.02; // edge attraction
    const IDEAL_LEN = 200; // ideal edge length
    const DAMPING = 0.85;
    const CENTER_GRAVITY = 0.005;
    const ITERATIONS = 300;
    const MIN_DIST = 10;
    const MAX_VEL = 30;

    for (let iter = 0; iter < ITERATIONS; iter++) {
      // Repulsion between all pairs
      for (let i = 0; i < sim.length; i++) {
        for (let j = i + 1; j < sim.length; j++) {
          const a = sim[i];
          const b = sim[j];
          const dx = b.x - a.x;
          const dy = b.y - a.y;
          const dist2 = Math.max(dx * dx + dy * dy, MIN_DIST * MIN_DIST);
          const dist = Math.sqrt(dist2);
          const force = REPULSION / dist2;
          const fx = (dx / dist) * force;
          const fy = (dy / dist) * force;
          a.vx -= fx;
          a.vy -= fy;
          b.vx += fx;
          b.vy += fy;
        }
      }

      // Spring attraction along edges
      for (const edge of edgeLookup) {
        const a = nodeByKey.get(edge.from);
        const b = nodeByKey.get(edge.to);
        if (!a || !b) continue;
        const dx = b.x - a.x;
        const dy = b.y - a.y;
        const dist = Math.max(Math.sqrt(dx * dx + dy * dy), MIN_DIST);
        const displacement = dist - IDEAL_LEN;
        const force = SPRING * displacement;
        const fx = (dx / dist) * force;
        const fy = (dy / dist) * force;
        a.vx += fx;
        a.vy += fy;
        b.vx -= fx;
        b.vy -= fy;
      }

      // Gentle pull toward center
      for (const n of sim) {
        n.vx -= n.x * CENTER_GRAVITY;
        n.vy -= n.y * CENTER_GRAVITY;
      }

      // Integrate with damping and velocity clamp
      for (const n of sim) {
        n.vx *= DAMPING;
        n.vy *= DAMPING;
        const v = Math.sqrt(n.vx * n.vx + n.vy * n.vy);
        if (v > MAX_VEL) {
          n.vx = (n.vx / v) * MAX_VEL;
          n.vy = (n.vy / v) * MAX_VEL;
        }
        n.x += n.vx;
        n.y += n.vy;
      }
    }

    // Shift to positive coordinates (min at margin)
    const minX = Math.min(...sim.map((n) => n.x));
    const minY = Math.min(...sim.map((n) => n.y));
    const offsetX = 80 - minX;
    const offsetY = 80 - minY;

    const repositioned = current.map((n) => {
      const s = nodeByKey.get(nodeKey(n))!;
      return { ...n, x: s.x + offsetX, y: s.y + offsetY };
    });
    setNodes(repositioned);
    scheduleSave();
  };

  // Radial: place nodes around a circle, grouped by kind as arcs
  const applyRadialLayout = () => {
    const current = nodes();
    if (current.length === 0) return;

    const byKind: Record<string, PositionedNode[]> = {};
    for (const n of current) {
      (byKind[n.kind] ??= []).push(n);
    }
    const kinds = Object.keys(byKind).sort();

    const centerX = 500;
    const centerY = 400;
    const radius = Math.max(250, current.length * 25);

    const totalNodes = current.length;
    let cursor = 0;
    const positioned: PositionedNode[] = [];

    kinds.forEach((kind) => {
      const group = byKind[kind];
      group.forEach((n) => {
        const angle = (cursor / totalNodes) * Math.PI * 2 - Math.PI / 2;
        cursor++;
        positioned.push({
          ...n,
          x: centerX + Math.cos(angle) * radius,
          y: centerY + Math.sin(angle) * radius,
        });
      });
    });

    setNodes(positioned);
    scheduleSave();
  };

  const handleCreateTask = async () => {
    const title = window.prompt("Task title:");
    if (!title?.trim()) return;
    await createTask(props.projectId, { title: title.trim() });
    await loadCanvas();
  };

  // Keyboard: Esc collapses popup / closes menu
  onMount(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (viewMenuOpen()) {
          setViewMenuOpen(false);
        } else if (expandedId()) {
          setExpandedId(null);
        }
      }
    };
    document.addEventListener("keydown", handler);
    onCleanup(() => document.removeEventListener("keydown", handler));
  });

  // Close menus on outside click
  onMount(() => {
    const handler = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      if (viewMenuOpen() && !target.closest(".canvas-view-menu")) {
        setViewMenuOpen(false);
      }
      if (filterMenuOpen() && !target.closest(".canvas-filter-menu")) {
        setFilterMenuOpen(false);
      }
    };
    document.addEventListener("click", handler);
    onCleanup(() => document.removeEventListener("click", handler));
  });

  return (
    <div
      ref={containerRef}
      class="canvas-view relative h-full w-full overflow-hidden bg-background"
      style={{
        "touch-action": "none",
        cursor: isPanning() ? "grabbing" : "grab",
      }}
      onPointerDown={handleSurfacePointerDown}
      onWheel={handleWheel}
    >
      {/* Toolbar */}
      <div class="canvas-toolbar">
        <div class="canvas-toolbar-search">
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" class="canvas-toolbar-search-icon">
            <circle cx="5" cy="5" r="3.5" />
            <path d="M8 8L11 11" />
          </svg>
          <input
            type="text"
            class="canvas-search"
            placeholder="Search..."
            value={searchText()}
            onInput={(e) => setSearchText(e.currentTarget.value)}
          />
          <Show when={searchText()}>
            <button
              type="button"
              class="canvas-search-clear"
              onClick={() => setSearchText("")}
              title="Clear"
            >
              <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.3">
                <path d="M2 2L8 8M8 2L2 8" />
              </svg>
            </button>
          </Show>
        </div>

        {/* Filter dropdown trigger */}
        <div class="canvas-filter-menu">
          <button
            type="button"
            class="canvas-toolbar-btn canvas-filter-trigger"
            onClick={() => setFilterMenuOpen(!filterMenuOpen())}
            data-active={activeFilterCount() > 0}
            title="Filter nodes"
          >
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3">
              <path d="M1 2h10L7.5 7v3.5L4.5 12V7L1 2z" />
            </svg>
            <span>Filter</span>
            <Show when={activeFilterCount() > 0}>
              <span class="canvas-filter-badge">{activeFilterCount()}</span>
            </Show>
          </button>
          <Show when={filterMenuOpen()}>
            <div class="canvas-filter-dropdown" onPointerDown={(e) => e.stopPropagation()}>
              <div class="canvas-filter-section">
                <div class="canvas-filter-section-header">
                  <span>Kind</span>
                  <Show when={hiddenKinds().size > 0}>
                    <button
                      type="button"
                      class="canvas-filter-clear"
                      onClick={() => setHiddenKinds(new Set())}
                    >
                      show all
                    </button>
                  </Show>
                </div>
                <div class="canvas-filter-chips">
                  <For each={KINDS}>
                    {(kind) => {
                      const count = () => kindCounts()[kind] ?? 0;
                      return (
                        <button
                          type="button"
                          class="canvas-filter-chip"
                          data-active={!hiddenKinds().has(kind)}
                          data-kind={kind}
                          onClick={() => toggleKind(kind)}
                          disabled={count() === 0}
                        >
                          <span class="canvas-filter-chip-dot" />
                          <span>{kind}</span>
                          <span class="canvas-filter-chip-count">{count()}</span>
                        </button>
                      );
                    }}
                  </For>
                </div>
              </div>

              <div class="canvas-filter-divider" />

              <div class="canvas-filter-section">
                <div class="canvas-filter-section-header">
                  <span>Status</span>
                  <Show when={hiddenStatuses().size > 0}>
                    <button
                      type="button"
                      class="canvas-filter-clear"
                      onClick={() => setHiddenStatuses(new Set())}
                    >
                      show all
                    </button>
                  </Show>
                </div>
                <div class="canvas-filter-chips">
                  <For each={["todo", "active", "review", "done", "running", "waiting", "blocked", "failed"]}>
                    {(status) => {
                      const count = () => statusCounts()[status] ?? 0;
                      if (count() === 0) return null;
                      return (
                        <button
                          type="button"
                          class="canvas-filter-chip"
                          data-active={!hiddenStatuses().has(status)}
                          data-status={status}
                          onClick={() => toggleStatus(status)}
                        >
                          <span class="canvas-filter-chip-dot" />
                          <span>{status}</span>
                          <span class="canvas-filter-chip-count">{count()}</span>
                        </button>
                      );
                    }}
                  </For>
                </div>
              </div>
            </div>
          </Show>
        </div>

        <div class="canvas-toolbar-divider" />

        <div class="canvas-toolbar-actions">
          <div class="canvas-view-menu">
            <button
              type="button"
              class="canvas-toolbar-btn canvas-view-trigger"
              onClick={() => setViewMenuOpen(!viewMenuOpen())}
              title="Layout options"
            >
              <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3">
                <circle cx="3" cy="3" r="1.2" />
                <circle cx="9" cy="3" r="1.2" />
                <circle cx="6" cy="6" r="1.2" />
                <circle cx="3" cy="9" r="1.2" />
                <circle cx="9" cy="9" r="1.2" />
                <line x1="3" y1="3" x2="6" y2="6" />
                <line x1="9" y1="3" x2="6" y2="6" />
                <line x1="3" y1="9" x2="6" y2="6" />
                <line x1="9" y1="9" x2="6" y2="6" />
              </svg>
              <span>View: {layoutMode()}</span>
              <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.3">
                <path d="M2 4L5 7L8 4" />
              </svg>
            </button>
            <Show when={viewMenuOpen()}>
              <div
                class="canvas-view-dropdown"
                onPointerDown={(e) => e.stopPropagation()}
              >
                <button
                  type="button"
                  class="canvas-view-item"
                  data-active={layoutMode() === "free"}
                  onClick={() => applyLayout("free")}
                >
                  <span class="canvas-view-item-label">Free</span>
                  <span class="canvas-view-item-hint">Drag to reposition</span>
                </button>
                <button
                  type="button"
                  class="canvas-view-item"
                  data-active={layoutMode() === "force"}
                  onClick={() => applyLayout("force")}
                >
                  <span class="canvas-view-item-label">Force-directed</span>
                  <span class="canvas-view-item-hint">Clusters connected nodes</span>
                </button>
                <button
                  type="button"
                  class="canvas-view-item"
                  data-active={layoutMode() === "radial"}
                  onClick={() => applyLayout("radial")}
                >
                  <span class="canvas-view-item-label">Radial</span>
                  <span class="canvas-view-item-hint">Circle grouped by kind</span>
                </button>
                <button
                  type="button"
                  class="canvas-view-item"
                  data-active={layoutMode() === "grouped"}
                  onClick={() => applyLayout("grouped")}
                >
                  <span class="canvas-view-item-label">Grouped columns</span>
                  <span class="canvas-view-item-hint">One column per kind</span>
                </button>
                <button
                  type="button"
                  class="canvas-view-item"
                  data-active={layoutMode() === "grid"}
                  onClick={() => applyLayout("grid")}
                >
                  <span class="canvas-view-item-label">Grid</span>
                  <span class="canvas-view-item-hint">Uniform tiling</span>
                </button>
              </div>
            </Show>
          </div>
          <button class="canvas-toolbar-btn" onClick={() => void handleCreateTask()}>
            + Task
          </button>
          <button class="canvas-toolbar-btn" onClick={() => void handleResetLayout()}>
            Reset
          </button>
        </div>
      </div>

      {/* Surface */}
      <div
        ref={surfaceRef}
        class="canvas-surface"
        style={{
          transform: `translate(${panX()}px, ${panY()}px) scale(${zoom()})`,
          "transform-origin": "0 0",
        }}
      >
        {/* Edges (SVG) */}
        <svg class="canvas-edges" style={{ width: "10000px", height: "10000px" }}>
          <For each={visibleEdges()}>
            {(edge) => {
              const from = () => visibleNodes().find((n) => nodeKey(n) === edge.from);
              const to = () => visibleNodes().find((n) => nodeKey(n) === edge.to);
              return (
                <Show when={from() && to()}>
                  {(() => {
                    const f = from()!;
                    const t = to()!;
                    const fs = NODE_STYLES[f.kind] ?? NODE_STYLES.task;
                    const ts = NODE_STYLES[t.kind] ?? NODE_STYLES.task;
                    const x1 = f.x + fs.width / 2;
                    const y1 = f.y + fs.height / 2;
                    const x2 = t.x + ts.width / 2;
                    const y2 = t.y + ts.height / 2;
                    return (
                      <line
                        x1={x1}
                        y1={y1}
                        x2={x2}
                        y2={y2}
                        stroke="oklch(var(--border) / 0.6)"
                        stroke-width={1}
                      />
                    );
                  })()}
                </Show>
              );
            }}
          </For>
        </svg>

        {/* Nodes */}
        <For each={visibleNodes()}>
          {(node) => {
            const style = NODE_STYLES[node.kind] ?? NODE_STYLES.task;
            const isExpanded = () => expandedId() === nodeKey(node);
            const isDragging = () => draggingId() === nodeKey(node);

            return (
              <div
                data-node-id={nodeKey(node)}
                class={cn(
                  "canvas-node",
                  `canvas-node-${node.kind}`,
                  isExpanded() && "canvas-node-expanded",
                  isDragging() && "canvas-node-dragging",
                )}
                style={{
                  left: `${node.x}px`,
                  top: `${node.y}px`,
                  width: `${isExpanded() ? Math.max(360, style.width) : style.width}px`,
                  "min-height": `${style.height}px`,
                  "border-color": style.color,
                }}
                onPointerDown={(e) => handleNodePointerDown(node, e)}
                onDblClick={(e) => {
                  e.stopPropagation();
                  openFocus(node);
                }}
              >
                <div class="canvas-node-header">
                  <span class="canvas-node-kind" style={{ color: style.color }}>
                    {node.kind}
                  </span>
                  <Show when={node.status}>
                    <span class="canvas-node-status">{node.status}</span>
                  </Show>
                </div>
                <div class="canvas-node-title">{node.label}</div>

                {/* Expanded content */}
                <Show when={isExpanded()}>
                  <div
                    class="canvas-node-body"
                    onPointerDown={(e) => e.stopPropagation()}
                  >
                    <Show when={node.kind === "task"}>
                      <TaskExpanded
                        node={node}
                        projectId={props.projectId}
                        onChange={() => void loadCanvas()}
                        onOpenThread={props.onOpenThread}
                      />
                    </Show>
                    <Show when={node.kind === "thread"}>
                      <ThreadExpanded
                        node={node}
                        onOpenThread={props.onOpenThread}
                      />
                    </Show>
                    <Show when={node.kind !== "task" && node.kind !== "thread"}>
                      <div
                        class="markdown-body canvas-node-markdown"
                        innerHTML={renderMarkdown(node.content ?? "")}
                      />
                    </Show>
                  </div>
                </Show>
              </div>
            );
          }}
        </For>
      </div>

      <Show when={loading()}>
        <div class="canvas-loading">Loading canvas...</div>
      </Show>
      <Show when={!loading() && nodes().length === 0}>
        <div class="canvas-empty">
          <div class="canvas-empty-title">Empty canvas</div>
          <div class="canvas-empty-body">
            Create a task or start a thread to see nodes on the canvas.
          </div>
          <button class="canvas-toolbar-btn" onClick={() => void handleCreateTask()}>
            + Create a task
          </button>
        </div>
      </Show>

      {/* Zoom indicator */}
      <div class="canvas-zoom-indicator">{Math.round(zoom() * 100)}%</div>
    </div>
  );
};

// ─── Expanded views ──────────────────────────────

const TaskExpanded: Component<{
  node: CanvasNode;
  projectId: number;
  onChange: () => void;
  onOpenThread?: (id: string) => void;
}> = (props) => {
  const [editContent, setEditContent] = createSignal(props.node.content ?? "");
  const [editing, setEditing] = createSignal(false);

  const handleSave = async () => {
    await updateTask(props.projectId, props.node.id, { content: editContent() });
    setEditing(false);
    props.onChange();
  };

  const handleCycleStatus = async () => {
    const order = ["todo", "active", "review", "done"];
    const curr = props.node.status ?? "todo";
    const next = order[(order.indexOf(curr) + 1) % order.length];
    await updateTask(props.projectId, props.node.id, { status: next });
    props.onChange();
  };

  const handleDelete = async () => {
    if (!confirm(`Delete task "${props.node.label}"?`)) return;
    await deleteTask(props.projectId, props.node.id);
    props.onChange();
  };

  const handleDispatch = async () => {
    const result = await dispatchTask(props.projectId, props.node.id, "new_thread");
    if (result.thread_id) {
      props.onOpenThread?.(result.thread_id);
    }
    props.onChange();
  };

  let reviewData: { summary?: string; changes?: string[]; suggested_next?: string[] } = {};
  try {
    const rj = (props.node as CanvasNode & { review_json?: string }).review_json;
    if (rj) reviewData = JSON.parse(rj);
  } catch {
    /* ignore */
  }

  return (
    <div class="task-expanded">
      <Show
        when={editing()}
        fallback={
          <div
            class="markdown-body canvas-node-markdown"
            onClick={() => {
              setEditContent(props.node.content ?? "");
              setEditing(true);
            }}
            innerHTML={
              props.node.content
                ? renderMarkdown(props.node.content)
                : '<p class="text-muted-foreground/40 italic">Click to add content</p>'
            }
          />
        }
      >
        <textarea
          class="canvas-node-textarea"
          value={editContent()}
          onInput={(e) => setEditContent(e.currentTarget.value)}
          onBlur={() => void handleSave()}
          autofocus
        />
      </Show>

      <Show when={props.node.status === "review" && reviewData.summary}>
        <div class="task-review-box">
          <div class="task-review-label">Completion Review</div>
          <div class="task-review-summary">{reviewData.summary}</div>
          <div class="task-review-actions">
            <button
              class="canvas-action-btn canvas-action-approve"
              onClick={async () => {
                await reviewAction(props.projectId, props.node.id, "approve");
                props.onChange();
              }}
            >
              Approve
            </button>
            <button
              class="canvas-action-btn"
              onClick={async () => {
                await reviewAction(props.projectId, props.node.id, "replan");
                props.onChange();
              }}
            >
              Re-plan
            </button>
          </div>
        </div>
      </Show>

      <div class="canvas-node-actions">
        <button class="canvas-action-btn" onClick={() => void handleCycleStatus()}>
          Status: {props.node.status ?? "todo"} →
        </button>
        <Show when={props.node.status === "todo" && props.node.content}>
          <button class="canvas-action-btn canvas-action-primary" onClick={() => void handleDispatch()}>
            Dispatch
          </button>
        </Show>
        <button class="canvas-action-btn canvas-action-danger" onClick={() => void handleDelete()}>
          Delete
        </button>
      </div>
    </div>
  );
};

const ThreadExpanded: Component<{
  node: CanvasNode;
  onOpenThread?: (id: string) => void;
}> = (props) => {
  return (
    <div class="thread-expanded">
      <div class="thread-summary">
        <Show when={props.node.content}>
          <div class="text-muted-foreground/70 text-[13px] leading-relaxed">
            {props.node.content}
          </div>
        </Show>
      </div>
      <div class="canvas-node-actions">
        <button
          class="canvas-action-btn canvas-action-primary"
          onClick={() => props.onOpenThread?.(props.node.id)}
        >
          Open chat →
        </button>
      </div>
    </div>
  );
};

export default CanvasView;
