import { type Component, For, Show, createEffect, createSignal, on, onCleanup, onMount } from "solid-js";
import * as THREE from "three";
import { getKnowledgeGraph, type KnowledgeGraphNode, type KnowledgeGraphEdge } from "@/lib/api";

// ── Node kind → color mapping ──

const KIND_COLORS: Record<string, number> = {
  module:      0x5a9bcf,
  function:    0x7cc47c,
  feature:     0xd4a843,
  bug:         0xd45050,
  idea:        0xb07cd4,
  observation: 0x5ac4c4,
  decision:    0xd4843a,
  risk:        0xd45080,
};

const KIND_CSS: Record<string, string> = {
  module:      "#5a9bcf",
  function:    "#7cc47c",
  feature:     "#d4a843",
  bug:         "#d45050",
  idea:        "#b07cd4",
  observation: "#5ac4c4",
  decision:    "#d4843a",
  risk:        "#d45080",
};

const DEFAULT_COLOR = 0x888888;
const DEFAULT_CSS = "#888888";
const EDGE_COLOR = 0x444444;
const EDGE_HIGHLIGHT = 0x999999;
const LABEL_COLOR = "#c8c8c8";
const LABEL_DIM = "#666666";

function getBgColor(): number {
  const style = getComputedStyle(document.documentElement);
  const raw = style.getPropertyValue("--background").trim();
  const match = raw.match(/([\d.]+)\s+([\d.]+)%\s+([\d.]+)%/);
  if (match) {
    const [, h, s, l] = match.map(Number);
    const el = document.createElement("div");
    el.style.color = `hsl(${h} ${s}% ${l}%)`;
    document.body.appendChild(el);
    const computed = getComputedStyle(el).color;
    el.remove();
    const rgb = computed.match(/\d+/g);
    if (rgb && rgb.length >= 3) {
      return (Number(rgb[0]) << 16) | (Number(rgb[1]) << 8) | Number(rgb[2]);
    }
  }
  return 0x111114;
}

// ── Force simulation ──

interface SimNode {
  key: string;
  kind: string;
  label: string;
  summary: string;
  confidence: string;
  source: string;
  nodeId: string;
  metadata: Record<string, unknown>;
  updatedAt: string;
  x: number;
  y: number;
  vx: number;
  vy: number;
  mesh: THREE.Mesh;
  ring: THREE.LineLoop;
  labelSprite: THREE.Sprite;
  raw: KnowledgeGraphNode;
}

interface SimEdge {
  from: string;
  to: string;
  relation: string;
  line: THREE.Line;
}

function extractRecordKey(id: unknown): string {
  if (typeof id === "string") return id;
  if (id && typeof id === "object") {
    const obj = id as Record<string, unknown>;
    if (obj.tb && obj.id) {
      const inner = obj.id;
      if (Array.isArray(inner)) return `${obj.tb}:${inner.join(",")}`;
      if (inner && typeof inner === "object" && "String" in (inner as Record<string, unknown>)) {
        return `${obj.tb}:${String((inner as Record<string, unknown>).String)}`;
      }
      return `${obj.tb}:${String(inner)}`;
    }
  }
  return String(id);
}

function makeTextSprite(text: string, color: string): THREE.Sprite {
  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d")!;
  const fontSize = 28;
  ctx.font = `500 ${fontSize}px system-ui, sans-serif`;
  const metrics = ctx.measureText(text);
  const width = Math.ceil(metrics.width) + 12;
  const height = fontSize + 8;
  canvas.width = width;
  canvas.height = height;
  ctx.font = `500 ${fontSize}px system-ui, sans-serif`;
  ctx.fillStyle = color;
  ctx.textBaseline = "middle";
  ctx.fillText(text, 6, height / 2);
  const texture = new THREE.CanvasTexture(canvas);
  texture.minFilter = THREE.LinearFilter;
  const material = new THREE.SpriteMaterial({ map: texture, transparent: true, depthTest: false });
  const sprite = new THREE.Sprite(material);
  sprite.scale.set(width / 28, height / 28, 1);
  return sprite;
}

function nodeRadius(kind: string): number {
  switch (kind) {
    case "feature": return 0.55;
    case "module": return 0.45;
    case "idea": case "decision": return 0.4;
    default: return 0.35;
  }
}

