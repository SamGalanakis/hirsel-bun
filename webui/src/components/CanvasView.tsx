import {
  type Component,
  type JSX,
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
import {
  getCanvas,
  patchCanvasLayout,
  resetCanvasLayout,
  deleteCanvasNode,
  createCanvasNode,
  updateCanvasNode,
} from "@/lib/api/canvas";
import { recordNodeFocus } from "@/lib/api/projects";
import { dispatchTask, reviewAction } from "@/lib/api/tasks";
import { cn } from "@/lib/cn";
import { renderNodeMarkdown } from "@/lib/markdown";
import TagEditor from "@/components/TagEditor";

interface CanvasViewProps {
  projectId: number;
  refreshNonce?: number;
  onOpenThread?: (threadId: string) => void;
  /** External focus request — when this changes to a non-null key, focus that node */
  focusRequest?: string | null;
  /** External create request — when this changes to a kind, open the new-node dialog */
  createRequest?: string | null;
  /** External center-on request — pans the camera to the given "kind:id" without opening focus */
  centerRequest?: string | null;
  /** External search query — when non-empty, overrides internal filter for live preview while palette is open */
  externalSearch?: string;
  /** Rendered on the left side of the focus overlay (chat surface etc). */
  focusChatSlot?: () => JSX.Element;
  onRequestHandled?: () => void;
  onRequestPalette?: () => void;
}

interface PositionedNode extends CanvasNode {
  x: number;
  y: number;
}

// Visual config per kind. Width/height are the *fallback/expanded* dimensions;
// the pill size is computed per-node from label length via `nodeDims()`.
const NODE_STYLES: Record<string, { color: string; width: number; height: number }> = {
  task: { color: "oklch(var(--signal-amber))", width: 220, height: 28 },
  thread: { color: "oklch(var(--brand))", width: 220, height: 28 },
  component: { color: "oklch(var(--signal-green))", width: 200, height: 28 },
  entity: { color: "oklch(var(--signal-blue))", width: 200, height: 28 },
  convention: { color: "oklch(0.72 0.1 300)", width: 200, height: 28 },
  decision: { color: "oklch(var(--signal-amber))", width: 200, height: 28 },
  fact: { color: "oklch(var(--muted-foreground))", width: 200, height: 28 },
  goal: { color: "oklch(var(--signal-green))", width: 200, height: 28 },
  document: { color: "oklch(var(--signal-blue))", width: 200, height: 28 },
};

const PILL_HEIGHT = 28;
const PILL_PAD = 24; // left dot area + right padding
const PILL_CHAR_WIDTH = 6.5;
const PILL_STATUS_WIDTH = 48; // reserve room for a short status chip when present
const PILL_MIN_WIDTH = 120;
const PILL_MAX_WIDTH = 300;

// Compute display dimensions for a node in its collapsed (pill) state.
// Width scales with label length; status kinds reserve extra room.
function nodeDims(node: CanvasNode): { width: number; height: number } {
  const label = node.label ?? "";
  const hasStatus = !!node.status && node.status !== "active" && node.status !== "";
  const contentWidth =
    label.length * PILL_CHAR_WIDTH + PILL_PAD + (hasStatus ? PILL_STATUS_WIDTH : 0);
  const width = Math.max(PILL_MIN_WIDTH, Math.min(PILL_MAX_WIDTH, contentWidth));
  return { width, height: PILL_HEIGHT };
}

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
  const [focusedKey, setFocusedKey] = createSignal<string | null>(null);
  const [trayOpen, setTrayOpen] = createSignal(false);

  const welcomeKey = () => `hirsel_canvas_welcomed_${props.projectId}`;
  const [showWelcomeBanner, setShowWelcomeBanner] = createSignal(
    !localStorage.getItem(`hirsel_canvas_welcomed_${props.projectId}`),
  );
  const dismissWelcome = () => {
    try {
      localStorage.setItem(welcomeKey(), "1");
    } catch {
      /* ignore */
    }
    setShowWelcomeBanner(false);
  };

  const seenKey = () => `hirsel_seen_notifications_${props.projectId}`;
  const [seenSet, setSeenSet] = createSignal<Set<string>>(
    new Set(JSON.parse(localStorage.getItem(`hirsel_seen_notifications_${props.projectId}`) ?? "[]")),
  );
  const persistSeen = (next: Set<string>) => {
    setSeenSet(next);
    try {
      localStorage.setItem(seenKey(), JSON.stringify([...next]));
    } catch {
      /* ignore */
    }
  };

  // Filters
  const [hiddenKinds, setHiddenKinds] = createSignal<Set<string>>(new Set());
  const [hiddenStatuses, setHiddenStatuses] = createSignal<Set<string>>(new Set(["done"]));
  const [searchText, setSearchText] = createSignal("");
  const [layoutMode, setLayoutMode] = createSignal<
    "grid" | "grouped" | "force"
  >("force");
  const [forceMotionState, setForceMotionState] = createSignal<
    "paused" | "settling" | "live"
  >("paused");

  // ── Force layout tunables (persisted) ────────────────────────────────────
  const PARAMS_KEY = `hirsel_force_params_${props.projectId}`;
  const DEFAULT_PARAMS = {
    clustering: 0.05, // kind-centroid pull strength
    spacing: 160, // ideal edge length
    repulsion: 900, // base repulsion coefficient
    crossKindBoost: 2.5, // extra repulsion between different kinds
    speed: 1, // simulation tempo scalar
  };
  const loadedParams = (() => {
    try {
      return { ...DEFAULT_PARAMS, ...JSON.parse(localStorage.getItem(PARAMS_KEY) ?? "{}") };
    } catch {
      return DEFAULT_PARAMS;
    }
  })();
  const [forceParams, setForceParams] = createSignal(loadedParams);
  const updateParam = <K extends keyof typeof DEFAULT_PARAMS>(
    key: K,
    value: number,
  ) => {
    const next = { ...forceParams(), [key]: value };
    setForceParams(next);
    try {
      localStorage.setItem(PARAMS_KEY, JSON.stringify(next));
    } catch {
      /* ignore */
    }
  };
  const resetForceParams = () => {
    setForceParams(DEFAULT_PARAMS);
    try {
      localStorage.removeItem(PARAMS_KEY);
    } catch {
      /* ignore */
    }
  };
  const [viewMenuOpen, setViewMenuOpen] = createSignal(false);
  const [filterMenuOpen, setFilterMenuOpen] = createSignal(false);

  let containerRef: HTMLDivElement | undefined;
  let surfaceRef: HTMLDivElement | undefined;

  const nodeKey = (n: CanvasNode) => `${n.kind}:${n.id}`;

  const loadCanvas = async () => {
    try {
      const view = await getCanvas(props.projectId);
      const hasStoredLayout = Object.keys(view.layout).length > 0;

      let positioned: PositionedNode[];
      if (hasStoredLayout) {
        positioned = view.nodes.map((n, i) => {
          const key = nodeKey(n);
          const stored = view.layout[key];
          if (stored) return { ...n, x: stored.x, y: stored.y };
          const col = i % 5;
          const row = Math.floor(i / 5);
          return { ...n, x: 200 + col * 240, y: 200 + row * 48 };
        });
      } else {
        // No stored layout — group by kind so clusters are visible on first load
        const byKind: Record<string, CanvasNode[]> = {};
        for (const n of view.nodes) {
          (byKind[n.kind] ??= []).push(n);
        }
        const kinds = Object.keys(byKind).sort();
        positioned = [];
        kinds.forEach((kind, col) => {
          byKind[kind].forEach((n, row) => {
            positioned.push({ ...n, x: 80 + col * 240, y: 80 + row * 44 });
          });
        });
      }

      setNodes(positioned);
      setEdges(view.edges);
      setLoading(false);
      if (!hasStoredLayout && positioned.length > 0) {
        // Kick off animated FA2 settle, then fit camera once settled
        setLayoutMode("force");
        queueMicrotask(() => {
          startSettleAnimation();
          // fitToContent after the settle has roughly finished
          setTimeout(() => fitToContent(), 2200);
        });
      }
    } catch (e) {
      console.error("Failed to load canvas", e);
      setLoading(false);
    }
  };

  const fitToContent = () => {
    const rect = containerRef?.getBoundingClientRect();
    const current = nodes();
    if (!rect || current.length === 0) return;
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const n of current) {
      const d = nodeDims(n);
      minX = Math.min(minX, n.x);
      minY = Math.min(minY, n.y);
      maxX = Math.max(maxX, n.x + d.width);
      maxY = Math.max(maxY, n.y + d.height);
    }
    const pad = 80;
    const w = maxX - minX + pad * 2;
    const h = maxY - minY + pad * 2;
    const sx = rect.width / w;
    const sy = rect.height / h;
    const scale = Math.max(0.4, Math.min(1.2, Math.min(sx, sy)));
    const cx = (minX + maxX) / 2;
    const cy = (minY + maxY) / 2;
    setZoom(scale);
    setPanX(rect.width / 2 - cx * scale);
    setPanY(rect.height / 2 - cy * scale);
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
    const external = (props.externalSearch ?? "").toLowerCase().trim();
    const search = external || searchText().toLowerCase().trim();
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

  // Save layout (debounced) + status pill
  const [saveStatus, setSaveStatus] = createSignal<"idle" | "saving" | "saved" | "failed">(
    "idle",
  );
  let saveTimeout: number | undefined;
  let savedPillTimeout: number | undefined;
  const scheduleSave = () => {
    if (saveTimeout) clearTimeout(saveTimeout);
    saveTimeout = window.setTimeout(async () => {
      const positions: Record<string, CanvasPosition> = {};
      for (const n of nodes()) {
        positions[nodeKey(n)] = { x: n.x, y: n.y };
      }
      setSaveStatus("saving");
      try {
        await patchCanvasLayout(props.projectId, positions);
        setSaveStatus("saved");
        if (savedPillTimeout) clearTimeout(savedPillTimeout);
        savedPillTimeout = window.setTimeout(() => setSaveStatus("idle"), 1000);
      } catch (e) {
        console.error("Failed to save layout", e);
        setSaveStatus("failed");
        if (savedPillTimeout) clearTimeout(savedPillTimeout);
        savedPillTimeout = window.setTimeout(() => setSaveStatus("idle"), 3000);
      }
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
      target.closest(".canvas-hotbar") ||
      target.closest(".canvas-tray") ||
      target.closest(".canvas-view-dropdown") ||
      target.closest(".canvas-filter-dropdown") ||
      target.closest(".canvas-tray-dropdown") ||
      target.closest(".canvas-zoom-indicator") ||
      target.closest(".canvas-focus-overlay") ||
      target.closest(".canvas-dialog-overlay") ||
      target.closest(".canvas-welcome-banner") ||
      target.closest(".canvas-save-pill") ||
      target.closest(".command-palette-overlay") ||
      target.closest(".command-palette")
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

  const nodeByKey = (key: string) => nodes().find((n) => nodeKey(n) === key);

  const openFocus = (node: CanvasNode) => {
    recordNodeFocus(props.projectId, node.kind, node.id).catch((error) => {
      console.warn("record_focus failed", error);
    });
    if (node.kind === "thread") {
      props.onOpenThread?.(node.id);
      return;
    }
    setExpandedId(null);
    setFocusedKey(nodeKey(node));
  };

  const openFocusByKey = (key: string) => {
    const n = nodeByKey(key);
    if (n) openFocus(n);
  };

  const handleMarkdownClick = (e: MouseEvent) => {
    const target = (e.target as HTMLElement | null)?.closest("[data-node-key]");
    if (!target) return;
    e.preventDefault();
    e.stopPropagation();
    const key = target.getAttribute("data-node-key");
    if (key) openFocusByKey(key);
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

  // Notifications: derive attention-worthy items from loaded canvas data
  interface Notification {
    key: string;
    nodeKey: string;
    kind: "review" | "finished" | "blocked" | "highlight";
    label: string;
    preview: string;
  }

  const notifications = createMemo<Notification[]>(() => {
    const out: Notification[] = [];
    for (const n of nodes()) {
      const baseKey = `${n.kind}:${n.id}`;
      if (n.kind === "task" && n.status === "review") {
        out.push({
          key: `${baseKey}#review`,
          nodeKey: baseKey,
          kind: "review",
          label: "Review completion",
          preview: n.label,
        });
      } else if (n.kind === "thread") {
        if (n.status === "completed") {
          out.push({
            key: `${baseKey}#completed`,
            nodeKey: baseKey,
            kind: "finished",
            label: "Thread finished",
            preview: n.label,
          });
        } else if (n.status === "blocked") {
          out.push({
            key: `${baseKey}#blocked`,
            nodeKey: baseKey,
            kind: "blocked",
            label: "Thread blocked",
            preview: n.label,
          });
        }
        // highlight field (on the original ShepherdThread — exposed via canvas focused_task_id is unrelated)
        // Threads with a non-null highlight surface as an attention item
        const highlight = n.highlight;
        if (highlight) {
          out.push({
            key: `${baseKey}#hl:${highlight.slice(0, 48)}`,
            nodeKey: baseKey,
            kind: "highlight",
            label: n.label,
            preview: highlight,
          });
        }
      }
    }
    return out;
  });

  const unseenCount = createMemo(() => {
    const seen = seenSet();
    return notifications().filter((n) => !seen.has(n.key)).length;
  });

  const markAllSeen = () => {
    const all = new Set(seenSet());
    for (const n of notifications()) all.add(n.key);
    persistSeen(all);
  };

  const handleNotificationClick = (n: Notification) => {
    const node = nodeByKey(n.nodeKey);
    if (node) openFocus(node);
    const next = new Set(seenSet());
    next.add(n.key);
    persistSeen(next);
    setTrayOpen(false);
  };

  const handleResetLayout = async () => {
    await resetCanvasLayout(props.projectId);
    await loadCanvas();
  };

  const applyLayout = (mode: "grid" | "grouped" | "force") => {
    setLayoutMode(mode);
    setViewMenuOpen(false);
    // Any mode switch cancels an in-flight settle animation
    if (mode !== "force") stopSettleAnimation();

    const current = nodes();
    if (current.length === 0) return;

    if (mode === "grid") {
      const cols = Math.max(1, Math.ceil(Math.sqrt(current.length)));
      const repositioned = current.map((n, i) => ({
        ...n,
        x: 80 + (i % cols) * 240,
        y: 80 + Math.floor(i / cols) * 48,
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
          repositioned.push({ ...n, x: 80 + col * 240, y: 80 + row * 44 });
        });
      });
      setNodes(repositioned);
      scheduleSave();
    } else if (mode === "force") {
      startSettleAnimation();
    }
  };

  // FA2-flavored settle animation: run physics with rAF until energy drops
  // below threshold, then freeze and save. Dragged nodes are pinned.
  //
  // Forces:
  //   - Repulsion  ∝ (deg_a+1)(deg_b+1)/dist²  (ForceAtlas2-style degree scaling)
  //   - Attraction along edges (spring)
  //   - Kind-centroid attraction (mild pull toward same-kind centroid → clusters)
  //   - Gravity toward world origin
  let liveRaf: number | undefined;
  let liveVel = new Map<string, { vx: number; vy: number }>();
  const seedForcePositionsIfStacked = () => {
    const seed = nodes();
    const allSame =
      seed.length > 1 &&
      seed.every((n) => n.x === seed[0].x && n.y === seed[0].y);
    if (!allSame) return;

    const r = 300;
    const seeded = seed.map((n, i) => ({
      ...n,
      x: Math.cos((i / seed.length) * Math.PI * 2) * r + 400,
      y: Math.sin((i / seed.length) * Math.PI * 2) * r + 300,
    }));
    setNodes(seeded);
  };
  const stopSettleAnimation = (opts?: { persist?: boolean }) => {
    const wasRunning = liveRaf !== undefined;
    if (wasRunning) cancelAnimationFrame(liveRaf);
    liveRaf = undefined;
    liveVel.clear();
    setForceMotionState("paused");
    if (opts?.persist && wasRunning) scheduleSave();
  };
  const startSettleAnimation = () => {
    stopSettleAnimation();
    setLayoutMode("force");
    setForceMotionState("settling");
    liveVel = new Map();
    seedForcePositionsIfStacked();
    runPhysicsLoop({ stopOnSettle: true });
  };
  const startLiveAnimation = () => {
    stopSettleAnimation();
    setLayoutMode("force");
    setForceMotionState("live");
    liveVel = new Map();
    seedForcePositionsIfStacked();
    runPhysicsLoop({ stopOnSettle: false });
  };
  const handleForceParamCommit = () => {
    if (layoutMode() !== "force") return;
    if (forceMotionState() === "live") {
      startLiveAnimation();
      return;
    }
    startSettleAnimation();
  };

  // Shared physics loop — used by settle animation + (eventually) any other
  // continuous mode. FA2-flavored: degree-scaled repulsion + spring + kind
  // clustering + center gravity. When `stopOnSettle` is true, the loop ends
  // once kinetic energy stays below SETTLE_THRESHOLD for two consecutive
  // frames and positions are persisted.
  const runPhysicsLoop = (opts: { stopOnSettle: boolean }) => {
    const SPRING = 0.04;
    const DAMPING = 0.82;
    const CENTER_GRAVITY = 0.006;
    const MIN_DIST = 12;
    const MAX_VEL = 28;
    const SETTLE_THRESHOLD = 0.4;
    const MAX_FRAMES_HARD = 900; // absolute cap ~15s at 60fps

    let frames = 0;
    let lastEnergy = Infinity;

    const tick = () => {
      const current = nodes();
      const edgeList = edges();
      const params = forceParams();
      const REPULSION = params.repulsion;
      const IDEAL_LEN = params.spacing;
      const KIND_CLUSTER_PULL = params.clustering;
      const CROSS_KIND_BOOST = params.crossKindBoost;
      const SIM_SPEED = params.speed;
      frames++;
      if (current.length === 0) {
        liveRaf = requestAnimationFrame(tick);
        return;
      }

      // Ensure velocity entries
      const keys = new Set<string>();
      for (const n of current) {
        const k = nodeKey(n);
        keys.add(k);
        if (!liveVel.has(k)) liveVel.set(k, { vx: 0, vy: 0 });
      }
      for (const k of Array.from(liveVel.keys())) {
        if (!keys.has(k)) liveVel.delete(k);
      }

      const pinnedKey = draggingId();

      // Per-node degree (number of incident edges)
      const degree = new Map<string, number>();
      for (const e of edgeList) {
        degree.set(e.from, (degree.get(e.from) ?? 0) + 1);
        degree.set(e.to, (degree.get(e.to) ?? 0) + 1);
      }
      const deg = (k: string) => (degree.get(k) ?? 0) + 1;

      // Kind centroids for cluster pull
      const kindSum = new Map<string, { x: number; y: number; count: number }>();
      for (const n of current) {
        const k = kindSum.get(n.kind) ?? { x: 0, y: 0, count: 0 };
        k.x += n.x;
        k.y += n.y;
        k.count += 1;
        kindSum.set(n.kind, k);
      }
      const kindCentroid = new Map<string, { x: number; y: number }>();
      kindSum.forEach((v, k) => {
        kindCentroid.set(k, { x: v.x / v.count, y: v.y / v.count });
      });

      const idx = new Map<string, number>();
      current.forEach((n, i) => idx.set(nodeKey(n), i));

      // Repulsion (FA2-style degree-scaled, boosted between different kinds)
      for (let i = 0; i < current.length; i++) {
        for (let j = i + 1; j < current.length; j++) {
          const a = current[i];
          const b = current[j];
          const ka = nodeKey(a);
          const kb = nodeKey(b);
          const dx = b.x - a.x;
          const dy = b.y - a.y;
          const dist2 = Math.max(dx * dx + dy * dy, MIN_DIST * MIN_DIST);
          const dist = Math.sqrt(dist2);
          const sameKind = a.kind === b.kind;
          const kindMul = sameKind ? 1 : CROSS_KIND_BOOST;
          const force = ((REPULSION * kindMul * deg(ka) * deg(kb)) / dist2) * SIM_SPEED;
          const fx = (dx / dist) * force;
          const fy = (dy / dist) * force;
          const va = liveVel.get(ka)!;
          const vb = liveVel.get(kb)!;
          va.vx -= fx;
          va.vy -= fy;
          vb.vx += fx;
          vb.vy += fy;
        }
      }

      // Spring attraction along edges
      for (const edge of edgeList) {
        const ai = idx.get(edge.from);
        const bi = idx.get(edge.to);
        if (ai === undefined || bi === undefined) continue;
        const a = current[ai];
        const b = current[bi];
        const dx = b.x - a.x;
        const dy = b.y - a.y;
        const dist = Math.max(Math.sqrt(dx * dx + dy * dy), MIN_DIST);
        const displacement = dist - IDEAL_LEN;
        const force = SPRING * displacement * SIM_SPEED;
        const fx = (dx / dist) * force;
        const fy = (dy / dist) * force;
        const va = liveVel.get(nodeKey(a))!;
        const vb = liveVel.get(nodeKey(b))!;
        va.vx += fx;
        va.vy += fy;
        vb.vx -= fx;
        vb.vy -= fy;
      }

      // Kind clustering — pull each node toward its kind centroid
      for (const n of current) {
        const c = kindCentroid.get(n.kind);
        if (!c) continue;
        const v = liveVel.get(nodeKey(n))!;
        v.vx += (c.x - n.x) * KIND_CLUSTER_PULL * SIM_SPEED;
        v.vy += (c.y - n.y) * KIND_CLUSTER_PULL * SIM_SPEED;
      }

      // Center gravity
      for (const n of current) {
        const v = liveVel.get(nodeKey(n))!;
        v.vx -= n.x * CENTER_GRAVITY * SIM_SPEED;
        v.vy -= n.y * CENTER_GRAVITY * SIM_SPEED;
      }

      // Integrate
      let energy = 0;
      const next = current.map((n) => {
        const k = nodeKey(n);
        if (k === pinnedKey) {
          const v = liveVel.get(k)!;
          v.vx = 0;
          v.vy = 0;
          return n;
        }
        const v = liveVel.get(k)!;
        v.vx *= DAMPING;
        v.vy *= DAMPING;
        const speed = Math.sqrt(v.vx * v.vx + v.vy * v.vy);
        const maxVel = MAX_VEL * Math.max(0.65, SIM_SPEED);
        if (speed > maxVel) {
          v.vx = (v.vx / speed) * maxVel;
          v.vy = (v.vy / speed) * maxVel;
        }
        energy += v.vx * v.vx + v.vy * v.vy;
        return { ...n, x: n.x + v.vx, y: n.y + v.vy };
      });
      setNodes(next);

      const settled =
        energy < SETTLE_THRESHOLD && lastEnergy < SETTLE_THRESHOLD;
      lastEnergy = energy;

      if (opts.stopOnSettle && (settled || frames >= MAX_FRAMES_HARD)) {
        liveRaf = undefined;
        setForceMotionState("paused");
        scheduleSave();
        return;
      }

      liveRaf = requestAnimationFrame(tick);
    };
    liveRaf = requestAnimationFrame(tick);
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

  const CREATABLE_KINDS = ["task", "document", "goal", "decision"] as const;
  type CreatableKind = (typeof CREATABLE_KINDS)[number];

  const [newKind, setNewKind] = createSignal<CreatableKind>("task");
  const [newTaskOpen, setNewTaskOpen] = createSignal(false);
  const [newTaskTitle, setNewTaskTitle] = createSignal("");
  const [newTaskBody, setNewTaskBody] = createSignal("");
  const [newTaskTags, setNewTaskTags] = createSignal<string[]>([]);
  const [newTaskSaving, setNewTaskSaving] = createSignal(false);
  const [newDocumentSubtype, setNewDocumentSubtype] = createSignal<"markdown" | "html">("markdown");
  const [newMenuOpen, setNewMenuOpen] = createSignal(false);
  let newTaskInputRef: HTMLInputElement | undefined;

  const openNewNode = (kind: CreatableKind) => {
    setNewKind(kind);
    setNewTaskTitle("");
    setNewTaskBody("");
    setNewTaskTags([]);
    setNewDocumentSubtype("markdown");
    setNewMenuOpen(false);
    setNewTaskOpen(true);
    queueMicrotask(() => newTaskInputRef?.focus());
  };

  const submitNewTask = async () => {
    const title = newTaskTitle().trim();
    if (!title || newTaskSaving()) return;
    setNewTaskSaving(true);
    try {
      const body = newTaskBody().trim();
      const tags = newTaskTags();
      await createCanvasNode(props.projectId, {
        kind: newKind(),
        title,
        ...(body ? { content: body } : {}),
        ...(newKind() === "document" ? { subtype: newDocumentSubtype() } : {}),
        ...(tags.length > 0 ? { tags } : {}),
      });
      setNewTaskOpen(false);
      await loadCanvas();
    } finally {
      setNewTaskSaving(false);
    }
  };

  // Keyboard: Esc collapses popup / closes menu
  onMount(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (newTaskOpen()) {
          setNewTaskOpen(false);
        } else if (focusedKey()) {
          setFocusedKey(null);
        } else if (newMenuOpen()) {
          setNewMenuOpen(false);
        } else if (viewMenuOpen()) {
          setViewMenuOpen(false);
        } else if (expandedId()) {
          setExpandedId(null);
        }
      }
    };
    document.addEventListener("keydown", handler);
    onCleanup(() => document.removeEventListener("keydown", handler));
    onCleanup(stopSettleAnimation);
  });

  // Close menus on outside click
  onMount(() => {
    const handler = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      if (
        viewMenuOpen() &&
        (!target.closest(".canvas-view-menu") || target.closest(".canvas-new-menu"))
      ) {
        setViewMenuOpen(false);
      }
      if (filterMenuOpen() && !target.closest(".canvas-filter-menu")) {
        setFilterMenuOpen(false);
      }
      if (
        newMenuOpen() &&
        (!target.closest(".canvas-new-menu") || target.closest(".canvas-view-dropdown"))
      ) {
        // the dropdown-item clicks set menuOpen=false themselves
        if (!target.closest(".canvas-view-dropdown")) setNewMenuOpen(false);
      }
      if (trayOpen() && !target.closest(".canvas-tray")) {
        setTrayOpen(false);
      }
    };
    document.addEventListener("click", handler);
    onCleanup(() => document.removeEventListener("click", handler));
  });

  // React to external focus/create requests from the workspace (e.g. command palette)
  createEffect(() => {
    const key = props.focusRequest;
    if (!key) return;
    const n = nodeByKey(key);
    if (n) openFocus(n);
    props.onRequestHandled?.();
  });
  createEffect(() => {
    const kind = props.createRequest;
    if (!kind) return;
    if ((["task", "document", "goal", "decision"] as string[]).includes(kind)) {
      openNewNode(kind as CreatableKind);
    }
    props.onRequestHandled?.();
  });
  createEffect(() => {
    const key = props.centerRequest;
    if (!key) return;
    const n = nodeByKey(key);
    if (n) centerOnNode(n);
    props.onRequestHandled?.();
  });

  const centerOnNode = (n: CanvasNode & { x: number; y: number }) => {
    const rect = containerRef?.getBoundingClientRect();
    if (!rect) return;
    const d = nodeDims(n);
    const cx = n.x + d.width / 2;
    const cy = n.y + d.height / 2;
    const scale = Math.max(0.6, zoom());
    setZoom(scale);
    setPanX(rect.width / 2 - cx * scale);
    setPanY(rect.height / 2 - cy * scale);
  };

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
      {/* Notification tray — top-right floating */}
      <div class="canvas-tray">
        <button
          type="button"
          class={cn("canvas-tray-btn", unseenCount() > 0 && "has-unseen")}
          onClick={() => {
            const opening = !trayOpen();
            setTrayOpen(opening);
            if (opening) markAllSeen();
          }}
          aria-haspopup="menu"
          aria-expanded={trayOpen()}
          title="Notifications"
        >
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
            <path d="M8 2C6 2 4.5 3.5 4.5 5.5V9L3 11h10L11.5 9V5.5C11.5 3.5 10 2 8 2z" />
            <path d="M6.5 13a1.5 1.5 0 0 0 3 0" />
          </svg>
          <Show when={unseenCount() > 0}>
            <span class="canvas-tray-badge">{unseenCount()}</span>
          </Show>
        </button>
        <Show when={trayOpen()}>
          <div class="canvas-tray-dropdown" onPointerDown={(e) => e.stopPropagation()}>
            <Show
              when={notifications().length > 0}
              fallback={
                <div class="canvas-tray-empty">All caught up</div>
              }
            >
              <For each={notifications()}>
                {(notif) => (
                  <button
                    type="button"
                    class="canvas-tray-item"
                    onClick={() => handleNotificationClick(notif)}
                  >
                    <span
                      class={cn("canvas-tray-item-dot", `canvas-tray-item-${notif.kind}`)}
                    />
                    <span class="canvas-tray-item-label">{notif.label}</span>
                    <span class="canvas-tray-item-preview">{notif.preview}</span>
                  </button>
                )}
              </For>
            </Show>
          </div>
        </Show>
      </div>

      {/* Hotbar — bottom-center floating */}
      <div class="canvas-hotbar">
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

        <div class="canvas-hotbar-actions">
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
                  data-active={layoutMode() === "force"}
                  onClick={() => applyLayout("force")}
                >
                  <span class="canvas-view-item-label">Force</span>
                  <span class="canvas-view-item-hint">
                    Clustered by kind · settle once or run live
                  </span>
                </button>
                <button
                  type="button"
                  class="canvas-view-item"
                  data-active={layoutMode() === "grouped"}
                  onClick={() => applyLayout("grouped")}
                >
                  <span class="canvas-view-item-label">Grouped</span>
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

                <div class="canvas-view-divider" />
                <div class="canvas-params-panel">
                  <div class="canvas-params-header">
                    <span>Force motion</span>
                  </div>
                  <div class="canvas-force-actions">
                    <button
                      type="button"
                      class="canvas-force-toggle"
                      data-active={forceMotionState() === "settling"}
                      aria-pressed={forceMotionState() === "settling"}
                      onClick={() =>
                        forceMotionState() === "settling"
                          ? stopSettleAnimation({ persist: true })
                          : startSettleAnimation()
                      }
                    >
                      Settle
                    </button>
                    <button
                      type="button"
                      class="canvas-force-toggle"
                      data-active={forceMotionState() === "live"}
                      aria-pressed={forceMotionState() === "live"}
                      onClick={() =>
                        forceMotionState() === "live"
                          ? stopSettleAnimation({ persist: true })
                          : startLiveAnimation()
                      }
                    >
                      Live
                    </button>
                  </div>
                  <div class="canvas-params-header">
                    <span>Force parameters</span>
                    <button
                      type="button"
                      class="canvas-params-reset"
                      onClick={() => {
                        resetForceParams();
                        handleForceParamCommit();
                      }}
                    >
                      reset
                    </button>
                  </div>
                  <label class="canvas-param">
                    <div class="canvas-param-row">
                      <span class="canvas-param-label">Speed</span>
                      <span class="canvas-param-value">
                        {forceParams().speed.toFixed(1)}×
                      </span>
                    </div>
                    <input
                      type="range"
                      min="0.4"
                      max="5"
                      step="0.1"
                      value={forceParams().speed}
                      onInput={(e) =>
                        updateParam("speed", parseFloat(e.currentTarget.value))
                      }
                      onChange={() => handleForceParamCommit()}
                    />
                  </label>
                  <label class="canvas-param">
                    <div class="canvas-param-row">
                      <span class="canvas-param-label">Clustering</span>
                      <span class="canvas-param-value">
                        {forceParams().clustering.toFixed(3)}
                      </span>
                    </div>
                    <input
                      type="range"
                      min="0"
                      max="0.35"
                      step="0.005"
                      value={forceParams().clustering}
                      onInput={(e) =>
                        updateParam("clustering", parseFloat(e.currentTarget.value))
                      }
                      onChange={() => handleForceParamCommit()}
                    />
                  </label>
                  <label class="canvas-param">
                    <div class="canvas-param-row">
                      <span class="canvas-param-label">Spacing</span>
                      <span class="canvas-param-value">
                        {Math.round(forceParams().spacing)}
                      </span>
                    </div>
                    <input
                      type="range"
                      min="40"
                      max="1400"
                      step="20"
                      value={forceParams().spacing}
                      onInput={(e) =>
                        updateParam("spacing", parseFloat(e.currentTarget.value))
                      }
                      onChange={() => handleForceParamCommit()}
                    />
                  </label>
                  <label class="canvas-param">
                    <div class="canvas-param-row">
                      <span class="canvas-param-label">Repulsion</span>
                      <span class="canvas-param-value">
                        {Math.round(forceParams().repulsion)}
                      </span>
                    </div>
                    <input
                      type="range"
                      min="200"
                      max="20000"
                      step="250"
                      value={forceParams().repulsion}
                      onInput={(e) =>
                        updateParam("repulsion", parseFloat(e.currentTarget.value))
                      }
                      onChange={() => handleForceParamCommit()}
                    />
                  </label>
                  <label class="canvas-param">
                    <div class="canvas-param-row">
                      <span class="canvas-param-label">Cross-kind push</span>
                      <span class="canvas-param-value">
                        {forceParams().crossKindBoost.toFixed(1)}×
                      </span>
                    </div>
                    <input
                      type="range"
                      min="0.25"
                      max="24"
                      step="0.25"
                      value={forceParams().crossKindBoost}
                      onInput={(e) =>
                        updateParam(
                          "crossKindBoost",
                          parseFloat(e.currentTarget.value),
                        )
                      }
                      onChange={() => handleForceParamCommit()}
                    />
                  </label>
                </div>
              </div>
            </Show>
          </div>
          <div class="canvas-view-menu canvas-new-menu">
            <button
              class="canvas-toolbar-btn canvas-view-trigger"
              onClick={() => setNewMenuOpen(!newMenuOpen())}
              aria-haspopup="menu"
              aria-expanded={newMenuOpen()}
            >
              <span>+ New</span>
              <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.3">
                <path d="M2 4L5 7L8 4" />
              </svg>
            </button>
            <Show when={newMenuOpen()}>
              <div class="canvas-view-dropdown" onPointerDown={(e) => e.stopPropagation()}>
                <For each={CREATABLE_KINDS}>
                  {(kind) => (
                    <button
                      type="button"
                      class="canvas-view-item"
                      onClick={() => openNewNode(kind)}
                    >
                      <span
                        class="canvas-view-item-dot"
                        style={{
                          background: (NODE_STYLES[kind] ?? NODE_STYLES.task).color,
                        }}
                      />
                      <span class="canvas-view-item-label">
                        {kind.charAt(0).toUpperCase() + kind.slice(1)}
                      </span>
                      <span class="canvas-view-item-hint">
                        {kind === "task"
                          ? "Actionable, dispatchable"
                          : kind === "document"
                            ? "Free-form notes"
                            : kind === "goal"
                              ? "Objective or outcome"
                              : "Captured choice"}
                      </span>
                    </button>
                  )}
                </For>
              </div>
            </Show>
          </div>
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
          <defs>
            <marker
              id="canvas-edge-arrow"
              viewBox="0 0 10 10"
              refX="9"
              refY="5"
              markerWidth="7"
              markerHeight="7"
              orient="auto-start-reverse"
              markerUnits="userSpaceOnUse"
            >
              <path d="M0,0 L10,5 L0,10 Z" fill="oklch(var(--border))" />
            </marker>
          </defs>
          <For each={visibleEdges()}>
            {(edge) => {
              const from = createMemo(() =>
                visibleNodes().find((n) => nodeKey(n) === edge.from),
              );
              const to = createMemo(() =>
                visibleNodes().find((n) => nodeKey(n) === edge.to),
              );
              // Clip endpoints to node borders so arrow lands on edge, not center
              const geom = createMemo(() => {
                const f = from();
                const t = to();
                if (!f || !t) return null;
                const fd = nodeDims(f);
                const td = nodeDims(t);
                const cxA = f.x + fd.width / 2;
                const cyA = f.y + fd.height / 2;
                const cxB = t.x + td.width / 2;
                const cyB = t.y + td.height / 2;
                const dx = cxB - cxA;
                const dy = cyB - cyA;
                const len = Math.hypot(dx, dy);
                if (len < 1) return null;
                const clip = (w: number, h: number) => {
                  const adx = Math.abs(dx) || 1e-6;
                  const ady = Math.abs(dy) || 1e-6;
                  return Math.min(w / 2 / adx, h / 2 / ady);
                };
                const tA = clip(fd.width, fd.height);
                const tB = clip(td.width, td.height);
                const x1 = cxA + dx * tA;
                const y1 = cyA + dy * tA;
                const x2 = cxB - dx * tB;
                const y2 = cyB - dy * tB;
                const mx = (x1 + x2) / 2;
                const my = (y1 + y2) / 2;
                return { x1, y1, x2, y2, mx, my };
              });

              const label = () => {
                const r = edge.relation;
                if (!r || r === "relates_to") return "";
                return r.replaceAll("_", " ");
              };

              return (
                <Show when={geom()}>
                  <g class="canvas-edge">
                    {/* Invisible wider hit target for hover */}
                    <line
                      x1={geom()!.x1}
                      y1={geom()!.y1}
                      x2={geom()!.x2}
                      y2={geom()!.y2}
                      stroke="transparent"
                      stroke-width={12}
                      class="canvas-edge-hit"
                    />
                    {/* Visible line with arrow */}
                    <line
                      x1={geom()!.x1}
                      y1={geom()!.y1}
                      x2={geom()!.x2}
                      y2={geom()!.y2}
                      class="canvas-edge-line"
                      marker-end="url(#canvas-edge-arrow)"
                    />
                    {/* Midpoint label (shown on hover via CSS) */}
                    <Show when={label()}>
                      <g
                        class="canvas-edge-label"
                        transform={`translate(${geom()!.mx}, ${geom()!.my})`}
                      >
                        <rect
                          class="canvas-edge-label-bg"
                          x={-label().length * 3.2 - 6}
                          y={-8}
                          width={label().length * 6.4 + 12}
                          height={16}
                          rx={0}
                        />
                        <text
                          class="canvas-edge-label-text"
                          text-anchor="middle"
                          dominant-baseline="central"
                        >
                          {label()}
                        </text>
                      </g>
                    </Show>
                  </g>
                </Show>
              );
            }}
          </For>
        </svg>

        {/* Nodes */}
        <For each={visibleNodes()}>
          {(node) => {
            const style = NODE_STYLES[node.kind] ?? NODE_STYLES.task;
            const dims = () => nodeDims(node);
            const isExpanded = () => expandedId() === nodeKey(node);
            const isDragging = () => draggingId() === nodeKey(node);

            const hasHighlight = () => !!node.highlight;
            return (
              <div
                data-node-id={nodeKey(node)}
                class={cn(
                  "canvas-node",
                  `canvas-node-${node.kind}`,
                  isExpanded() ? "canvas-node-expanded" : "canvas-node-pill",
                  isDragging() && "canvas-node-dragging",
                  hasHighlight() && "canvas-node-highlighted",
                )}
                style={{
                  left: `${node.x}px`,
                  top: `${node.y}px`,
                  width: `${isExpanded() ? 380 : dims().width}px`,
                  "min-height": `${isExpanded() ? 80 : dims().height}px`,
                  "border-color": hasHighlight() ? "oklch(var(--signal-amber))" : style.color,
                }}
                onPointerDown={(e) => handleNodePointerDown(node, e)}
                onDblClick={(e) => {
                  e.stopPropagation();
                  openFocus(node);
                }}
              >
                <Show
                  when={isExpanded()}
                  fallback={
                    <>
                      <span
                        class="canvas-node-pill-dot"
                        style={{ background: style.color }}
                        aria-hidden="true"
                      />
                      <span class="canvas-node-pill-title">{node.label}</span>
                      <Show when={node.status && node.status !== "active"}>
                        <span class="canvas-node-pill-status">{node.status}</span>
                      </Show>
                    </>
                  }
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
                  <Show when={node.tags && node.tags.length > 0}>
                    <div class="tag-chip-row canvas-node-tags">
                      <For each={node.tags ?? []}>
                        {(tag) => (
                          <span class="tag-chip">
                            <span class="tag-chip-label">{tag}</span>
                          </span>
                        )}
                      </For>
                    </div>
                  </Show>
                </Show>
                <Show when={hasHighlight() && isExpanded()}>
                  <div class="canvas-node-highlight-preview">
                    {node.highlight}
                  </div>
                </Show>

                {/* Hover actions (expanded only) */}
                <Show when={isExpanded()}>
                  <div
                    class="canvas-node-hover-actions"
                    onPointerDown={(e) => e.stopPropagation()}
                  >
                    <Show when={node.content}>
                      <button
                        type="button"
                        class="canvas-node-hover-action"
                        title="Copy contents"
                        onClick={(e) => {
                          e.stopPropagation();
                          void navigator.clipboard.writeText(node.content ?? "");
                        }}
                      >
                        <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                          <rect x="5" y="5" width="9" height="9" />
                          <path d="M11 5V3a1 1 0 0 0-1-1H3a1 1 0 0 0-1 1v7a1 1 0 0 0 1 1h2" />
                        </svg>
                      </button>
                    </Show>
                    <button
                      type="button"
                      class="canvas-node-hover-action"
                      title="Expand to focus"
                      onClick={(e) => {
                        e.stopPropagation();
                        openFocus(node);
                      }}
                    >
                      <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M3 7V3h4M13 9v4H9M3 9v4h4M13 7V3H9" />
                      </svg>
                    </button>
                    <button
                      type="button"
                      class="canvas-node-hover-action is-danger"
                      title={node.kind === "thread" ? "Archive thread" : "Delete"}
                      onClick={async (e) => {
                        e.stopPropagation();
                        const verb = node.kind === "thread" ? "Archive" : "Delete";
                        if (!confirm(`${verb} "${node.label}"?`)) return;
                        await deleteCanvasNode(props.projectId, node.kind, node.id);
                        setExpandedId(null);
                        await loadCanvas();
                      }}
                    >
                      <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M3 4h10M6 4V2h4v2M5 4l1 10h4l1-10" />
                      </svg>
                    </button>
                  </div>
                </Show>

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
                        onNavigate={openFocusByKey}
                        resolveNode={nodeByKey}
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
                        class={cn(
                          "canvas-node-markdown",
                          node.kind === "document" && node.subtype === "html"
                            ? "canvas-scope"
                            : "markdown-body",
                        )}
                        data-canvas-project-id={props.projectId}
                        onClick={handleMarkdownClick}
                        innerHTML={
                          node.kind === "document" && node.subtype === "html"
                            ? (node.content ?? "")
                            : renderNodeMarkdown(node.content ?? "", nodeByKey)
                        }
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
          <div class="canvas-empty-title">A blank canvas</div>
          <div class="canvas-empty-body">
            This is where you'll keep your tasks, notes, and decisions.
            Start with something small — you can expand from here.
          </div>
          <div class="canvas-empty-actions">
            <button
              class="canvas-empty-cta"
              onClick={() => openNewNode("task")}
            >
              <span
                class="canvas-empty-cta-dot"
                style={{ background: NODE_STYLES.task.color }}
              />
              <span class="canvas-empty-cta-label">Create a task</span>
              <span class="canvas-empty-cta-hint">Something to do</span>
            </button>
            <button
              class="canvas-empty-cta"
              onClick={() => openNewNode("document")}
            >
              <span
                class="canvas-empty-cta-dot"
                style={{ background: NODE_STYLES.document.color }}
              />
              <span class="canvas-empty-cta-label">Create a document</span>
              <span class="canvas-empty-cta-hint">Notes, specs, context</span>
            </button>
          </div>
        </div>
      </Show>

      {/* First-time welcome banner */}
      <Show when={showWelcomeBanner() && !loading() && nodes().length > 0}>
        <div class="canvas-welcome-banner">
          <span class="canvas-welcome-banner-text">
            <kbd>⌘K</kbd> to jump anywhere · drag nodes to arrange · double-click
            to focus
          </span>
          <button
            type="button"
            class="canvas-welcome-banner-close"
            onClick={dismissWelcome}
            title="Dismiss"
          >
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round">
              <path d="M2 2l6 6M8 2L2 8" />
            </svg>
          </button>
        </div>
      </Show>

      {/* Zoom indicator */}
      <div class="canvas-zoom-indicator">{Math.round(zoom() * 100)}%</div>

      {/* Save status pill */}
      <Show when={saveStatus() !== "idle" && saveStatus() !== "saving"}>
        <div
          class={cn(
            "canvas-save-pill",
            saveStatus() === "failed" && "is-failed",
          )}
        >
          {saveStatus() === "saved" ? "Saved" : "Save failed"}
        </div>
      </Show>

      {/* Focus mode overlay */}
      <Show when={focusedKey() && nodeByKey(focusedKey()!)}>
        <FocusView
          node={nodeByKey(focusedKey()!)!}
          projectId={props.projectId}
          resolveNode={nodeByKey}
          onNavigate={openFocusByKey}
          onClose={() => setFocusedKey(null)}
          onChange={() => void loadCanvas()}
          onOpenThread={props.onOpenThread}
          chatSlot={props.focusChatSlot}
        />
      </Show>

      {/* New task dialog */}
      <Show when={newTaskOpen()}>
        <div class="canvas-dialog-overlay" onClick={() => setNewTaskOpen(false)}>
          <div
            class="canvas-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="canvas-new-task-title"
            onClick={(e) => e.stopPropagation()}
            onKeyDown={(e) => {
              if (e.key === "Escape") setNewTaskOpen(false);
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void submitNewTask();
            }}
          >
            <div class="canvas-dialog-header">
              <span
                class="canvas-dialog-eyebrow"
                style={{ color: (NODE_STYLES[newKind()] ?? NODE_STYLES.task).color }}
              >
                New
              </span>
              <h2 class="canvas-dialog-title" id="canvas-new-task-title">
                {newKind().charAt(0).toUpperCase() + newKind().slice(1)}
              </h2>
            </div>
            <form
              class="canvas-dialog-body"
              onSubmit={(e) => {
                e.preventDefault();
                void submitNewTask();
              }}
            >
              <label class="canvas-dialog-label" for="canvas-new-task-input">
                Title
              </label>
              <input
                ref={newTaskInputRef}
                id="canvas-new-task-input"
                class="canvas-dialog-input"
                type="text"
                value={newTaskTitle()}
                placeholder={
                  newKind() === "task"
                    ? "Short, action-oriented title"
                    : newKind() === "document"
                      ? "Document title"
                      : newKind() === "goal"
                        ? "What are you aiming for?"
                        : "What was decided?"
                }
                onInput={(e) => setNewTaskTitle(e.currentTarget.value)}
              />
              <Show when={newKind() === "document"}>
                <label class="canvas-dialog-label">Format</label>
                <div class="canvas-dialog-segmented" role="radiogroup" aria-label="Document format">
                  <button
                    type="button"
                    role="radio"
                    aria-checked={newDocumentSubtype() === "markdown"}
                    class={cn(
                      "canvas-dialog-segmented-btn",
                      newDocumentSubtype() === "markdown" && "is-active",
                    )}
                    onClick={() => setNewDocumentSubtype("markdown")}
                  >
                    Markdown
                  </button>
                  <button
                    type="button"
                    role="radio"
                    aria-checked={newDocumentSubtype() === "html"}
                    class={cn(
                      "canvas-dialog-segmented-btn",
                      newDocumentSubtype() === "html" && "is-active",
                    )}
                    onClick={() => setNewDocumentSubtype("html")}
                  >
                    HTML
                  </button>
                </div>
              </Show>
              <label class="canvas-dialog-label">
                Tags <span class="canvas-dialog-label-hint">optional</span>
              </label>
              <TagEditor
                value={newTaskTags()}
                onChange={(tags) => {
                  setNewTaskTags(tags);
                }}
                placeholder="add tags…"
              />
              <label class="canvas-dialog-label" for="canvas-new-task-body">
                Notes{" "}
                <span class="canvas-dialog-label-hint">
                  optional ·{" "}
                  {newKind() === "document"
                    ? newDocumentSubtype() === "html"
                      ? "html"
                      : "markdown"
                    : "markdown"}
                </span>
              </label>
              <textarea
                id="canvas-new-task-body"
                class="canvas-dialog-textarea"
                value={newTaskBody()}
                placeholder="Context, acceptance criteria, links…"
                onInput={(e) => setNewTaskBody(e.currentTarget.value)}
                rows={5}
              />
              <div class="canvas-dialog-footer">
                <span class="canvas-dialog-hint">⌘↵ to create</span>
                <div class="canvas-dialog-actions">
                  <button
                    type="button"
                    class="canvas-toolbar-btn"
                    onClick={() => setNewTaskOpen(false)}
                  >
                    Cancel
                  </button>
                  <button
                    type="submit"
                    class="canvas-toolbar-btn canvas-dialog-primary"
                    disabled={!newTaskTitle().trim() || newTaskSaving()}
                  >
                    {newTaskSaving() ? "Creating…" : "Create task"}
                  </button>
                </div>
              </div>
            </form>
          </div>
        </div>
      </Show>
    </div>
  );
};

// ─── Expanded views ──────────────────────────────

const TaskExpanded: Component<{
  node: CanvasNode;
  projectId: number;
  onChange: () => void;
  onOpenThread?: (id: string) => void;
  onNavigate?: (key: string) => void;
  resolveNode?: (key: string) => CanvasNode | undefined;
}> = (props) => {
  const [editContent, setEditContent] = createSignal(props.node.content ?? "");
  const [editing, setEditing] = createSignal(false);

  const handleSave = async () => {
    await updateCanvasNode(props.projectId, props.node.kind, props.node.id, {
      content: editContent(),
    });
    setEditing(false);
    props.onChange();
  };

  const handleCycleStatus = async () => {
    const order = ["todo", "active", "review", "done"];
    const curr = props.node.status ?? "todo";
    const next = order[(order.indexOf(curr) + 1) % order.length];
    await updateCanvasNode(props.projectId, props.node.kind, props.node.id, { status: next });
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
            onClick={(e) => {
              const hit = (e.target as HTMLElement | null)?.closest("[data-node-key]");
              if (hit) {
                e.preventDefault();
                e.stopPropagation();
                const k = hit.getAttribute("data-node-key");
                if (k) props.onNavigate?.(k);
                return;
              }
              setEditContent(props.node.content ?? "");
              setEditing(true);
            }}
            innerHTML={
              props.node.content
                ? renderNodeMarkdown(
                    props.node.content,
                    props.resolveNode ?? (() => undefined),
                  )
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
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              void handleSave();
            }
            if (e.key === "Escape") {
              e.preventDefault();
              setEditContent(props.node.content ?? "");
              setEditing(false);
            }
          }}
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

// ─── Focus mode ──────────────────────────────────

const FocusView: Component<{
  node: CanvasNode;
  projectId: number;
  resolveNode: (key: string) => CanvasNode | undefined;
  onNavigate: (key: string) => void;
  onClose: () => void;
  onChange: () => void;
  onOpenThread?: (id: string) => void;
  chatSlot?: () => JSX.Element;
}> = (props) => {
  const editable = () =>
    ["task", "document", "goal", "decision"].includes(props.node.kind);
  const [editing, setEditing] = createSignal(false);
  const [draft, setDraft] = createSignal(props.node.content ?? "");
  const [copied, setCopied] = createSignal(false);

  const style = () => NODE_STYLES[props.node.kind] ?? NODE_STYLES.task;

  const handleMarkdownClick = (e: MouseEvent) => {
    const hit = (e.target as HTMLElement | null)?.closest("[data-node-key]");
    if (!hit) return;
    e.preventDefault();
    const key = hit.getAttribute("data-node-key");
    if (key) props.onNavigate(key);
  };

  const handleSave = async () => {
    if (!editable()) {
      setEditing(false);
      return;
    }
    await updateCanvasNode(props.projectId, props.node.kind, props.node.id, {
      content: draft(),
    });
    setEditing(false);
    props.onChange();
  };

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(props.node.content ?? "");
      setCopied(true);
      setTimeout(() => setCopied(false), 1400);
    } catch {
      /* ignore */
    }
  };

  const handleDelete = async () => {
    const verb = props.node.kind === "thread" ? "Archive" : "Delete";
    if (!confirm(`${verb} "${props.node.label}"?`)) return;
    await deleteCanvasNode(props.projectId, props.node.kind, props.node.id);
    props.onClose();
    props.onChange();
  };

  return (
    <div class="canvas-focus-overlay" onClick={props.onClose}>
      <div
        class="canvas-focus"
        role="dialog"
        aria-modal="true"
        aria-labelledby="canvas-focus-title"
        onClick={(e) => e.stopPropagation()}
      >
        <header class="canvas-focus-header">
          <button
            class="canvas-focus-back"
            type="button"
            onClick={props.onClose}
            title="Back to canvas (Esc)"
          >
            <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
              <path d="M10 3L5 8l5 5" />
            </svg>
            <span>Canvas</span>
          </button>
          <span class="canvas-focus-sep" aria-hidden="true">/</span>
          <span class="canvas-focus-kind" style={{ color: style().color }}>
            {props.node.kind}
          </span>
          <Show when={props.node.kind === "document"}>
            <button
              type="button"
              class="canvas-focus-subtype-toggle"
              title={`Rendering as ${props.node.subtype === "html" ? "HTML" : "markdown"}. Click to toggle.`}
              onClick={async () => {
                const next = props.node.subtype === "html" ? "markdown" : "html";
                await updateCanvasNode(props.projectId, props.node.kind, props.node.id, {
                  subtype: next,
                });
                props.onChange();
              }}
            >
              {props.node.subtype === "html" ? "html" : "markdown"}
            </button>
          </Show>
          <Show when={props.node.status}>
            <span class="canvas-focus-status">{props.node.status}</span>
          </Show>
          <div class="canvas-focus-spacer" />
          <button
            type="button"
            class="canvas-focus-action"
            title={copied() ? "Copied!" : "Copy contents"}
            onClick={() => void handleCopy()}
          >
            <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
              <rect x="5" y="5" width="9" height="9" />
              <path d="M11 5V3a1 1 0 0 0-1-1H3a1 1 0 0 0-1 1v7a1 1 0 0 0 1 1h2" />
            </svg>
          </button>
          <button
            type="button"
            class="canvas-focus-action is-danger"
            title={props.node.kind === "thread" ? "Archive thread" : "Delete"}
            onClick={() => void handleDelete()}
          >
            <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
              <path d="M3 4h10M6 4V2h4v2M5 4l1 10h4l1-10" />
            </svg>
          </button>
          <button
            type="button"
            class="canvas-focus-action"
            title="Close (Esc)"
            onClick={props.onClose}
          >
            <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
              <path d="M3 3l10 10M13 3L3 13" />
            </svg>
          </button>
        </header>

        <div
          class={cn(
            "canvas-focus-body",
            props.chatSlot && "canvas-focus-body-split",
          )}
        >
          <Show when={props.chatSlot}>
            <aside class="canvas-focus-chat">{props.chatSlot!()}</aside>
          </Show>
          <div class="canvas-focus-inner">
            <h1 class="canvas-focus-title" id="canvas-focus-title">
              {props.node.label}
            </h1>
            <Show when={editable()}>
              <div class="canvas-focus-tags">
                <TagEditor
                  value={(props.node.tags ?? []) as string[]}
                  onChange={async (tags) => {
                    await updateCanvasNode(
                      props.projectId,
                      props.node.kind,
                      props.node.id,
                      { tags },
                    );
                    props.onChange();
                  }}
                  placeholder="add tags…"
                />
              </div>
            </Show>
            <Show
              when={editing()}
              fallback={
                <div
                  class={cn(
                    "canvas-focus-markdown",
                    props.node.kind === "document" && props.node.subtype === "html"
                      ? "canvas-scope"
                      : "markdown-body",
                    editable() && "is-editable",
                  )}
                  data-canvas-project-id={props.projectId}
                  onClick={(e) => {
                    const hit = (e.target as HTMLElement | null)?.closest("[data-node-key]");
                    if (hit) {
                      handleMarkdownClick(e);
                      return;
                    }
                    if (editable()) {
                      setDraft(props.node.content ?? "");
                      setEditing(true);
                    }
                  }}
                  innerHTML={
                    props.node.content
                      ? props.node.kind === "document" && props.node.subtype === "html"
                        ? props.node.content
                        : renderNodeMarkdown(props.node.content, props.resolveNode)
                      : editable()
                        ? '<p class="canvas-focus-placeholder">Click to add notes…</p>'
                        : '<p class="canvas-focus-placeholder">No content.</p>'
                  }
                />
              }
            >
              <textarea
                class="canvas-focus-textarea"
                value={draft()}
                onInput={(e) => setDraft(e.currentTarget.value)}
                onBlur={() => void handleSave()}
                onKeyDown={(e) => {
                  if (e.key === "Escape") {
                    setEditing(false);
                    setDraft(props.node.content ?? "");
                  }
                  if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                    void handleSave();
                  }
                }}
                autofocus
              />
            </Show>
          </div>
        </div>
      </div>
    </div>
  );
};

export default CanvasView;
