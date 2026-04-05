import { type Component, For, Show, createEffect, createSignal, on, onCleanup, onMount } from "solid-js";
import * as THREE from "three";
import { getKnowledgeGraph, type KnowledgeGraphNode, type KnowledgeGraphEdge } from "@/lib/api";

// ── Color system ──

const KIND_COLORS: Record<string, number> = {
  module:      0x5a9bcf,
  function:    0x7cc47c,
  feature:     0xd4a843,
  bug:         0xd45050,
  idea:        0xb07cd4,
  observation: 0x5ac4c4,
  decision:    0xd4843a,
  risk:        0xd45080,
  lore:        0x8bc78b,
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
  lore:        "#8bc78b",
};

const DEFAULT_COLOR = 0x888888;
const DEFAULT_CSS = "#888888";
const EDGE_COLOR = 0x555555;
const EDGE_HIGHLIGHT = 0xaaaaaa;
const LABEL_COLOR = "#c8c8c8";

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

// ── Types ──

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
  pinned: boolean;
}

interface SimEdge {
  from: string;
  to: string;
  relation: string;
  line: THREE.Line;
  labelSprite: THREE.Sprite | null;
}

// ── Helpers ──

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

function makeTextSprite(text: string, color: string, fontSize = 28, fontWeight = "500"): THREE.Sprite {
  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d")!;
  ctx.font = `${fontWeight} ${fontSize}px system-ui, sans-serif`;
  const metrics = ctx.measureText(text);
  const width = Math.ceil(metrics.width) + 12;
  const height = fontSize + 8;
  canvas.width = width;
  canvas.height = height;
  ctx.font = `${fontWeight} ${fontSize}px system-ui, sans-serif`;
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

function makeEdgeLabelSprite(text: string): THREE.Sprite {
  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d")!;
  const fontSize = 20;
  ctx.font = `400 ${fontSize}px system-ui, sans-serif`;
  const metrics = ctx.measureText(text);
  const pad = 8;
  const width = Math.ceil(metrics.width) + pad * 2;
  const height = fontSize + pad;
  canvas.width = width;
  canvas.height = height;

  // Background pill
  ctx.fillStyle = "rgba(0,0,0,0.7)";
  ctx.fillRect(0, 0, width, height);
  ctx.fillStyle = "#999";
  ctx.font = `400 ${fontSize}px system-ui, sans-serif`;
  ctx.textBaseline = "middle";
  ctx.fillText(text, pad, height / 2);

  const texture = new THREE.CanvasTexture(canvas);
  texture.minFilter = THREE.LinearFilter;
  const material = new THREE.SpriteMaterial({ map: texture, transparent: true, depthTest: false, opacity: 0 });
  const sprite = new THREE.Sprite(material);
  sprite.scale.set(width / 28, height / 28, 1);
  return sprite;
}

function nodeRadius(kind: string): number {
  switch (kind) {
    case "feature": return 0.65;
    case "module": return 0.55;
    case "idea": case "decision": case "lore": return 0.5;
    default: return 0.42;
  }
}

// ── Force simulation ──

function simulateForces(nodes: SimNode[], edges: SimEdge[], alpha: number) {
  const repulsion = 3.5;
  const attraction = 0.008;
  const damping = 0.88;
  const centerGravity = 0.002;

  for (let i = 0; i < nodes.length; i++) {
    if (nodes[i].pinned) continue;
    for (let j = i + 1; j < nodes.length; j++) {
      const a = nodes[i];
      const b = nodes[j];
      let dx = a.x - b.x;
      let dy = a.y - b.y;
      const dist = Math.sqrt(dx * dx + dy * dy) || 0.01;
      const force = (repulsion * alpha) / (dist * dist);
      dx *= force / dist;
      dy *= force / dist;
      if (!a.pinned) { a.vx += dx; a.vy += dy; }
      if (!b.pinned) { b.vx -= dx; b.vy -= dy; }
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
    if (!a.pinned) { a.vx += (dx / dist) * force; a.vy += (dy / dist) * force; }
    if (!b.pinned) { b.vx -= (dx / dist) * force; b.vy -= (dy / dist) * force; }
  }

  for (const n of nodes) {
    if (n.pinned) continue;
    n.vx -= n.x * centerGravity * alpha;
    n.vy -= n.y * centerGravity * alpha;
    n.vx *= damping;
    n.vy *= damping;
    n.x += n.vx;
    n.y += n.vy;
  }
}

// ── Minimap renderer ──

function drawMinimap(
  canvas: HTMLCanvasElement,
  nodes: SimNode[],
  camera: THREE.OrthographicCamera,
  hidden: Set<string>,
) {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const w = canvas.width;
  const h = canvas.height;
  ctx.clearRect(0, 0, w, h);

  const visible = nodes.filter((n) => !hidden.has(n.kind));
  if (visible.length === 0) return;

  // Compute graph bounds
  let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
  for (const n of visible) {
    if (n.x < minX) minX = n.x;
    if (n.x > maxX) maxX = n.x;
    if (n.y < minY) minY = n.y;
    if (n.y > maxY) maxY = n.y;
  }
  const pad = 3;
  minX -= pad; maxX += pad; minY -= pad; maxY += pad;
  const gw = maxX - minX || 1;
  const gh = maxY - minY || 1;

  const toX = (x: number) => ((x - minX) / gw) * w;
  const toY = (y: number) => h - ((y - minY) / gh) * h;

  // Draw edges
  ctx.strokeStyle = "rgba(255,255,255,0.08)";
  ctx.lineWidth = 0.5;
  const nodeMap = new Map<string, SimNode>();
  for (const n of visible) nodeMap.set(n.key, n);

  // Draw nodes
  for (const n of visible) {
    const hex = KIND_COLORS[n.kind] ?? DEFAULT_COLOR;
    const r = ((hex >> 16) & 0xff);
    const g = ((hex >> 8) & 0xff);
    const b = (hex & 0xff);
    ctx.fillStyle = `rgba(${r},${g},${b},0.8)`;
    ctx.beginPath();
    ctx.arc(toX(n.x), toY(n.y), 2, 0, Math.PI * 2);
    ctx.fill();
  }

  // Draw viewport rect
  const vl = toX(camera.left);
  const vr = toX(camera.right);
  const vt = toY(camera.top);
  const vb = toY(camera.bottom);
  ctx.strokeStyle = "rgba(255,255,255,0.35)";
  ctx.lineWidth = 1;
  ctx.strokeRect(vl, vt, vr - vl, vb - vt);
}

// ── Component ──

interface KnowledgeGraphViewProps {
  projectId: number;
  reloadToken?: number;
}

const KnowledgeGraphView: Component<KnowledgeGraphViewProps> = (props) => {
  let containerRef: HTMLDivElement | undefined;
  let minimapRef: HTMLCanvasElement | undefined;
  let renderer: THREE.WebGLRenderer | null = null;
  let scene: THREE.Scene | null = null;
  let camera: THREE.OrthographicCamera | null = null;
  let animFrame = 0;
  let simNodes: SimNode[] = [];
  let simEdges: SimEdge[] = [];
  let alpha = 1.0;
  let resizeObserver: ResizeObserver | undefined;

  const [loading, setLoading] = createSignal(true);
  const [nodeCount, setNodeCount] = createSignal(0);
  const [hovered, setHovered] = createSignal<SimNode | null>(null);
  const [hoveredEdge, setHoveredEdge] = createSignal<SimEdge | null>(null);
  const [selected, setSelected] = createSignal<SimNode | null>(null);
  const [search, setSearch] = createSignal("");
  const [hiddenKinds, setHiddenKinds] = createSignal<Set<string>>(new Set());
  const [allKinds, setAllKinds] = createSignal<string[]>([]);
  const [connectedKeys, setConnectedKeys] = createSignal<Set<string>>(new Set());
  const [contextMenu, setContextMenu] = createSignal<{ x: number; y: number; node: SimNode } | null>(null);

  let panOffset = { x: 0, y: 0 };
  let zoom = 1;
  let isPanning = false;
  let panStart = { x: 0, y: 0 };
  let didDrag = false;
  let lastClickTime = 0;
  let draggingNode: SimNode | null = null;

  // Animated pan/zoom targets
  let animatingView = false;
  let animViewStart = { x: 0, y: 0, zoom: 1 };
  let animViewTarget = { x: 0, y: 0, zoom: 1 };
  let animViewProgress = 0;

  const screenToWorld = (clientX: number, clientY: number): { x: number; y: number } => {
    if (!camera || !containerRef || !renderer) return { x: 0, y: 0 };
    const rect = renderer.domElement.getBoundingClientRect();
    const ndcX = ((clientX - rect.left) / rect.width) * 2 - 1;
    const ndcY = -((clientY - rect.top) / rect.height) * 2 + 1;
    return {
      x: ((ndcX + 1) / 2) * (camera.right - camera.left) + camera.left,
      y: ((ndcY + 1) / 2) * (camera.top - camera.bottom) + camera.bottom,
    };
  };

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

  const animatePanTo = (x: number, y: number, targetZoom: number) => {
    animViewStart = { x: panOffset.x, y: panOffset.y, zoom };
    animViewTarget = { x, y, zoom: targetZoom };
    animViewProgress = 0;
    animatingView = true;
  };

  const getConnectedKeys = (nodeKey: string): Set<string> => {
    const keys = new Set<string>();
    for (const e of simEdges) {
      if (e.from === nodeKey) keys.add(e.to);
      if (e.to === nodeKey) keys.add(e.from);
    }
    return keys;
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

  const HIT_RADIUS_WORLD = 1.8;

  const hitTest = (clientX: number, clientY: number): SimNode | null => {
    if (!camera || !containerRef || !renderer) return null;
    const { x: worldX, y: worldY } = screenToWorld(clientX, clientY);
    let closest: SimNode | null = null;
    let closestDist = HIT_RADIUS_WORLD;
    for (const n of simNodes) {
      if (!n.mesh.visible) continue;
      const dx = n.x - worldX;
      const dy = n.y - worldY;
      const dist = Math.sqrt(dx * dx + dy * dy);
      if (dist < closestDist) {
        closestDist = dist;
        closest = n;
      }
    }
    return closest;
  };

  const edgeHitTest = (clientX: number, clientY: number): SimEdge | null => {
    if (!camera || !containerRef || !renderer) return null;
    const { x: wx, y: wy } = screenToWorld(clientX, clientY);
    const nodeMap = new Map<string, SimNode>();
    for (const n of simNodes) nodeMap.set(n.key, n);

    let closest: SimEdge | null = null;
    let closestDist = 1.2;

    for (const e of simEdges) {
      if (!e.line.visible) continue;
      const a = nodeMap.get(e.from);
      const b = nodeMap.get(e.to);
      if (!a || !b) continue;

      // Point-to-segment distance
      const dx = b.x - a.x;
      const dy = b.y - a.y;
      const len2 = dx * dx + dy * dy;
      if (len2 < 0.01) continue;
      const t = Math.max(0, Math.min(1, ((wx - a.x) * dx + (wy - a.y) * dy) / len2));
      const px = a.x + t * dx;
      const py = a.y + t * dy;
      const dist = Math.sqrt((wx - px) * (wx - px) + (wy - py) * (wy - py));
      if (dist < closestDist) {
        closestDist = dist;
        closest = e;
      }
    }
    return closest;
  };

  const selectNode = (node: SimNode | null) => {
    setSelected(node);
    setContextMenu(null);
    if (node) {
      setConnectedKeys(getConnectedKeys(node.key));
    } else {
      setConnectedKeys(new Set());
    }
  };

  const navigateToNode = (node: SimNode) => {
    selectNode(node);
    animatePanTo(node.x, node.y, Math.max(zoom, 1.5));
  };

  const zoomToNeighborhood = (node: SimNode) => {
    selectNode(node);
    const connected = getConnectedKeys(node.key);
    let minX = node.x, maxX = node.x, minY = node.y, maxY = node.y;
    for (const n of simNodes) {
      if (connected.has(n.key)) {
        minX = Math.min(minX, n.x);
        maxX = Math.max(maxX, n.x);
        minY = Math.min(minY, n.y);
        maxY = Math.max(maxY, n.y);
      }
    }
    const cx = (minX + maxX) / 2;
    const cy = (minY + maxY) / 2;
    const extent = Math.max(maxX - minX, maxY - minY, 4);
    const targetZoom = Math.min(4, 30 / extent);
    animatePanTo(cx, cy, targetZoom);
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
      if (e.labelSprite) scene.remove(e.labelSprite);
    }
    simNodes = [];
    simEdges = [];
    alpha = 1.0;

    const kinds = new Set<string>();

    for (const node of data.nodes) {
      const key = extractRecordKey(node.id);
      const color = KIND_COLORS[node.kind] ?? DEFAULT_COLOR;
      const r = nodeRadius(node.kind);

      // Main circle
      const geo = new THREE.CircleGeometry(r, 32);
      const mat = new THREE.MeshBasicMaterial({ color });
      const mesh = new THREE.Mesh(geo, mat);

      // Selection ring
      const ringGeo = new THREE.BufferGeometry().setFromPoints(
        Array.from({ length: 49 }, (_, i) => {
          const angle = (i / 48) * Math.PI * 2;
          return new THREE.Vector3(Math.cos(angle) * (r + 0.18), Math.sin(angle) * (r + 0.18), 0.05);
        }),
      );
      const ringMat = new THREE.LineBasicMaterial({ color, transparent: true, opacity: 0 });
      const ring = new THREE.LineLoop(ringGeo, ringMat);

      const label = node.label || node.node_id || node.kind;
      const displayLabel = label.length > 22 ? label.slice(0, 20) + "\u2026" : label;
      const sprite = makeTextSprite(displayLabel, LABEL_COLOR);

      mesh.position.set((Math.random() - 0.5) * 12, (Math.random() - 0.5) * 12, 0);
      ring.position.copy(mesh.position);
      sprite.position.set(mesh.position.x, mesh.position.y - r - 0.55, 0.1);

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
        pinned: false,
      });
    }

    for (const edge of data.edges) {
      const fromKey = extractRecordKey(edge.in);
      const toKey = extractRecordKey(edge.out);
      const geo = new THREE.BufferGeometry().setFromPoints([
        new THREE.Vector3(0, 0, -0.1),
        new THREE.Vector3(0, 0, -0.1),
      ]);
      const mat = new THREE.LineBasicMaterial({ color: EDGE_COLOR, transparent: true, opacity: 0.35 });
      const line = new THREE.Line(geo, mat);

      // Edge label (hidden by default)
      const labelSprite = edge.relation ? makeEdgeLabelSprite(edge.relation) : null;

      scene.add(line);
      if (labelSprite) scene.add(labelSprite);

      simEdges.push({ from: fromKey, to: toKey, relation: edge.relation, line, labelSprite });
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
    const hovEdge = hoveredEdge();
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
      n.labelSprite.position.set(n.x, n.y - r - 0.55, 0.1);

      const isSelected = sel && sel.key === n.key;
      const isHovered = hov && hov.key === n.key;
      const isConnected = sel && conn.has(n.key);
      const isDimmed = (sel && !isSelected && !isConnected) || isSearchFiltered;

      // Ring
      const ringMat = n.ring.material as THREE.LineBasicMaterial;
      if (isSelected) {
        ringMat.opacity = 0.9;
        ringMat.color.setHex(0xffffff);
      } else if (isHovered) {
        ringMat.opacity = 0.6;
        ringMat.color.setHex(KIND_COLORS[n.kind] ?? DEFAULT_COLOR);
      } else if (isConnected) {
        ringMat.opacity = 0.3;
        ringMat.color.setHex(KIND_COLORS[n.kind] ?? DEFAULT_COLOR);
      } else {
        ringMat.opacity = 0;
      }

      // Node opacity
      const meshMat = n.mesh.material as THREE.MeshBasicMaterial;
      meshMat.opacity = isDimmed ? 0.12 : 1;
      meshMat.transparent = isDimmed;

      // Label opacity
      const labelMat = n.labelSprite.material as THREE.SpriteMaterial;
      labelMat.opacity = isDimmed ? 0.08 : isSelected || isHovered ? 1 : 0.65;
    }

    for (const e of simEdges) {
      const a = nodeMap.get(e.from);
      const b = nodeMap.get(e.to);
      if (!a || !b) continue;
      const positions = (e.line.geometry as THREE.BufferGeometry).attributes.position;
      (positions as THREE.BufferAttribute).setXYZ(0, a.x, a.y, -0.1);
      (positions as THREE.BufferAttribute).setXYZ(1, b.x, b.y, -0.1);
      positions.needsUpdate = true;

      // Edge label position (midpoint)
      if (e.labelSprite) {
        e.labelSprite.position.set((a.x + b.x) / 2, (a.y + b.y) / 2, 0.2);
      }

      const lineMat = e.line.material as THREE.LineBasicMaterial;
      const isHiddenA = hidden.has(a.kind);
      const isHiddenB = hidden.has(b.kind);
      e.line.visible = !isHiddenA && !isHiddenB;
      if (e.labelSprite) e.labelSprite.visible = e.line.visible;

      const isThisEdgeHovered = hovEdge === e;

      if (sel) {
        const isEdgeConnected = e.from === sel.key || e.to === sel.key;
        lineMat.opacity = isEdgeConnected ? 0.7 : 0.06;
        lineMat.color.setHex(isEdgeConnected ? EDGE_HIGHLIGHT : EDGE_COLOR);
        // Show label on connected edges when node selected
        if (e.labelSprite) {
          (e.labelSprite.material as THREE.SpriteMaterial).opacity = isEdgeConnected ? 0.85 : 0;
        }
      } else if (isThisEdgeHovered) {
        lineMat.opacity = 0.7;
        lineMat.color.setHex(EDGE_HIGHLIGHT);
        if (e.labelSprite) {
          (e.labelSprite.material as THREE.SpriteMaterial).opacity = 0.9;
        }
      } else {
        lineMat.opacity = 0.35;
        lineMat.color.setHex(EDGE_COLOR);
        if (e.labelSprite) {
          (e.labelSprite.material as THREE.SpriteMaterial).opacity = 0;
        }
      }
    }
  };

  const animate = () => {
    if (!renderer || !scene || !camera) return;

    // Animated view transitions
    if (animatingView) {
      animViewProgress += 0.04;
      const t = Math.min(1, animViewProgress);
      const ease = t < 0.5 ? 4 * t * t * t : 1 - (-2 * t + 2) ** 3 / 2; // cubic ease-in-out
      panOffset.x = animViewStart.x + (animViewTarget.x - animViewStart.x) * ease;
      panOffset.y = animViewStart.y + (animViewTarget.y - animViewStart.y) * ease;
      zoom = animViewStart.zoom + (animViewTarget.zoom - animViewStart.zoom) * ease;
      updateCamera();
      if (t >= 1) animatingView = false;
    }

    if (alpha > 0.001) {
      simulateForces(simNodes, simEdges, alpha);
      alpha *= 0.995;
    }
    updatePositions();
    renderer.render(scene, camera);

    // Update minimap every ~10 frames
    if (minimapRef && camera && animFrame % 10 === 0) {
      drawMinimap(minimapRef, simNodes, camera, hiddenKinds());
    }

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

    // Wheel zoom
    containerRef.addEventListener("wheel", (e) => {
      e.preventDefault();
      const zoomFactor = e.deltaY > 0 ? 0.92 : 1.08;
      zoom *= zoomFactor;
      zoom = Math.max(0.15, Math.min(6, zoom));
      updateCamera();
    }, { passive: false });

    // Pointer down
    containerRef.addEventListener("pointerdown", (e) => {
      if (e.button === 2) return; // right-click handled separately
      setContextMenu(null);

      const hit = hitTest(e.clientX, e.clientY);

      // Start node drag if hitting a node
      if (hit && e.button === 0) {
        draggingNode = hit;
        hit.pinned = true;
        didDrag = false;
        panStart = { x: e.clientX, y: e.clientY };
        containerRef!.style.cursor = "grabbing";
        return;
      }

      isPanning = true;
      didDrag = false;
      panStart = { x: e.clientX, y: e.clientY };
      containerRef!.style.cursor = "grabbing";
    });

    // Context menu (right-click)
    containerRef.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      const hit = hitTest(e.clientX, e.clientY);
      if (hit) {
        const rect = containerRef!.getBoundingClientRect();
        setContextMenu({ x: e.clientX - rect.left, y: e.clientY - rect.top, node: hit });
        selectNode(hit);
      } else {
        setContextMenu(null);
      }
    });

    const onPointerMove = (e: PointerEvent) => {
      // Node dragging
      if (draggingNode && containerRef) {
        const dx = e.clientX - panStart.x;
        const dy = e.clientY - panStart.y;
        if (Math.abs(dx) > 3 || Math.abs(dy) > 3) didDrag = true;
        const scale = 40 / (zoom * containerRef.clientHeight);
        draggingNode.x += dx * scale;
        draggingNode.y -= dy * scale;
        panStart = { x: e.clientX, y: e.clientY };
        // Reheat simulation slightly so neighbors adjust
        alpha = Math.max(alpha, 0.05);
        return;
      }

      if (isPanning && containerRef) {
        const dx = e.clientX - panStart.x;
        const dy = e.clientY - panStart.y;
        if (Math.abs(dx) > 3 || Math.abs(dy) > 3) didDrag = true;
        const scale = 40 / (zoom * containerRef.clientHeight);
        panOffset.x -= dx * scale;
        panOffset.y += dy * scale;
        panStart = { x: e.clientX, y: e.clientY };
        updateCamera();
        return;
      }

      // Hover detection
      const nodeHit = hitTest(e.clientX, e.clientY);
      setHovered(nodeHit);

      // Edge hover (only when no node is hovered)
      if (!nodeHit) {
        const edgeHit = edgeHitTest(e.clientX, e.clientY);
        setHoveredEdge(edgeHit);
      } else {
        setHoveredEdge(null);
      }

      if (containerRef && !isPanning && !draggingNode) {
        containerRef.style.cursor = nodeHit ? "pointer" : "grab";
      }
    };

    const onPointerUp = (e: PointerEvent) => {
      // Node drag end
      if (draggingNode) {
        if (!didDrag) {
          // Was a click on a node, not a drag
          const now = Date.now();
          if (now - lastClickTime < 350) {
            // Double-click: zoom to neighborhood
            zoomToNeighborhood(draggingNode);
          } else {
            selectNode(draggingNode);
          }
          lastClickTime = now;
        }
        draggingNode.pinned = false;
        draggingNode = null;
        if (containerRef) containerRef.style.cursor = "grab";
        return;
      }

      const wasPanning = isPanning;
      isPanning = false;
      if (containerRef) containerRef.style.cursor = "grab";

      if (wasPanning && !didDrag) {
        const now = Date.now();
        const hit = hitTest(e.clientX, e.clientY);
        if (hit) {
          if (now - lastClickTime < 350) {
            zoomToNeighborhood(hit);
          } else {
            selectNode(hit);
          }
          lastClickTime = now;
        } else {
          selectNode(null);
          lastClickTime = now;
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

  const copyToClipboard = (text: string) => {
    void navigator.clipboard.writeText(text);
    setContextMenu(null);
  };

  const fitAll = () => {
    if (simNodes.length === 0) return;
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    for (const n of simNodes) {
      if (!n.mesh.visible) continue;
      minX = Math.min(minX, n.x);
      maxX = Math.max(maxX, n.x);
      minY = Math.min(minY, n.y);
      maxY = Math.max(maxY, n.y);
    }
    const cx = (minX + maxX) / 2;
    const cy = (minY + maxY) / 2;
    const extent = Math.max(maxX - minX, maxY - minY, 6);
    const targetZoom = Math.min(3, 35 / extent);
    animatePanTo(cx, cy, targetZoom);
  };

  return (
    <div class="kg-root" ref={containerRef}>
      {/* ── Toolbar ── */}
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
              <button type="button" class="kg-search-clear" onClick={() => setSearch("")}>
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
          <button type="button" class="kg-fit-btn" onClick={fitAll} title="Fit all nodes">
            <svg viewBox="0 0 24 24" class="h-3 w-3" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7" />
            </svg>
          </button>
          <span class="kg-node-count">{nodeCount()}</span>
        </div>
      </Show>

      {/* ── Hover tooltip ── */}
      <Show when={hovered() && !selected() && !draggingNode}>
        <div class="kg-tooltip">
          <div class="kg-tooltip-kind" style={{ color: KIND_CSS[hovered()!.kind] ?? DEFAULT_CSS }}>
            {hovered()!.kind}
          </div>
          <div class="kg-tooltip-label">{hovered()!.label}</div>
          <Show when={hovered()!.summary}>
            <div class="kg-tooltip-summary">{hovered()!.summary.slice(0, 140)}{hovered()!.summary.length > 140 ? "\u2026" : ""}</div>
          </Show>
          <div class="kg-tooltip-hint">Click to select \u00b7 Double-click to zoom \u00b7 Drag to move</div>
        </div>
      </Show>

      {/* ── Edge hover tooltip ── */}
      <Show when={hoveredEdge() && !hovered() && !selected()}>
        <div class="kg-tooltip kg-tooltip-edge">
          <div class="kg-tooltip-label">{hoveredEdge()!.relation || "connected"}</div>
        </div>
      </Show>

      {/* ── Context menu ── */}
      <Show when={contextMenu()}>
        <div
          class="kg-context-menu"
          style={{ left: `${contextMenu()!.x}px`, top: `${contextMenu()!.y}px` }}
        >
          <button type="button" class="kg-ctx-item" onClick={() => { zoomToNeighborhood(contextMenu()!.node); setContextMenu(null); }}>
            <svg viewBox="0 0 24 24" class="h-3 w-3" fill="none" stroke="currentColor" stroke-width="2">
              <circle cx="11" cy="11" r="8" /><line x1="21" y1="21" x2="16.65" y2="16.65" /><line x1="11" y1="8" x2="11" y2="14" /><line x1="8" y1="11" x2="14" y2="11" />
            </svg>
            Zoom to neighborhood
          </button>
          <button type="button" class="kg-ctx-item" onClick={() => copyToClipboard(contextMenu()!.node.nodeId)}>
            <svg viewBox="0 0 24 24" class="h-3 w-3" fill="none" stroke="currentColor" stroke-width="2">
              <rect x="9" y="9" width="13" height="13" rx="0" /><path d="M5 15H4a1 1 0 01-1-1V4a1 1 0 011-1h10a1 1 0 011 1v1" />
            </svg>
            Copy node ID
          </button>
          <Show when={contextMenu()!.node.summary}>
            <button type="button" class="kg-ctx-item" onClick={() => copyToClipboard(contextMenu()!.node.summary)}>
              <svg viewBox="0 0 24 24" class="h-3 w-3" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z" /><polyline points="14 2 14 8 20 8" />
              </svg>
              Copy summary
            </button>
          </Show>
          <div class="kg-ctx-sep" />
          <button type="button" class="kg-ctx-item" onClick={() => { selectNode(null); setContextMenu(null); }}>
            Deselect
          </button>
        </div>
      </Show>

      {/* ── Detail panel ── */}
      <Show when={selected()}>
        <div class="kg-detail">
          <div class="kg-detail-header">
            <div class="kg-detail-kind-badge" style={{ "--badge-color": KIND_CSS[selected()!.kind] ?? DEFAULT_CSS }}>
              {selected()!.kind}
            </div>
            <button type="button" class="kg-detail-close" onClick={() => selectNode(null)}>
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

          <Show when={getConnectedNodes(selected()!.key).length > 0}>
            <div class="kg-detail-connections">
              <div class="kg-detail-section-label">
                Connections ({getConnectedNodes(selected()!.key).length})
              </div>
              <div class="kg-detail-conn-list">
                <For each={getConnectedNodes(selected()!.key)}>
                  {(conn) => (
                    <button type="button" class="kg-detail-conn-item" onClick={() => navigateToNode(conn.node)}>
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

      {/* ── Minimap ── */}
      <Show when={!loading() && nodeCount() > 0}>
        <canvas
          ref={minimapRef}
          class="kg-minimap"
          width={140}
          height={100}
        />
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