function simulateForces(nodes: SimNode[], edges: SimEdge[], alpha: number) {
  const repulsion = 3.0;
  const attraction = 0.008;
  const damping = 0.88;
  const centerGravity = 0.002;

  for (let i = 0; i < nodes.length; i++) {
    for (let j = i + 1; j < nodes.length; j++) {
      const a = nodes[i];
      const b = nodes[j];
      let dx = a.x - b.x;
      let dy = a.y - b.y;
      const dist = Math.sqrt(dx * dx + dy * dy) || 0.01;
      const force = (repulsion * alpha) / (dist * dist);
      dx *= force / dist;
      dy *= force / dist;
      a.vx += dx;
      a.vy += dy;
      b.vx -= dx;
      b.vy -= dy;
    }
  }

  const nodeMap = new Map<string, SimNode>();
  for (const n of nodes) nodeMap.set(n.key, n);

  for (const edge of edges) {
    const a = nodeMap.get(edge.from);
    const b = nodeMap.get(edge.to);
    if (!a || !b) continue;
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const dist = Math.sqrt(dx * dx + dy * dy) || 0.01;
    const force = attraction * dist * alpha;
    a.vx += (dx / dist) * force;
    a.vy += (dy / dist) * force;
    b.vx -= (dx / dist) * force;
    b.vy -= (dy / dist) * force;
  }

  for (const n of nodes) {
    n.vx -= n.x * centerGravity * alpha;
    n.vy -= n.y * centerGravity * alpha;
  }

  for (const n of nodes) {
    n.vx *= damping;
    n.vy *= damping;
    n.x += n.vx;
    n.y += n.vy;
  }
}

// ── Component ──

interface KnowledgeGraphViewProps {
  projectId: number;
  reloadToken?: number;
}

const KnowledgeGraphView: Component<KnowledgeGraphViewProps> = (props) => {
  let containerRef: HTMLDivElement | undefined;
  let renderer: THREE.WebGLRenderer | null = null;
  let scene: THREE.Scene | null = null;
  let camera: THREE.OrthographicCamera | null = null;
  let animFrame = 0;
  let simNodes: SimNode[] = [];
  let simEdges: SimEdge[] = [];
  let alpha = 1.0;
  let resizeObserver: ResizeObserver | undefined;
  const raycaster = new THREE.Raycaster();
  const pointer = new THREE.Vector2();

  const [loading, setLoading] = createSignal(true);
  const [nodeCount, setNodeCount] = createSignal(0);
  const [hovered, setHovered] = createSignal<SimNode | null>(null);
  const [selected, setSelected] = createSignal<SimNode | null>(null);
  const [search, setSearch] = createSignal("");
  const [hiddenKinds, setHiddenKinds] = createSignal<Set<string>>(new Set());
  const [allKinds, setAllKinds] = createSignal<string[]>([]);
  const [connectedKeys, setConnectedKeys] = createSignal<Set<string>>(new Set());

  let panOffset = { x: 0, y: 0 };
  let zoom = 1;
  let isPanning = false;
  let panStart = { x: 0, y: 0 };
  let didDrag = false;

  const updateCamera = () => {
    if (!camera || !containerRef) return;
    const w = containerRef.clientWidth;
    const h = containerRef.clientHeight;
    const aspect = w / h;
    const half = 20 / zoom;
    camera.left = -half * aspect + panOffset.x;
    camera.right = half * aspect + panOffset.x;
    camera.top = half + panOffset.y;
    camera.bottom = -half + panOffset.y;
    camera.updateProjectionMatrix();
  };

  const getConnectedKeys = (nodeKey: string): Set<string> => {
    const keys = new Set<string>();
    for (const e of simEdges) {
      if (e.from === nodeKey) keys.add(e.to);
      if (e.to === nodeKey) keys.add(e.from);
    }
    return keys;
  };

  const getConnectedEdges = (nodeKey: string): SimEdge[] => {
    return simEdges.filter((e) => e.from === nodeKey || e.to === nodeKey);
  };

  const getConnectedNodes = (nodeKey: string): { node: SimNode; relation: string; direction: "in" | "out" }[] => {
    const result: { node: SimNode; relation: string; direction: "in" | "out" }[] = [];
    const nodeMap = new Map<string, SimNode>();
    for (const n of simNodes) nodeMap.set(n.key, n);
    for (const e of simEdges) {
      if (e.from === nodeKey) {
        const n = nodeMap.get(e.to);
        if (n) result.push({ node: n, relation: e.relation, direction: "out" });
      }
      if (e.to === nodeKey) {
        const n = nodeMap.get(e.from);
        if (n) result.push({ node: n, relation: e.relation, direction: "in" });
      }
    }
    return result;
  };

  const hitTest = (clientX: number, clientY: number): SimNode | null => {
    if (!camera || !containerRef || !renderer) return null;
    const rect = renderer.domElement.getBoundingClientRect();
    pointer.x = ((clientX - rect.left) / rect.width) * 2 - 1;
    pointer.y = -((clientY - rect.top) / rect.height) * 2 + 1;
    raycaster.setFromCamera(pointer, camera);
    const meshes = simNodes.filter((n) => n.mesh.visible).map((n) => n.mesh);
    const intersects = raycaster.intersectObjects(meshes);
    if (intersects.length > 0) {
      return simNodes.find((n) => n.mesh === intersects[0].object) ?? null;
    }
    return null;
  };

  const panToNode = (node: SimNode) => {
    panOffset.x = node.x;
    panOffset.y = node.y;
    zoom = Math.max(zoom, 1.5);
    updateCamera();
  };

  const selectNode = (node: SimNode | null) => {
    setSelected(node);
    if (node) {
      setConnectedKeys(getConnectedKeys(node.key));
    } else {
      setConnectedKeys(new Set());
    }
  };

  const buildGraph = (data: { nodes: KnowledgeGraphNode[]; edges: KnowledgeGraphEdge[] }) => {
    if (!scene) return;

    for (const n of simNodes) {
      scene.remove(n.mesh);
      scene.remove(n.ring);
      scene.remove(n.labelSprite);
    }
    for (const e of simEdges) {
      scene.remove(e.line);
    }
    simNodes = [];
    simEdges = [];
    alpha = 1.0;

    const kinds = new Set<string>();

    for (const node of data.nodes) {
      const key = extractRecordKey(node.id);
      const color = KIND_COLORS[node.kind] ?? DEFAULT_COLOR;
      const r = nodeRadius(node.kind);

      const geo = new THREE.CircleGeometry(r, 32);
      const mat = new THREE.MeshBasicMaterial({ color });
      const mesh = new THREE.Mesh(geo, mat);

      // Selection ring
      const ringGeo = new THREE.BufferGeometry().setFromPoints(
        Array.from({ length: 33 }, (_, i) => {
          const angle = (i / 32) * Math.PI * 2;
          return new THREE.Vector3(Math.cos(angle) * (r + 0.15), Math.sin(angle) * (r + 0.15), 0.05);
        }),
      );
      const ringMat = new THREE.LineBasicMaterial({ color, transparent: true, opacity: 0 });
      const ring = new THREE.LineLoop(ringGeo, ringMat);

      const label = node.label || node.node_id || node.kind;
      const sprite = makeTextSprite(label.length > 20 ? label.slice(0, 18) + "\u2026" : label, LABEL_COLOR);

      mesh.position.set((Math.random() - 0.5) * 10, (Math.random() - 0.5) * 10, 0);
      ring.position.copy(mesh.position);
      sprite.position.set(mesh.position.x, mesh.position.y - r - 0.5, 0.1);

      scene.add(mesh);
      scene.add(ring);
      scene.add(sprite);

      kinds.add(node.kind);

      simNodes.push({
        key,
        kind: node.kind,
        label,
        summary: node.summary || "",
        confidence: node.confidence || "",
        source: node.source || "",
        nodeId: node.node_id || "",
        metadata: node.metadata || {},
        updatedAt: node.updated_at || "",
        x: mesh.position.x,
        y: mesh.position.y,
        vx: 0,
        vy: 0,
        mesh,
        ring,
        labelSprite: sprite,
        raw: node,
      });
    }

    for (const edge of data.edges) {
      const fromKey = extractRecordKey(edge.in);
      const toKey = extractRecordKey(edge.out);
      const geo = new THREE.BufferGeometry().setFromPoints([
        new THREE.Vector3(0, 0, -0.1),
        new THREE.Vector3(0, 0, -0.1),
      ]);
      const mat = new THREE.LineBasicMaterial({ color: EDGE_COLOR, transparent: true, opacity: 0.4 });
      const line = new THREE.Line(geo, mat);
      scene.add(line);
      simEdges.push({ from: fromKey, to: toKey, relation: edge.relation, line });
    }

    setNodeCount(simNodes.length);
    setAllKinds(Array.from(kinds).sort());
  };

  const updatePositions = () => {
    const nodeMap = new Map<string, SimNode>();
    for (const n of simNodes) nodeMap.set(n.key, n);

    const hidden = hiddenKinds();
    const sel = selected();
    const hov = hovered();
    const conn = connectedKeys();
    const searchTerm = search().toLowerCase();

    for (const n of simNodes) {
      const isHidden = hidden.has(n.kind);
      const isSearchFiltered = searchTerm && !n.label.toLowerCase().includes(searchTerm) && !n.kind.toLowerCase().includes(searchTerm);
      n.mesh.visible = !isHidden;
      n.ring.visible = !isHidden;
      n.labelSprite.visible = !isHidden;

      if (isHidden) continue;

      n.mesh.position.set(n.x, n.y, 0);
      n.ring.position.set(n.x, n.y, 0);
      const r = nodeRadius(n.kind);
      n.labelSprite.position.set(n.x, n.y - r - 0.5, 0.1);

      // Visual states
      const isSelected = sel && sel.key === n.key;
      const isHovered = hov && hov.key === n.key;
      const isConnected = sel && conn.has(n.key);
      const isDimmed = (sel && !isSelected && !isConnected) || isSearchFiltered;

      // Ring visibility
      const ringMat = n.ring.material as THREE.LineBasicMaterial;
      if (isSelected) {
        ringMat.opacity = 0.9;
        ringMat.color.setHex(0xffffff);
      } else if (isHovered) {
        ringMat.opacity = 0.5;
        ringMat.color.setHex(KIND_COLORS[n.kind] ?? DEFAULT_COLOR);
      } else {
        ringMat.opacity = 0;
      }

      // Node opacity
      const meshMat = n.mesh.material as THREE.MeshBasicMaterial;
      meshMat.opacity = isDimmed ? 0.15 : 1;
      meshMat.transparent = isDimmed;

      // Label opacity
      const labelMat = n.labelSprite.material as THREE.SpriteMaterial;
      labelMat.opacity = isDimmed ? 0.1 : isSelected || isHovered ? 1 : 0.7;
    }

    for (const e of simEdges) {
      const a = nodeMap.get(e.from);
      const b = nodeMap.get(e.to);
      if (!a || !b) continue;
      const positions = (e.line.geometry as THREE.BufferGeometry).attributes.position;
      (positions as THREE.BufferAttribute).setXYZ(0, a.x, a.y, -0.1);
      (positions as THREE.BufferAttribute).setXYZ(1, b.x, b.y, -0.1);
      positions.needsUpdate = true;

      const lineMat = e.line.material as THREE.LineBasicMaterial;
      const isHidden = hidden.has(a.kind) || hidden.has(b.kind);
      e.line.visible = !isHidden;

      if (sel) {
        const isEdgeConnected = e.from === sel.key || e.to === sel.key;
        lineMat.opacity = isEdgeConnected ? 0.8 : 0.08;
        lineMat.color.setHex(isEdgeConnected ? EDGE_HIGHLIGHT : EDGE_COLOR);
      } else {
        lineMat.opacity = 0.4;
        lineMat.color.setHex(EDGE_COLOR);
      }
    }
  };

  const animate = () => {
    if (!renderer || !scene || !camera) return;
    if (alpha > 0.001) {
      simulateForces(simNodes, simEdges, alpha);
      alpha *= 0.995;
    }
    updatePositions();
    renderer.render(scene, camera);
    animFrame = requestAnimationFrame(animate);
  };

  const loadData = async () => {
    setLoading(true);
    try {
      const data = await getKnowledgeGraph(props.projectId);
      buildGraph(data);
    } catch {
      // empty graph is fine
    } finally {
      setLoading(false);
    }
  };

  onMount(() => {
    if (!containerRef) return;

    renderer = new THREE.WebGLRenderer({ antialias: true, alpha: false });
    renderer.setPixelRatio(window.devicePixelRatio);
    renderer.setClearColor(getBgColor());
    containerRef.appendChild(renderer.domElement);

    scene = new THREE.Scene();
    camera = new THREE.OrthographicCamera(-20, 20, 20, -20, 0.1, 100);
    camera.position.z = 50;

    const resize = () => {
      if (!containerRef || !renderer) return;
      const w = containerRef.clientWidth;
      const h = containerRef.clientHeight;
      renderer.setSize(w, h);
      updateCamera();
    };

    resizeObserver = new ResizeObserver(resize);
    resizeObserver.observe(containerRef);
    resize();

    containerRef.addEventListener("wheel", (e) => {
      e.preventDefault();
      zoom *= e.deltaY > 0 ? 0.92 : 1.08;
      zoom = Math.max(0.2, Math.min(5, zoom));
      updateCamera();
    }, { passive: false });

    containerRef.addEventListener("pointerdown", (e) => {
      isPanning = true;
      didDrag = false;
      panStart = { x: e.clientX, y: e.clientY };
      containerRef!.style.cursor = "grabbing";
    });

    const onPointerMove = (e: PointerEvent) => {
      if (isPanning && containerRef) {
        const dx = e.clientX - panStart.x;
        const dy = e.clientY - panStart.y;
        if (Math.abs(dx) > 3 || Math.abs(dy) > 3) didDrag = true;
        const scale = 40 / (zoom * containerRef.clientHeight);
        panOffset.x -= dx * scale;
        panOffset.y += dy * scale;
        panStart = { x: e.clientX, y: e.clientY };
        updateCamera();
      }

      // Hover detection
      const hit = hitTest(e.clientX, e.clientY);
      setHovered(hit);
      if (containerRef && !isPanning) {
        containerRef.style.cursor = hit ? "pointer" : "grab";
      }
    };

    const onPointerUp = (e: PointerEvent) => {
      const wasPanning = isPanning;
      isPanning = false;
      if (containerRef) containerRef.style.cursor = "grab";

      if (wasPanning && !didDrag) {
        const hit = hitTest(e.clientX, e.clientY);
        if (hit) {
          selectNode(hit);
        } else {
          selectNode(null);
        }
      }
    };

    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);

    containerRef.style.cursor = "grab";
    animate();
    void loadData();

    onCleanup(() => {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
    });
  });

  createEffect(on(() => props.projectId, () => { void loadData(); }));
  createEffect(on(() => props.reloadToken, () => { void loadData(); }));

  onCleanup(() => {
    cancelAnimationFrame(animFrame);
    resizeObserver?.disconnect();
    renderer?.dispose();
    if (renderer?.domElement.parentNode) {
      renderer.domElement.parentNode.removeChild(renderer.domElement);
    }
  });

  const toggleKind = (kind: string) => {
    setHiddenKinds((prev) => {
      const next = new Set(prev);
      if (next.has(kind)) next.delete(kind);
      else next.add(kind);
      return next;
    });
  };

  const navigateToNode = (node: SimNode) => {
    selectNode(node);
    panToNode(node);
  };

  return (
    <div class="kg-root" ref={containerRef}>
      {/* ── Top bar: search + kind filters ── */}
      <Show when={!loading() && nodeCount() > 0}>
        <div class="kg-toolbar">
          <div class="kg-search-wrap">
            <svg class="kg-search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <circle cx="11" cy="11" r="8" />
              <line x1="21" y1="21" x2="16.65" y2="16.65" />
            </svg>
            <input
              type="text"
              class="kg-search-input"
              placeholder="Search nodes..."
              value={search()}
              onInput={(e) => setSearch(e.currentTarget.value)}
            />
            <Show when={search()}>
              <button
                type="button"
                class="kg-search-clear"
                onClick={() => setSearch("")}
              >
                <svg viewBox="0 0 24 24" class="h-3 w-3" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M6 6l12 12" /><path d="M18 6L6 18" />
                </svg>
              </button>
            </Show>
          </div>
          <div class="kg-kind-filters">
            <For each={allKinds()}>
              {(kind) => (
                <button
                  type="button"
                  class="kg-kind-chip"
                  data-active={!hiddenKinds().has(kind)}
                  style={{ "--chip-color": KIND_CSS[kind] ?? DEFAULT_CSS }}
                  onClick={() => toggleKind(kind)}
                  title={`Toggle ${kind}`}
                >
                  <span class="kg-kind-dot" />
                  {kind}
                </button>
              )}
            </For>
          </div>
          <span class="kg-node-count">{nodeCount()}</span>
        </div>
      </Show>

      {/* ── Hover tooltip ── */}
      <Show when={hovered() && !selected()}>
        <div class="kg-tooltip">
          <div class="kg-tooltip-kind" style={{ color: KIND_CSS[hovered()!.kind] ?? DEFAULT_CSS }}>
            {hovered()!.kind}
          </div>
          <div class="kg-tooltip-label">{hovered()!.label}</div>
          <Show when={hovered()!.summary}>
            <div class="kg-tooltip-summary">{hovered()!.summary.slice(0, 120)}{hovered()!.summary.length > 120 ? "\u2026" : ""}</div>
          </Show>
        </div>
      </Show>

      {/* ── Detail panel ── */}
      <Show when={selected()}>
        <div class="kg-detail">
          <div class="kg-detail-header">
            <div class="kg-detail-kind-badge" style={{ "--badge-color": KIND_CSS[selected()!.kind] ?? DEFAULT_CSS }}>
              {selected()!.kind}
            </div>
            <button
              type="button"
              class="kg-detail-close"
              onClick={() => selectNode(null)}
            >
              <svg viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M6 6l12 12" /><path d="M18 6L6 18" />
              </svg>
            </button>
          </div>

          <h3 class="kg-detail-title">{selected()!.label}</h3>

          <Show when={selected()!.summary}>
            <p class="kg-detail-summary">{selected()!.summary}</p>
          </Show>

          <div class="kg-detail-meta">
            <Show when={selected()!.confidence}>
              <div class="kg-detail-field">
                <span class="kg-detail-field-label">Confidence</span>
                <span class="kg-detail-field-value">{selected()!.confidence}</span>
              </div>
            </Show>
            <Show when={selected()!.source}>
              <div class="kg-detail-field">
                <span class="kg-detail-field-label">Source</span>
                <span class="kg-detail-field-value">{selected()!.source}</span>
              </div>
            </Show>
            <Show when={selected()!.nodeId}>
              <div class="kg-detail-field">
                <span class="kg-detail-field-label">ID</span>
                <span class="kg-detail-field-value kg-mono">{selected()!.nodeId}</span>
              </div>
            </Show>
          </div>

          {/* Connected nodes */}
          <Show when={getConnectedNodes(selected()!.key).length > 0}>
            <div class="kg-detail-connections">
              <div class="kg-detail-section-label">Connections</div>
              <div class="kg-detail-conn-list">
                <For each={getConnectedNodes(selected()!.key)}>
                  {(conn) => (
                    <button
                      type="button"
                      class="kg-detail-conn-item"
                      onClick={() => navigateToNode(conn.node)}
                    >
                      <span class="kg-detail-conn-dot" style={{ background: KIND_CSS[conn.node.kind] ?? DEFAULT_CSS }} />
                      <span class="kg-detail-conn-label">{conn.node.label}</span>
                      <span class="kg-detail-conn-relation">
                        {conn.direction === "out" ? "\u2192" : "\u2190"} {conn.relation}
                      </span>
                    </button>
                  )}
                </For>
              </div>
            </div>
          </Show>
        </div>
      </Show>

      {/* ── Loading / Empty ── */}
      <Show when={loading()}>
        <div class="kg-status-overlay">
          <span class="kg-status-text">Loading graph\u2026</span>
        </div>
      </Show>
      <Show when={!loading() && nodeCount() === 0}>
        <div class="kg-empty">
          <svg viewBox="0 0 24 24" class="kg-empty-icon" fill="none" stroke="currentColor" stroke-width="1.5">
            <circle cx="12" cy="12" r="3" />
            <line x1="12" y1="3" x2="12" y2="9" />
            <line x1="12" y1="15" x2="12" y2="21" />
            <line x1="3" y1="12" x2="9" y2="12" />
            <line x1="15" y1="12" x2="21" y2="12" />
          </svg>
          <p class="kg-empty-title">Knowledge Graph</p>
          <p class="kg-empty-hint">
            Run a workspace scan or let the Librarian explore the codebase to seed the graph.
          </p>
        </div>
      </Show>
    </div>
  );
};

export default KnowledgeGraphView;
