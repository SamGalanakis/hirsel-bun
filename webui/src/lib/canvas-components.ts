import { getKnowledgeGraph, getWorkspaceFile, type WorkspaceFile } from "@/lib/api";
import { renderCanvasMermaid } from "@/lib/canvas-mermaid";

const VALID_TONES = new Set([
  "default",
  "muted",
  "info",
  "success",
  "warning",
  "danger",
]);

function normalizeTone(value: string | null): string {
  const tone = value?.trim().toLowerCase() ?? "";
  return VALID_TONES.has(tone) ? tone : "default";
}

function escapeHtml(value: string | null): string {
  const text = value ?? "";
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function normalizeRootId(value: string | null | undefined): string {
  const rootId = value?.trim() ?? "";
  if (!rootId || rootId === "main") {
    return "main";
  }
  return rootId.startsWith("thread:") ? rootId : "main";
}

function rootLabel(rootId: string): string {
  return rootId.startsWith("thread:") ? `thread ${rootId.slice("thread:".length)}` : "main";
}

function readPositiveInt(value: string | null, fallback: number): number {
  const parsed = Number.parseInt(value ?? "", 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
}

function normalizeCodeText(value: string): string {
  const text = value.replace(/\r\n?/g, "\n").replace(/^\n+|\n+$/g, "");
  if (!text) return "";

  const lines = text.split("\n");
  const indents = lines
    .filter((line) => line.trim().length > 0)
    .map((line) => line.match(/^[ \t]*/)?.[0].length ?? 0);
  const commonIndent = indents.length > 0 ? Math.min(...indents) : 0;
  if (commonIndent === 0) return text;

  return lines
    .map((line) => {
      if (line.trim().length === 0) return "";
      return line.slice(commonIndent);
    })
    .join("\n");
}

function splitCodeLines(code: string): string[] {
  if (!code) return [];
  const lines = code.replace(/\r\n?/g, "\n").split("\n");
  if (lines.length > 1 && lines[lines.length - 1] === "") {
    lines.pop();
  }
  return lines;
}

function countCodeLines(code: string): number {
  return splitCodeLines(code).length;
}

function basename(path: string | null): string {
  const value = path?.trim() ?? "";
  if (!value) return "";
  return value.split("/").filter(Boolean).pop() ?? value;
}

function inferLanguage(value: string | null | undefined): string | null {
  const source = value?.trim().toLowerCase() ?? "";
  if (!source) return null;

  const extension = source.includes(".") ? source.split(".").pop() ?? source : source;
  const normalized = extension.replace(/[^a-z0-9#+-]/g, "");
  if (!normalized) return null;

  const aliases: Record<string, string> = {
    cjs: "js",
    coffeescript: "coffee",
    htm: "html",
    markdown: "md",
    mjs: "js",
    mts: "ts",
    pyi: "py",
    rs: "rust",
    sh: "shell",
    yml: "yaml",
  };

  return aliases[normalized] ?? normalized;
}

function formatLineRange(lineStart: number, lineEnd: number): string | null {
  if (lineStart <= 0 || lineEnd <= 0 || lineEnd < lineStart) return null;
  if (lineStart === lineEnd) return `L${lineStart}`;
  return `L${lineStart}-${lineEnd}`;
}

function buildCodeRows(code: string, lineStart: number): string {
  if (!code) {
    return '<div class="hirsel-code-empty">No code available.</div>';
  }

  const lines = splitCodeLines(code);

  return lines
    .map((line, index) => {
      const lineNumber = lineStart + index;
      const rendered = line.length > 0 ? escapeHtml(line) : "&#8203;";
      return `
        <div class="hirsel-code-row">
          <span class="hirsel-code-gutter">${lineNumber}</span>
          <code class="hirsel-code-line">${rendered}</code>
        </div>
      `;
    })
    .join("");
}

function buildCodeSkeleton(): string {
  return `
    <div class="hirsel-code-loading">
      <div class="skeleton h-3 w-2/5 rounded-none"></div>
      <div class="skeleton h-3 w-full rounded-none"></div>
      <div class="skeleton h-3 w-5/6 rounded-none"></div>
      <div class="skeleton h-3 w-4/6 rounded-none"></div>
      <div class="skeleton h-3 w-3/5 rounded-none"></div>
    </div>
  `;
}

type DiffRow =
  | {
      kind: "same";
      beforeNumber: number;
      afterNumber: number;
      text: string;
    }
  | {
      kind: "remove";
      beforeNumber: number;
      afterNumber: null;
      text: string;
    }
  | {
      kind: "add";
      beforeNumber: null;
      afterNumber: number;
      text: string;
    }
  | {
      kind: "skip";
      count: number;
    };

type DiffPayload = {
  rows: DiffRow[];
  additions: number;
  removals: number;
  unifiedText: string;
};

const DIFF_CONTEXT_LINES = 3;

function buildLineDiff(beforeCode: string, afterCode: string): DiffPayload {
  const beforeLines = splitCodeLines(beforeCode);
  const afterLines = splitCodeLines(afterCode);
  const table = Array.from({ length: beforeLines.length + 1 }, () =>
    Array<number>(afterLines.length + 1).fill(0),
  );

  for (let beforeIndex = beforeLines.length - 1; beforeIndex >= 0; beforeIndex -= 1) {
    for (let afterIndex = afterLines.length - 1; afterIndex >= 0; afterIndex -= 1) {
      if (beforeLines[beforeIndex] === afterLines[afterIndex]) {
        table[beforeIndex][afterIndex] = table[beforeIndex + 1][afterIndex + 1] + 1;
      } else {
        table[beforeIndex][afterIndex] = Math.max(
          table[beforeIndex + 1][afterIndex],
          table[beforeIndex][afterIndex + 1],
        );
      }
    }
  }

  const rows: DiffRow[] = [];
  const unified: string[] = [];
  let beforeIndex = 0;
  let afterIndex = 0;
  let beforeNumber = 1;
  let afterNumber = 1;
  let additions = 0;
  let removals = 0;

  while (beforeIndex < beforeLines.length && afterIndex < afterLines.length) {
    if (beforeLines[beforeIndex] === afterLines[afterIndex]) {
      rows.push({
        kind: "same",
        beforeNumber,
        afterNumber,
        text: beforeLines[beforeIndex],
      });
      unified.push(` ${beforeLines[beforeIndex]}`);
      beforeIndex += 1;
      afterIndex += 1;
      beforeNumber += 1;
      afterNumber += 1;
      continue;
    }

    if (table[beforeIndex + 1][afterIndex] >= table[beforeIndex][afterIndex + 1]) {
      rows.push({
        kind: "remove",
        beforeNumber,
        afterNumber: null,
        text: beforeLines[beforeIndex],
      });
      unified.push(`-${beforeLines[beforeIndex]}`);
      beforeIndex += 1;
      beforeNumber += 1;
      removals += 1;
      continue;
    }

    rows.push({
      kind: "add",
      beforeNumber: null,
      afterNumber,
      text: afterLines[afterIndex],
    });
    unified.push(`+${afterLines[afterIndex]}`);
    afterIndex += 1;
    afterNumber += 1;
    additions += 1;
  }

  while (beforeIndex < beforeLines.length) {
    rows.push({
      kind: "remove",
      beforeNumber,
      afterNumber: null,
      text: beforeLines[beforeIndex],
    });
    unified.push(`-${beforeLines[beforeIndex]}`);
    beforeIndex += 1;
    beforeNumber += 1;
    removals += 1;
  }

  while (afterIndex < afterLines.length) {
    rows.push({
      kind: "add",
      beforeNumber: null,
      afterNumber,
      text: afterLines[afterIndex],
    });
    unified.push(`+${afterLines[afterIndex]}`);
    afterIndex += 1;
    afterNumber += 1;
    additions += 1;
  }

  const compacted: DiffRow[] = [];
  let cursor = 0;
  while (cursor < rows.length) {
    if (rows[cursor]?.kind !== "same") {
      compacted.push(rows[cursor]!);
      cursor += 1;
      continue;
    }

    let end = cursor;
    while (end < rows.length && rows[end]?.kind === "same") {
      end += 1;
    }
    const run = rows.slice(cursor, end) as Array<Extract<DiffRow, { kind: "same" }>>;
    if (run.length <= DIFF_CONTEXT_LINES * 2 + 1) {
      compacted.push(...run);
    } else {
      compacted.push(...run.slice(0, DIFF_CONTEXT_LINES));
      compacted.push({
        kind: "skip",
        count: run.length - DIFF_CONTEXT_LINES * 2,
      });
      compacted.push(...run.slice(-DIFF_CONTEXT_LINES));
    }
    cursor = end;
  }

  return {
    rows: compacted,
    additions,
    removals,
    unifiedText: unified.join("\n"),
  };
}

function renderDiffLine(text: string): string {
  return text.length > 0 ? escapeHtml(text) : "&#8203;";
}

function buildDiffRows(rows: DiffRow[]): string {
  if (rows.length === 0) {
    return '<div class="hirsel-code-empty">Both code blocks are empty.</div>';
  }

  return rows
    .map((row) => {
      if (row.kind === "skip") {
        return `
          <div class="hirsel-diff-skip">
            <span>${row.count} unchanged line${row.count === 1 ? "" : "s"} hidden</span>
          </div>
        `;
      }

      const marker =
        row.kind === "add" ? "+" : row.kind === "remove" ? "-" : "·";
      return `
        <div class="hirsel-diff-row" data-kind="${row.kind}">
          <span class="hirsel-diff-number">${row.beforeNumber ?? ""}</span>
          <span class="hirsel-diff-number">${row.afterNumber ?? ""}</span>
          <span class="hirsel-diff-marker">${marker}</span>
          <code class="hirsel-diff-line">${renderDiffLine(row.text)}</code>
        </div>
      `;
    })
    .join("");
}

type DiffShellOptions = {
  title: string;
  caption?: string;
  badges: string[];
  beforeLabel: string;
  afterLabel: string;
  rows: DiffRow[];
  expanded: boolean;
  copied: boolean;
  copyEnabled: boolean;
};

function buildDiffShell(options: DiffShellOptions): string {
  const {
    title,
    caption,
    badges,
    beforeLabel,
    afterLabel,
    rows,
    expanded,
    copied,
    copyEnabled,
  } = options;

  return `
    <div class="hirsel-diff-shell" data-expanded="${expanded}">
      <div class="hirsel-code-header">
        <div class="hirsel-code-meta">
          <div class="hirsel-code-title">${escapeHtml(title)}</div>
          ${caption ? `<div class="hirsel-code-caption">${escapeHtml(caption)}</div>` : ""}
        </div>
        <div class="hirsel-code-controls">
          ${badges
            .map(
              (badge) =>
                `<span class="hirsel-code-badge">${escapeHtml(badge)}</span>`,
            )
            .join("")}
          <button
            type="button"
            class="hirsel-code-action"
            data-action="copy"
            ${copyEnabled ? "" : "disabled"}
          >
            ${copied ? "Copied" : "Copy diff"}
          </button>
          <button
            type="button"
            class="hirsel-code-action"
            data-action="toggle"
            aria-expanded="${expanded}"
          >
            ${expanded ? "Hide" : "Show"}
          </button>
        </div>
      </div>
      ${
        expanded
          ? `
            <div class="hirsel-diff-body">
              <div class="hirsel-diff-legend">
                <span class="hirsel-diff-origin" data-kind="remove">${escapeHtml(beforeLabel)}</span>
                <span class="hirsel-diff-origin-arrow" aria-hidden="true">→</span>
                <span class="hirsel-diff-origin" data-kind="add">${escapeHtml(afterLabel)}</span>
              </div>
              <div class="hirsel-diff-scroll">
                <div class="hirsel-diff-rows">
                  ${buildDiffRows(rows)}
                </div>
              </div>
            </div>
          `
          : ""
      }
    </div>
  `;
}

type CodeShellOptions = {
  title: string;
  caption?: string;
  badges: string[];
  code: string;
  lineStart: number;
  expanded: boolean;
  copied: boolean;
  loading?: boolean;
  error?: string;
  emptyLabel?: string;
};

function buildCodeShell(options: CodeShellOptions): string {
  const {
    title,
    caption,
    badges,
    code,
    lineStart,
    expanded,
    copied,
    loading = false,
    error = "",
    emptyLabel = "No code available.",
  } = options;

  const hasCode = code.trim().length > 0;
  const body = !expanded
    ? ""
    : loading
      ? buildCodeSkeleton()
      : error
        ? `<div class="hirsel-code-empty" data-tone="danger">${escapeHtml(error)}</div>`
        : hasCode
          ? `
            <div class="hirsel-code-body">
              <div class="hirsel-code-scroll">
                <div class="hirsel-code-rows">
                  ${buildCodeRows(code, lineStart)}
                </div>
              </div>
            </div>
          `
          : `<div class="hirsel-code-empty">${escapeHtml(emptyLabel)}</div>`;

  return `
    <div
      class="hirsel-code-shell"
      data-expanded="${expanded}"
      data-state="${loading ? "loading" : error ? "error" : "ready"}"
    >
      <div class="hirsel-code-header">
        <div class="hirsel-code-meta">
          <div class="hirsel-code-title">${escapeHtml(title)}</div>
          ${caption ? `<div class="hirsel-code-caption">${escapeHtml(caption)}</div>` : ""}
        </div>
        <div class="hirsel-code-controls">
          ${badges
            .map(
              (badge) =>
                `<span class="hirsel-code-badge">${escapeHtml(badge)}</span>`,
            )
            .join("")}
          <button
            type="button"
            class="hirsel-code-action"
            data-action="copy"
            ${hasCode ? "" : "disabled"}
          >
            ${copied ? "Copied" : "Copy"}
          </button>
          <button
            type="button"
            class="hirsel-code-action"
            data-action="toggle"
            aria-expanded="${expanded}"
          >
            ${expanded ? "Hide" : "Show"}
          </button>
        </div>
      </div>
      ${body}
    </div>
  `;
}

function normalizeFileStatus(value: string | null | undefined): string {
  const status = value?.trim().toLowerCase() ?? "";
  if (!status) return "default";
  if (["new", "added", "create", "created"].includes(status)) return "new";
  if (["modified", "changed", "updated", "edited"].includes(status)) return "modified";
  if (["deleted", "removed"].includes(status)) return "deleted";
  if (["renamed", "moved"].includes(status)) return "moved";
  return "default";
}

function formatFileStatus(status: string): string {
  switch (status) {
    case "new":
      return "New";
    case "modified":
      return "Modified";
    case "deleted":
      return "Deleted";
    case "moved":
      return "Moved";
    default:
      return "File";
  }
}

function currentCanvasProjectId(element: Element): number | null {
  const explicit = readPositiveInt(element.getAttribute("project-id"), 0);
  if (explicit > 0) return explicit;

  const root = element.closest<HTMLElement>("[data-canvas-project-id]");
  const parsed = Number.parseInt(root?.dataset.canvasProjectId ?? "", 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
}

type CachedGraphNode = {
  kind: string;
  node_id: string;
  label: string;
  summary: string;
  [key: string]: unknown;
};

const knowledgeGraphCache = new Map<number, Promise<Map<string, CachedGraphNode>>>();

function nodeKey(kind: string, nodeId: string): string {
  return `${kind}:${nodeId}`;
}

async function loadGraphNodeMap(projectId: number): Promise<Map<string, CachedGraphNode>> {
  const cached = knowledgeGraphCache.get(projectId);
  if (cached) return cached;
  const pending = getKnowledgeGraph(projectId).then((graph) => {
    const map = new Map<string, CachedGraphNode>();
    for (const node of graph.nodes) {
      map.set(nodeKey(node.kind, node.node_id), node as CachedGraphNode);
    }
    return map;
  });
  knowledgeGraphCache.set(projectId, pending);
  return pending;
}

function parseNodeAttr(value: string | null): { kind: string; nodeId: string } | null {
  const raw = value?.trim() ?? "";
  const index = raw.indexOf(":");
  if (index <= 0 || index === raw.length - 1) return null;
  return {
    kind: raw.slice(0, index).trim(),
    nodeId: raw.slice(index + 1).trim(),
  };
}

function renderNodeError(message: string): string {
  return `<div class="hirsel-code-empty" data-tone="danger">${escapeHtml(message)}</div>`;
}

function requestedLineRange(element: Element): { lineStart: number; lineEnd?: number } {
  const lineStart = readPositiveInt(element.getAttribute("line-start"), 1);
  const rawLineEnd = Number.parseInt(element.getAttribute("line-end") ?? "", 10);
  if (Number.isFinite(rawLineEnd) && rawLineEnd >= lineStart) {
    return { lineStart, lineEnd: rawLineEnd };
  }
  return { lineStart };
}

function buildWorkspaceRequestKey(data: {
  lineStart: number;
  lineEnd?: number;
  path: string;
  projectId: number;
  rootId: string;
}): string {
  return JSON.stringify({
    lineEnd: data.lineEnd ?? null,
    lineStart: data.lineStart,
    path: data.path,
    projectId: data.projectId,
    rootId: data.rootId,
  });
}

let nextTabsId = 0;

abstract class HirselCodeFrameElement extends HTMLElement {
  protected expanded = true;
  protected copied = false;
  private copyResetTimer: number | undefined;
  private listening = false;

  protected bindFrameInteractions(): void {
    if (this.listening) return;
    this.addEventListener("click", this.handleClick);
    this.listening = true;
  }

  disconnectedCallback(): void {
    if (this.listening) {
      this.removeEventListener("click", this.handleClick);
      this.listening = false;
    }
    if (this.copyResetTimer !== undefined) {
      window.clearTimeout(this.copyResetTimer);
      this.copyResetTimer = undefined;
    }
    this.cleanupFrame();
  }

  protected cleanupFrame(): void {}

  private handleClick = (event: Event): void => {
    const target = event.target as HTMLElement | null;
    const button = target?.closest<HTMLButtonElement>("button[data-action]");
    if (!button) return;

    const action = button.dataset.action;
    if (action === "toggle") {
      event.preventDefault();
      this.expanded = !this.expanded;
      this.renderFrame();
      return;
    }

    if (action === "copy") {
      event.preventDefault();
      void this.copyFrameContents();
    }
  };

  private async copyFrameContents(): Promise<void> {
    const payload = this.copyPayload();
    if (!payload) return;

    try {
      await navigator.clipboard.writeText(payload);
      this.copied = true;
      this.renderFrame();
      if (this.copyResetTimer !== undefined) {
        window.clearTimeout(this.copyResetTimer);
      }
      this.copyResetTimer = window.setTimeout(() => {
        this.copied = false;
        this.renderFrame();
      }, 1400);
    } catch {
      this.copied = false;
      this.renderFrame();
    }
  }

  protected abstract copyPayload(): string | null;
  protected abstract renderFrame(): void;
}

class HirselCardElement extends HTMLElement {
  private bodyHtml = "";
  private initialized = false;

  static get observedAttributes(): string[] {
    return ["tone", "eyebrow", "heading"];
  }

  connectedCallback(): void {
    this.render();
  }

  attributeChangedCallback(): void {
    if (this.initialized) this.render();
  }

  private captureBody(): void {
    if (this.initialized) return;
    this.bodyHtml = this.innerHTML.trim();
    this.initialized = true;
  }

  private render(): void {
    this.captureBody();
    const tone = normalizeTone(this.getAttribute("tone"));
    const eyebrow = this.getAttribute("eyebrow");
    const heading = this.getAttribute("heading");
    const header = eyebrow || heading
      ? `
        <div class="hirsel-card-header">
          ${eyebrow ? `<div class="hirsel-card-eyebrow">${escapeHtml(eyebrow)}</div>` : ""}
          ${heading ? `<div class="hirsel-card-heading">${escapeHtml(heading)}</div>` : ""}
        </div>
      `
      : "";

    this.dataset.hirselReady = "true";
    this.innerHTML = `
      <div class="hirsel-card-shell" data-tone="${tone}">
        ${header}
        <div class="hirsel-card-body">${this.bodyHtml}</div>
      </div>
    `;
  }
}

class HirselCalloutElement extends HTMLElement {
  private bodyHtml = "";
  private initialized = false;

  static get observedAttributes(): string[] {
    return ["tone", "title"];
  }

  connectedCallback(): void {
    this.render();
  }

  attributeChangedCallback(): void {
    if (this.initialized) this.render();
  }

  private captureBody(): void {
    if (this.initialized) return;
    this.bodyHtml = this.innerHTML.trim();
    this.initialized = true;
  }

  private render(): void {
    this.captureBody();
    const tone = normalizeTone(this.getAttribute("tone"));
    const title = this.getAttribute("title");

    this.dataset.hirselReady = "true";
    this.innerHTML = `
      <div class="hirsel-callout-shell" data-tone="${tone}">
        ${title ? `<div class="hirsel-callout-title">${escapeHtml(title)}</div>` : ""}
        <div class="hirsel-callout-body">${this.bodyHtml}</div>
      </div>
    `;
  }
}

class HirselStatGridElement extends HTMLElement {
  static get observedAttributes(): string[] {
    return ["min"];
  }

  connectedCallback(): void {
    this.applyAttributes();
  }

  attributeChangedCallback(): void {
    this.applyAttributes();
  }

  private applyAttributes(): void {
    const min = Number(this.getAttribute("min"));
    if (Number.isFinite(min) && min > 0) {
      this.style.setProperty("--hirsel-stat-min", `${min}px`);
    } else {
      this.style.removeProperty("--hirsel-stat-min");
    }
    this.dataset.hirselReady = "true";
  }
}

class HirselStatElement extends HTMLElement {
  private bodyHtml = "";
  private initialized = false;

  static get observedAttributes(): string[] {
    return ["tone", "label", "value", "detail"];
  }

  connectedCallback(): void {
    this.render();
  }

  attributeChangedCallback(): void {
    if (this.initialized) this.render();
  }

  private captureBody(): void {
    if (this.initialized) return;
    this.bodyHtml = this.innerHTML.trim();
    this.initialized = true;
  }

  private render(): void {
    this.captureBody();
    const tone = normalizeTone(this.getAttribute("tone"));
    const label = this.getAttribute("label");
    const value = this.getAttribute("value");
    const detail = this.getAttribute("detail");
    const footer = detail || this.bodyHtml
      ? `<div class="hirsel-stat-detail">${detail ? escapeHtml(detail) : this.bodyHtml}</div>`
      : "";

    this.dataset.hirselReady = "true";
    this.innerHTML = `
      <div class="hirsel-stat-shell" data-tone="${tone}">
        ${label ? `<div class="hirsel-stat-label">${escapeHtml(label)}</div>` : ""}
        <div class="hirsel-stat-value">${escapeHtml(value)}</div>
        ${footer}
      </div>
    `;
  }
}

type TabsPanel = {
  label: string;
  body: string;
  value: string;
};

class HirselTabsElement extends HTMLElement {
  private panels: TabsPanel[] = [];
  private activeIndex = 0;
  private initialized = false;
  private listening = false;
  private readonly tabsId = `hirsel-tabs-${++nextTabsId}`;

  static get observedAttributes(): string[] {
    return ["active", "tone"];
  }

  connectedCallback(): void {
    if (!this.initialized) {
      this.capturePanels();
      this.initialized = true;
    }
    if (!this.listening) {
      this.addEventListener("click", this.handleClick);
      this.addEventListener("keydown", this.handleKeyDown);
      this.listening = true;
    }
    this.syncActiveFromAttribute();
    this.render();
  }

  disconnectedCallback(): void {
    this.removeEventListener("click", this.handleClick);
    this.removeEventListener("keydown", this.handleKeyDown);
    this.listening = false;
  }

  attributeChangedCallback(): void {
    if (!this.initialized) return;
    this.syncActiveFromAttribute();
    this.render();
  }

  private capturePanels(): void {
    const sourcePanels = Array.from(this.children);
    this.panels = sourcePanels
      .map((element, index) => {
        const label =
          element.getAttribute("label") ||
          element.getAttribute("data-label") ||
          element.getAttribute("title") ||
          `Section ${index + 1}`;
        const value =
          element.getAttribute("value") ||
          label.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") ||
          `tab-${index + 1}`;
        return {
          label,
          value,
          body: element.innerHTML.trim(),
        };
      })
      .filter((panel) => panel.label);
  }

  private syncActiveFromAttribute(): void {
    if (this.panels.length === 0) return;
    const active = this.getAttribute("active");
    if (!active) {
      this.activeIndex = clamp(this.activeIndex, 0, this.panels.length - 1);
      return;
    }

    const numeric = Number.parseInt(active, 10);
    if (Number.isFinite(numeric)) {
      this.activeIndex = clamp(numeric, 0, this.panels.length - 1);
      return;
    }

    const byValue = this.panels.findIndex((panel) => panel.value === active);
    if (byValue >= 0) {
      this.activeIndex = byValue;
      return;
    }

    const byLabel = this.panels.findIndex(
      (panel) => panel.label.toLowerCase() === active.toLowerCase(),
    );
    if (byLabel >= 0) {
      this.activeIndex = byLabel;
    }
  }

  private render(): void {
    if (this.panels.length === 0) {
      this.dataset.hirselReady = "true";
      return;
    }

    const tone = normalizeTone(this.getAttribute("tone"));
    const buttons = this.panels
      .map((panel, index) => {
        const selected = index === this.activeIndex;
        return `
          <button
            type="button"
            class="hirsel-tab-button"
            data-index="${index}"
            id="${this.tabsId}-tab-${index}"
            role="tab"
            aria-selected="${selected}"
            aria-controls="${this.tabsId}-panel-${index}"
            tabindex="${selected ? "0" : "-1"}"
          >
            ${escapeHtml(panel.label)}
          </button>
        `;
      })
      .join("");

    const panels = this.panels
      .map((panel, index) => {
        const selected = index === this.activeIndex;
        return `
          <section
            class="hirsel-tabs-panel"
            id="${this.tabsId}-panel-${index}"
            role="tabpanel"
            aria-labelledby="${this.tabsId}-tab-${index}"
            ${selected ? "" : "hidden"}
          >
            ${panel.body}
          </section>
        `;
      })
      .join("");

    this.dataset.hirselReady = "true";
    this.innerHTML = `
      <div class="hirsel-tabs-shell" data-tone="${tone}">
        <div class="hirsel-tabs-nav" role="tablist" aria-label="Canvas sections">
          ${buttons}
        </div>
        <div class="hirsel-tabs-body">
          ${panels}
        </div>
      </div>
    `;
  }

  private handleClick = (event: Event): void => {
    const target = event.target as HTMLElement | null;
    const button = target?.closest<HTMLButtonElement>(".hirsel-tab-button[data-index]");
    if (!button) return;
    const index = Number.parseInt(button.dataset.index ?? "", 10);
    if (!Number.isFinite(index)) return;
    this.activeIndex = clamp(index, 0, this.panels.length - 1);
    this.render();
  };

  private handleKeyDown = (event: Event): void => {
    const keyboardEvent = event as KeyboardEvent;
    if (!["ArrowRight", "ArrowLeft", "Home", "End"].includes(keyboardEvent.key)) {
      return;
    }

    keyboardEvent.preventDefault();
    if (keyboardEvent.key === "Home") {
      this.activeIndex = 0;
    } else if (keyboardEvent.key === "End") {
      this.activeIndex = this.panels.length - 1;
    } else if (keyboardEvent.key === "ArrowRight") {
      this.activeIndex = (this.activeIndex + 1) % this.panels.length;
    } else if (keyboardEvent.key === "ArrowLeft") {
      this.activeIndex = (this.activeIndex - 1 + this.panels.length) % this.panels.length;
    }

    this.render();
    queueMicrotask(() => {
      const activeButton = this.querySelector<HTMLButtonElement>(
        `.hirsel-tab-button[data-index="${this.activeIndex}"]`,
      );
      activeButton?.focus();
    });
  };
}

class HirselDisclosureElement extends HTMLElement {
  private bodyHtml = "";
  private initialized = false;

  static get observedAttributes(): string[] {
    return ["title", "tone", "open"];
  }

  connectedCallback(): void {
    this.render();
  }

  attributeChangedCallback(): void {
    if (this.initialized) this.render();
  }

  private captureBody(): void {
    if (this.initialized) return;
    this.bodyHtml = this.innerHTML.trim();
    this.initialized = true;
  }

  private render(): void {
    this.captureBody();
    const tone = normalizeTone(this.getAttribute("tone"));
    const title = this.getAttribute("title") || "Details";
    const open = this.hasAttribute("open") ? "open" : "";

    this.dataset.hirselReady = "true";
    this.innerHTML = `
      <details class="hirsel-disclosure-shell" data-tone="${tone}" ${open}>
        <summary class="hirsel-disclosure-summary">
          <span class="hirsel-disclosure-title">${escapeHtml(title)}</span>
          <span class="hirsel-disclosure-chevron" aria-hidden="true">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <polyline points="6 9 12 15 18 9" />
            </svg>
          </span>
        </summary>
        <div class="hirsel-disclosure-body">${this.bodyHtml}</div>
      </details>
    `;
  }
}

class HirselProgressElement extends HTMLElement {
  private bodyHtml = "";
  private initialized = false;

  static get observedAttributes(): string[] {
    return ["tone", "label", "detail", "value", "max"];
  }

  connectedCallback(): void {
    this.render();
  }

  attributeChangedCallback(): void {
    if (this.initialized) this.render();
  }

  private captureBody(): void {
    if (this.initialized) return;
    this.bodyHtml = this.innerHTML.trim();
    this.initialized = true;
  }

  private render(): void {
    this.captureBody();
    const tone = normalizeTone(this.getAttribute("tone"));
    const label = this.getAttribute("label");
    const detail = this.getAttribute("detail");
    const rawValue = Number.parseFloat(this.getAttribute("value") ?? "0");
    const rawMax = Number.parseFloat(this.getAttribute("max") ?? "100");
    const max = Number.isFinite(rawMax) && rawMax > 0 ? rawMax : 100;
    const value = Number.isFinite(rawValue) ? clamp(rawValue, 0, max) : 0;
    const percent = max === 0 ? 0 : (value / max) * 100;
    const footer = this.bodyHtml
      ? `<div class="hirsel-progress-footer">${this.bodyHtml}</div>`
      : "";

    this.dataset.hirselReady = "true";
    this.innerHTML = `
      <div class="hirsel-progress-shell" data-tone="${tone}">
        ${(label || detail) ? `
          <div class="hirsel-progress-meta">
            ${label ? `<span class="hirsel-progress-label">${escapeHtml(label)}</span>` : ""}
            ${detail ? `<span class="hirsel-progress-detail">${escapeHtml(detail)}</span>` : ""}
          </div>
        ` : ""}
        <div class="hirsel-progress-track">
          <div class="hirsel-progress-fill" style="width:${percent}%"></div>
        </div>
        ${footer}
      </div>
    `;
  }
}

class HirselCodeElement extends HirselCodeFrameElement {
  private codeText = "";
  private initialized = false;

  static get observedAttributes(): string[] {
    return ["collapsed", "filename", "language", "line-start", "title"];
  }

  connectedCallback(): void {
    if (!this.initialized) {
      this.codeText = normalizeCodeText(this.textContent ?? "");
      this.expanded = !this.hasAttribute("collapsed");
      this.initialized = true;
    }
    this.bindFrameInteractions();
    this.renderFrame();
  }

  attributeChangedCallback(name: string): void {
    if (!this.initialized) return;
    if (name === "collapsed") {
      this.expanded = !this.hasAttribute("collapsed");
    }
    this.renderFrame();
  }

  protected copyPayload(): string | null {
    return this.codeText || null;
  }

  protected renderFrame(): void {
    const titleAttr = this.getAttribute("title")?.trim() ?? "";
    const filename = this.getAttribute("filename")?.trim() ?? "";
    const title = titleAttr || filename || "Code";
    const caption = titleAttr && filename && titleAttr !== filename ? filename : "";
    const language =
      inferLanguage(this.getAttribute("language")) || inferLanguage(filename) || null;
    const lineStart = readPositiveInt(this.getAttribute("line-start"), 1);
    const lineCount = countCodeLines(this.codeText);
    const lineEnd = lineCount > 0 ? lineStart + lineCount - 1 : 0;
    const badges = [
      language,
      formatLineRange(lineStart, lineEnd),
    ].filter((value): value is string => Boolean(value));

    this.dataset.hirselReady = "true";
    this.innerHTML = buildCodeShell({
      title,
      caption,
      badges,
      code: this.codeText,
      lineStart,
      expanded: this.expanded,
      copied: this.copied,
      emptyLabel: "No inline code was provided.",
    });
  }
}

class HirselCodeRefElement extends HirselCodeFrameElement {
  private initialized = false;
  private loading = false;
  private error = "";
  private snippet: WorkspaceFile | null = null;
  private requestKey = "";
  private abortController: AbortController | null = null;

  static get observedAttributes(): string[] {
    return [
      "collapsed",
      "filename",
      "language",
      "line-end",
      "line-start",
      "path",
      "root-id",
      "title",
    ];
  }

  connectedCallback(): void {
    if (!this.initialized) {
      this.expanded = !this.hasAttribute("collapsed");
      this.initialized = true;
    }
    this.bindFrameInteractions();
    void this.loadSnippet();
    this.renderFrame();
  }

  attributeChangedCallback(name: string): void {
    if (!this.initialized) return;
    if (name === "collapsed") {
      this.expanded = !this.hasAttribute("collapsed");
      this.renderFrame();
      return;
    }

    if (name === "title" || name === "filename" || name === "language") {
      this.renderFrame();
      return;
    }

    void this.loadSnippet();
  }

  protected cleanupFrame(): void {
    this.abortController?.abort();
  }

  protected copyPayload(): string | null {
    return this.snippet?.content || null;
  }

  protected renderFrame(): void {
    const rootId = normalizeRootId(this.getAttribute("root-id"));
    const requestedPath = (this.getAttribute("path") ?? "").trim();
    const resolvedPath = this.snippet?.path || requestedPath;
    const titleAttr = this.getAttribute("title")?.trim() ?? "";
    const filenameAttr = this.getAttribute("filename")?.trim() ?? "";
    const title = titleAttr || filenameAttr || basename(resolvedPath) || "Code Reference";
    const captionParts = [resolvedPath || filenameAttr];
    captionParts.push(rootLabel(rootId));
    const caption = captionParts.filter(Boolean).join(" · ");
    const language =
      inferLanguage(this.getAttribute("language")) ||
      inferLanguage(filenameAttr) ||
      inferLanguage(resolvedPath) ||
      null;
    const requestedStart = readPositiveInt(this.getAttribute("line-start"), 1);
    const requestedEnd = readPositiveInt(this.getAttribute("line-end"), requestedStart);
    const actualRange =
      this.snippet && this.snippet.lineEnd >= this.snippet.lineStart
        ? formatLineRange(this.snippet.lineStart, this.snippet.lineEnd)
        : null;
    const requestedRange = formatLineRange(requestedStart, requestedEnd);
    const badges = [
      language,
      actualRange || requestedRange,
      this.snippet?.truncated ? "truncated" : null,
    ].filter((value): value is string => Boolean(value));
    const lineStart = this.snippet?.lineStart ?? requestedStart;

    this.dataset.hirselReady = "true";
    this.innerHTML = buildCodeShell({
      title,
      caption,
      badges,
      code: this.snippet?.content ?? "",
      lineStart,
      expanded: this.expanded,
      copied: this.copied,
      loading: this.loading,
      error: this.error,
      emptyLabel: requestedPath
        ? "No code was returned for this range."
        : "Set a path attribute to load a workspace file.",
    });
  }

  private currentProjectId(): number | null {
    return currentCanvasProjectId(this);
  }

  private requestedRange(): { lineStart: number; lineEnd?: number } {
    return requestedLineRange(this);
  }

  private async loadSnippet(): Promise<void> {
    const projectId = this.currentProjectId();
    const path = (this.getAttribute("path") ?? "").trim();
    const rootId = normalizeRootId(this.getAttribute("root-id"));
    const { lineStart, lineEnd } = this.requestedRange();

    if (!path) {
      this.abortController?.abort();
      this.requestKey = "";
      this.loading = false;
      this.error = "";
      this.snippet = null;
      this.renderFrame();
      return;
    }

    if (!projectId) {
      this.abortController?.abort();
      this.requestKey = "";
      this.loading = false;
      this.error = "Canvas project context is missing.";
      this.snippet = null;
      this.renderFrame();
      return;
    }

    const requestKey = buildWorkspaceRequestKey({
      lineEnd,
      lineStart,
      path,
      projectId,
      rootId,
    });
    if (requestKey === this.requestKey && (this.loading || this.snippet || this.error)) {
      return;
    }

    this.requestKey = requestKey;
    this.abortController?.abort();
    const controller = new AbortController();
    this.abortController = controller;
    this.loading = true;
    this.error = "";
    this.snippet = null;
    this.renderFrame();

    try {
      const snippet = await getWorkspaceFile(projectId, {
        path,
        rootId,
        lineEnd,
        lineStart,
        signal: controller.signal,
      });
      if (controller.signal.aborted) return;
      this.loading = false;
      this.error = "";
      this.snippet = snippet;
      this.copied = false;
      this.renderFrame();
    } catch (error) {
      if (controller.signal.aborted) return;
      this.loading = false;
      this.snippet = null;
      this.error =
        error instanceof Error ? error.message : "Failed to load workspace file.";
      this.copied = false;
      this.renderFrame();
    }
  }
}

class HirselFileRefElement extends HTMLElement {
  private initialized = false;
  private listening = false;
  private labelText = "";
  private expanded = false;
  private loading = false;
  private error = "";
  private snippet: WorkspaceFile | null = null;
  private requestKey = "";
  private abortController: AbortController | null = null;

  static get observedAttributes(): string[] {
    return [
      "label",
      "line-end",
      "line-start",
      "open",
      "path",
      "root-id",
      "status",
      "title",
    ];
  }

  connectedCallback(): void {
    if (!this.initialized) {
      this.labelText = (this.textContent ?? "").trim();
      this.expanded = this.hasAttribute("open");
      this.initialized = true;
    }
    if (!this.listening) {
      this.addEventListener("click", this.handleClick);
      this.listening = true;
    }
    if (this.expanded) {
      void this.loadSnippet();
    }
    this.render();
  }

  disconnectedCallback(): void {
    if (this.listening) {
      this.removeEventListener("click", this.handleClick);
      this.listening = false;
    }
    this.abortController?.abort();
  }

  attributeChangedCallback(name: string): void {
    if (!this.initialized) return;
    if (name === "open") {
      this.expanded = this.hasAttribute("open");
      if (this.expanded) {
        void this.loadSnippet();
      }
      this.render();
      return;
    }

    if (name === "label" || name === "status" || name === "title") {
      this.render();
      return;
    }

    if (this.expanded) {
      void this.loadSnippet();
    } else {
      this.loading = false;
      this.error = "";
      this.snippet = null;
      this.requestKey = "";
      this.abortController?.abort();
    }
    this.render();
  }

  private handleClick = (event: Event): void => {
    const target = event.target as HTMLElement | null;
    const button = target?.closest<HTMLButtonElement>("button[data-action='toggle']");
    if (!button) return;
    event.preventDefault();
    this.expanded = !this.expanded;
    if (this.expanded) {
      void this.loadSnippet();
    }
    this.render();
  };

  private render(): void {
    const path = (this.getAttribute("path") ?? "").trim();
    const rootId = normalizeRootId(this.getAttribute("root-id"));
    const status = normalizeFileStatus(this.getAttribute("status"));
    const requestedStart = readPositiveInt(this.getAttribute("line-start"), 1);
    const requestedEnd = Number.parseInt(this.getAttribute("line-end") ?? "", 10);
    const hasExplicitRange =
      this.hasAttribute("line-start") || this.hasAttribute("line-end") || !!this.snippet;
    const lineRange = !hasExplicitRange
      ? null
      : Number.isFinite(requestedEnd) && requestedEnd >= requestedStart
        ? formatLineRange(requestedStart, requestedEnd)
        : this.snippet
          ? formatLineRange(this.snippet.lineStart, this.snippet.lineEnd)
          : formatLineRange(requestedStart, requestedStart);
    const label =
      this.getAttribute("label")?.trim() ||
      this.labelText ||
      this.getAttribute("title")?.trim() ||
      basename(path) ||
      "File";
    const workspaceText = rootLabel(rootId);
    const preview = !this.expanded
      ? ""
      : `
        <div class="hirsel-fileref-preview">
          <div class="hirsel-fileref-preview-meta">
            <span>${escapeHtml(path || label)}</span>
            <span>${escapeHtml(workspaceText)}</span>
            ${lineRange ? `<span>${escapeHtml(lineRange)}</span>` : ""}
          </div>
          ${
            this.loading
              ? buildCodeSkeleton()
              : this.error
                ? `<div class="hirsel-code-empty" data-tone="danger">${escapeHtml(this.error)}</div>`
                : this.snippet?.content
                  ? `
                    <div class="hirsel-code-body">
                      <div class="hirsel-code-scroll">
                        <div class="hirsel-code-rows">
                          ${buildCodeRows(this.snippet.content, this.snippet.lineStart)}
                        </div>
                      </div>
                    </div>
                  `
                  : `<div class="hirsel-code-empty">Preview unavailable.</div>`
          }
        </div>
      `;

    this.dataset.hirselReady = "true";
    this.innerHTML = `
      <span class="hirsel-fileref-shell" data-open="${this.expanded}">
        <button type="button" class="hirsel-fileref-pill" data-action="toggle" data-status="${status}">
          <span class="hirsel-file-status" data-status="${status}">${escapeHtml(formatFileStatus(status))}</span>
          <span class="hirsel-fileref-label">${escapeHtml(label)}</span>
          ${path && label !== path ? `<span class="hirsel-fileref-path">${escapeHtml(path)}</span>` : ""}
          ${lineRange ? `<span class="hirsel-fileref-range">${escapeHtml(lineRange)}</span>` : ""}
        </button>
        ${preview}
      </span>
    `;
  }

  private async loadSnippet(): Promise<void> {
    const projectId = currentCanvasProjectId(this);
    const path = (this.getAttribute("path") ?? "").trim();
    const rootId = normalizeRootId(this.getAttribute("root-id"));
    const { lineStart, lineEnd } = requestedLineRange(this);

    if (!path) {
      this.loading = false;
      this.error = "Path is required.";
      this.snippet = null;
      this.requestKey = "";
      this.abortController?.abort();
      this.render();
      return;
    }

    if (!projectId) {
      this.loading = false;
      this.error = "Canvas project context is missing.";
      this.snippet = null;
      this.requestKey = "";
      this.abortController?.abort();
      this.render();
      return;
    }

    const requestKey = buildWorkspaceRequestKey({
      lineEnd,
      lineStart,
      path,
      projectId,
      rootId,
    });
    if (requestKey === this.requestKey && (this.loading || this.snippet || this.error)) {
      return;
    }

    this.requestKey = requestKey;
    this.abortController?.abort();
    const controller = new AbortController();
    this.abortController = controller;
    this.loading = true;
    this.error = "";
    this.snippet = null;
    this.render();

    try {
      const snippet = await getWorkspaceFile(projectId, {
        path,
        rootId,
        lineEnd,
        lineStart,
        signal: controller.signal,
      });
      if (controller.signal.aborted) return;
      this.loading = false;
      this.error = "";
      this.snippet = snippet;
      this.render();
    } catch (error) {
      if (controller.signal.aborted) return;
      this.loading = false;
      this.snippet = null;
      this.error =
        error instanceof Error ? error.message : "Failed to load workspace file.";
      this.render();
    }
  }
}

type FileListItem = {
  detailHtml: string;
  label: string;
  lineEnd: string;
  lineStart: string;
  open: boolean;
  path: string;
  rootId: string;
  status: string;
};

class HirselFileListElement extends HTMLElement {
  private initialized = false;
  private items: FileListItem[] = [];

  static get observedAttributes(): string[] {
    return ["summary", "title", "root-id"];
  }

  connectedCallback(): void {
    if (!this.initialized) {
      this.captureItems();
      this.initialized = true;
    }
    this.render();
  }

  attributeChangedCallback(): void {
    if (this.initialized) this.render();
  }

  private captureItems(): void {
    const defaultRootId = normalizeRootId(this.getAttribute("root-id"));
    const sourceItems = Array.from(this.children);
    this.items = sourceItems
      .map((element) => ({
        detailHtml: element.getAttribute("detail")?.trim()
          ? escapeHtml(element.getAttribute("detail"))
          : element.innerHTML.trim(),
        label:
          element.getAttribute("label")?.trim() ||
          element.getAttribute("title")?.trim() ||
          "",
        lineEnd: element.getAttribute("line-end")?.trim() ?? "",
        lineStart: element.getAttribute("line-start")?.trim() ?? "",
        open: element.hasAttribute("open"),
        path: element.getAttribute("path")?.trim() ?? "",
        rootId: normalizeRootId(element.getAttribute("root-id") || defaultRootId),
        status: normalizeFileStatus(element.getAttribute("status")),
      }))
      .filter((item) => item.path);
  }

  private render(): void {
    const title = this.getAttribute("title")?.trim() || "File List";
    const summary = this.getAttribute("summary")?.trim() || "";
    const statusCounts = this.items.reduce<Record<string, number>>((counts, item) => {
      counts[item.status] = (counts[item.status] ?? 0) + 1;
      return counts;
    }, {});

    this.dataset.hirselReady = "true";
    this.innerHTML = `
      <div class="hirsel-filelist-shell">
        <div class="hirsel-filelist-header">
          <div class="hirsel-filelist-meta">
            <div class="hirsel-code-title">${escapeHtml(title)}</div>
            ${summary ? `<div class="hirsel-code-caption">${escapeHtml(summary)}</div>` : ""}
          </div>
          <div class="hirsel-code-controls">
            <span class="hirsel-code-badge">${this.items.length} file${this.items.length === 1 ? "" : "s"}</span>
            ${["new", "modified", "deleted", "moved"]
              .filter((status) => (statusCounts[status] ?? 0) > 0)
              .map(
                (status) =>
                  `<span class="hirsel-code-badge" data-status="${status}">${formatFileStatus(status)} ${statusCounts[status]}</span>`,
              )
              .join("")}
          </div>
        </div>
        <div class="hirsel-filelist-body">
          ${
            this.items.length === 0
              ? '<div class="hirsel-code-empty">No files were provided.</div>'
              : this.items
                  .map(
                    (item) => `
                      <div class="hirsel-filelist-item" data-status="${item.status}">
                        <div class="hirsel-filelist-row">
                          <hirsel-fileref
                            path="${escapeHtml(item.path)}"
                            root-id="${escapeHtml(item.rootId)}"
                            status="${escapeHtml(item.status)}"
                            ${item.label ? `label="${escapeHtml(item.label)}"` : ""}
                            ${item.lineStart ? `line-start="${escapeHtml(item.lineStart)}"` : ""}
                            ${item.lineEnd ? `line-end="${escapeHtml(item.lineEnd)}"` : ""}
                            ${item.open ? "open" : ""}
                          ></hirsel-fileref>
                        </div>
                        ${item.detailHtml ? `<div class="hirsel-filelist-detail">${item.detailHtml}</div>` : ""}
                      </div>
                    `,
                  )
                  .join("")
          }
        </div>
      </div>
    `;
  }
}

type PatchsetItem = {
  bodyHtml: string;
  open: boolean;
  path: string;
  status: string;
  summary: string;
  title: string;
};

class HirselPatchsetElement extends HTMLElement {
  private initialized = false;
  private items: PatchsetItem[] = [];

  static get observedAttributes(): string[] {
    return ["summary", "title"];
  }

  connectedCallback(): void {
    if (!this.initialized) {
      this.captureItems();
      this.initialized = true;
    }
    this.render();
  }

  attributeChangedCallback(): void {
    if (this.initialized) this.render();
  }

  private captureItems(): void {
    const sections = Array.from(this.children);
    this.items = sections.map((section) => ({
      bodyHtml: section.innerHTML.trim(),
      open: section.hasAttribute("open"),
      path: section.getAttribute("path")?.trim() ?? "",
      status: normalizeFileStatus(section.getAttribute("status")),
      summary:
        section.getAttribute("summary")?.trim() ||
        section.getAttribute("detail")?.trim() ||
        "",
      title:
        section.getAttribute("title")?.trim() ||
        section.getAttribute("label")?.trim() ||
        section.getAttribute("path")?.trim() ||
        "Change",
    }));
  }

  private render(): void {
    const title = this.getAttribute("title")?.trim() || "Patch Set";
    const summary = this.getAttribute("summary")?.trim() || "";
    const statusCounts = this.items.reduce<Record<string, number>>((counts, item) => {
      counts[item.status] = (counts[item.status] ?? 0) + 1;
      return counts;
    }, {});

    this.dataset.hirselReady = "true";
    this.innerHTML = `
      <div class="hirsel-patchset-shell">
        <div class="hirsel-filelist-header">
          <div class="hirsel-filelist-meta">
            <div class="hirsel-code-title">${escapeHtml(title)}</div>
            ${summary ? `<div class="hirsel-code-caption">${escapeHtml(summary)}</div>` : ""}
          </div>
          <div class="hirsel-code-controls">
            <span class="hirsel-code-badge">${this.items.length} file${this.items.length === 1 ? "" : "s"}</span>
            ${["new", "modified", "deleted", "moved"]
              .filter((status) => (statusCounts[status] ?? 0) > 0)
              .map(
                (status) =>
                  `<span class="hirsel-code-badge" data-status="${status}">${formatFileStatus(status)} ${statusCounts[status]}</span>`,
              )
              .join("")}
          </div>
        </div>
        <div class="hirsel-patchset-body">
          ${
            this.items.length === 0
              ? '<div class="hirsel-code-empty">No patch items were provided.</div>'
              : this.items
                  .map(
                    (item, index) => `
                      <details
                        class="hirsel-patchset-item"
                        data-status="${item.status}"
                        ${item.open || index === 0 ? "open" : ""}
                      >
                        <summary class="hirsel-patchset-summary">
                          <div class="hirsel-patchset-heading">
                            <span class="hirsel-file-status" data-status="${item.status}">${escapeHtml(formatFileStatus(item.status))}</span>
                            <span class="hirsel-patchset-title">${escapeHtml(item.title)}</span>
                          </div>
                          ${item.summary ? `<div class="hirsel-patchset-note">${escapeHtml(item.summary)}</div>` : ""}
                          ${item.path ? `<div class="hirsel-patchset-path">${escapeHtml(item.path)}</div>` : ""}
                        </summary>
                        <div class="hirsel-patchset-item-body">
                          ${item.bodyHtml}
                        </div>
                      </details>
                    `,
                  )
                  .join("")
          }
        </div>
      </div>
    `;
  }
}

class HirselDiagramElement extends HirselCodeFrameElement {
  private initialized = false;
  private sourceText = "";
  private svg = "";
  private loading = false;
  private error = "";
  private renderVersion = 0;

  static get observedAttributes(): string[] {
    return ["collapsed", "title"];
  }

  connectedCallback(): void {
    if (!this.initialized) {
      this.sourceText = normalizeCodeText(this.textContent ?? "");
      this.expanded = !this.hasAttribute("collapsed");
      this.initialized = true;
    }
    this.bindFrameInteractions();
    if (this.expanded && !this.svg && !this.loading) {
      void this.renderDiagram();
    }
    this.renderFrame();
  }

  protected cleanupFrame(): void {
    this.renderVersion += 1;
  }

  attributeChangedCallback(name: string): void {
    if (!this.initialized) return;
    if (name === "collapsed") {
      this.expanded = !this.hasAttribute("collapsed");
      if (this.expanded && !this.svg && !this.loading) {
        void this.renderDiagram();
      }
    }
    this.renderFrame();
  }

  protected copyPayload(): string | null {
    return this.sourceText || null;
  }

  protected renderFrame(): void {
    const title = this.getAttribute("title")?.trim() || "Diagram";
    const body = !this.expanded
      ? ""
      : this.loading
        ? buildCodeSkeleton()
        : this.error
          ? `<div class="hirsel-code-empty" data-tone="danger">${escapeHtml(this.error)}</div>`
          : this.svg
            ? `<div class="hirsel-diagram-body"><div class="hirsel-diagram-frame">${this.svg}</div></div>`
            : '<div class="hirsel-code-empty">No diagram source was provided.</div>';

    this.dataset.hirselReady = "true";
    this.innerHTML = `
      <div class="hirsel-diagram-shell" data-expanded="${this.expanded}">
        <div class="hirsel-code-header">
          <div class="hirsel-code-meta">
            <div class="hirsel-code-title">${escapeHtml(title)}</div>
            <div class="hirsel-code-caption">Mermaid diagram</div>
          </div>
          <div class="hirsel-code-controls">
            <span class="hirsel-code-badge">Mermaid</span>
            <button
              type="button"
              class="hirsel-code-action"
              data-action="copy"
              ${this.sourceText.trim() ? "" : "disabled"}
            >
              ${this.copied ? "Copied" : "Copy source"}
            </button>
            <button
              type="button"
              class="hirsel-code-action"
              data-action="toggle"
              aria-expanded="${this.expanded}"
            >
              ${this.expanded ? "Hide" : "Show"}
            </button>
          </div>
        </div>
        ${body}
      </div>
    `;
  }

  private async renderDiagram(): Promise<void> {
    const source = this.sourceText.trim();
    if (!source) {
      this.loading = false;
      this.error = "No Mermaid source was provided.";
      this.svg = "";
      this.renderFrame();
      return;
    }

    const version = ++this.renderVersion;
    this.loading = true;
    this.error = "";
    this.svg = "";
    this.renderFrame();

    try {
      const svg = await renderCanvasMermaid(source, this);
      if (version !== this.renderVersion) return;
      this.loading = false;
      this.error = "";
      this.svg = svg;
      this.renderFrame();
    } catch (error) {
      if (version !== this.renderVersion) return;
      this.loading = false;
      this.svg = "";
      this.error =
        error instanceof Error ? error.message : "Failed to render Mermaid diagram.";
      this.renderFrame();
    }
  }
}

class HirselDiaElement extends HirselDiagramElement {}

class HirselCodeDiffElement extends HirselCodeFrameElement {
  private initialized = false;
  private beforeLabel = "Before";
  private afterLabel = "After";
  private beforeCode = "";
  private afterCode = "";

  static get observedAttributes(): string[] {
    return ["collapsed", "filename", "language", "title"];
  }

  connectedCallback(): void {
    if (!this.initialized) {
      this.captureSources();
      this.expanded = !this.hasAttribute("collapsed");
      this.initialized = true;
    }
    this.bindFrameInteractions();
    this.renderFrame();
  }

  attributeChangedCallback(name: string): void {
    if (!this.initialized) return;
    if (name === "collapsed") {
      this.expanded = !this.hasAttribute("collapsed");
    }
    this.renderFrame();
  }

  protected copyPayload(): string | null {
    const diff = buildLineDiff(this.beforeCode, this.afterCode);
    return diff.unifiedText || null;
  }

  protected renderFrame(): void {
    const titleAttr = this.getAttribute("title")?.trim() ?? "";
    const filename = this.getAttribute("filename")?.trim() ?? "";
    const title = titleAttr || filename || "Code Diff";
    const captionParts = [
      titleAttr && filename && titleAttr !== filename ? filename : "",
      `${this.beforeLabel} -> ${this.afterLabel}`,
    ].filter(Boolean);
    const language =
      inferLanguage(this.getAttribute("language")) || inferLanguage(filename) || null;
    const diff = buildLineDiff(this.beforeCode, this.afterCode);
    const badges = [
      language,
      diff.additions > 0 ? `+${diff.additions}` : null,
      diff.removals > 0 ? `-${diff.removals}` : null,
    ].filter((value): value is string => Boolean(value));

    this.dataset.hirselReady = "true";
    this.innerHTML = buildDiffShell({
      title,
      caption: captionParts.join(" · "),
      badges,
      beforeLabel: this.beforeLabel,
      afterLabel: this.afterLabel,
      rows: diff.rows,
      expanded: this.expanded,
      copied: this.copied,
      copyEnabled: diff.unifiedText.trim().length > 0,
    });
  }

  private captureSources(): void {
    const blocks = Array.from(this.children);
    const beforeBlock =
      blocks.find((block) => block.getAttribute("data-side") === "before") ?? blocks[0] ?? null;
    const afterBlock =
      blocks.find((block) => block.getAttribute("data-side") === "after") ??
      blocks.filter((block) => block !== beforeBlock)[0] ??
      null;

    this.beforeLabel =
      beforeBlock?.getAttribute("label")?.trim() ||
      beforeBlock?.getAttribute("title")?.trim() ||
      "Before";
    this.afterLabel =
      afterBlock?.getAttribute("label")?.trim() ||
      afterBlock?.getAttribute("title")?.trim() ||
      "After";
    this.beforeCode = normalizeCodeText(beforeBlock?.textContent ?? "");
    this.afterCode = normalizeCodeText(afterBlock?.textContent ?? "");
  }
}

class HirselNodeRefElement extends HTMLElement {
  connectedCallback(): void {
    void this.render();
  }

  async attributeChangedCallback(): Promise<void> {
    void this.render();
  }

  static get observedAttributes(): string[] {
    return ["node"];
  }

  private async render(): Promise<void> {
    const projectId = currentCanvasProjectId(this);
    if (!projectId) {
      this.innerHTML = renderNodeError("Missing canvas project context for node reference.");
      return;
    }
    const parsed = parseNodeAttr(this.getAttribute("node"));
    if (!parsed) {
      this.innerHTML = renderNodeError("Invalid node reference. Expected kind:id.");
      return;
    }
    const node = (await loadGraphNodeMap(projectId)).get(nodeKey(parsed.kind, parsed.nodeId));
    if (!node) {
      this.innerHTML = renderNodeError(`Unknown graph node ${parsed.kind}:${parsed.nodeId}.`);
      return;
    }
    this.dataset.hirselReady = "true";
    this.innerHTML = `<span class="hirsel-fileref-shell"><span class="hirsel-fileref-pill" data-status="default"><span class="hirsel-fileref-label">${escapeHtml(node.label || parsed.nodeId)}</span><span class="hirsel-fileref-path">${escapeHtml(parsed.kind)}</span></span></span>`;
  }
}

class HirselNodeFieldElement extends HTMLElement {
  connectedCallback(): void {
    void this.render();
  }

  async attributeChangedCallback(): Promise<void> {
    void this.render();
  }

  static get observedAttributes(): string[] {
    return ["field", "node"];
  }

  private async render(): Promise<void> {
    const projectId = currentCanvasProjectId(this);
    if (!projectId) {
      this.innerHTML = renderNodeError("Missing canvas project context for node field.");
      return;
    }
    const parsed = parseNodeAttr(this.getAttribute("node"));
    const field = this.getAttribute("field")?.trim() ?? "";
    if (!parsed || !field) {
      this.innerHTML = renderNodeError("hirsel-node-field requires node and field attributes.");
      return;
    }
    const node = (await loadGraphNodeMap(projectId)).get(nodeKey(parsed.kind, parsed.nodeId));
    if (!node) {
      this.innerHTML = renderNodeError(`Unknown graph node ${parsed.kind}:${parsed.nodeId}.`);
      return;
    }
    const value = node[field];
    if (value == null) {
      this.innerHTML = renderNodeError(`Field '${field}' not found on ${parsed.kind}:${parsed.nodeId}.`);
      return;
    }
    this.dataset.hirselReady = "true";
    this.innerHTML = `<span>${escapeHtml(typeof value === "string" ? value : JSON.stringify(value))}</span>`;
  }
}

class HirselNodeListElement extends HTMLElement {
  connectedCallback(): void {
    void this.render();
  }

  async attributeChangedCallback(): Promise<void> {
    void this.render();
  }

  static get observedAttributes(): string[] {
    return ["node", "relation"];
  }

  private async render(): Promise<void> {
    const projectId = currentCanvasProjectId(this);
    if (!projectId) {
      this.innerHTML = renderNodeError("Missing canvas project context for node list.");
      return;
    }
    const parsed = parseNodeAttr(this.getAttribute("node"));
    const relation = this.getAttribute("relation")?.trim() ?? "";
    if (!parsed || !relation) {
      this.innerHTML = renderNodeError("hirsel-node-list requires node and relation attributes.");
      return;
    }
    const graph = await getKnowledgeGraph(projectId);
    const nodes = new Map(graph.nodes.map((node) => [nodeKey(node.kind, node.node_id), node as CachedGraphNode]));
    const selfKey = nodeKey(parsed.kind, parsed.nodeId);
    const related = graph.edges
      .filter((edge) => edge.relation === relation)
      .flatMap((edge) => {
        const outKey = String(edge.out && typeof edge.out === "object" ? `${(edge.out as { tb?: string }).tb}:${((edge.out as { id?: unknown }).id as unknown[] | undefined)?.join?.(",") ?? ""}` : edge.out);
        const inKey = String(edge.in && typeof edge.in === "object" ? `${(edge.in as { tb?: string }).tb}:${((edge.in as { id?: unknown }).id as unknown[] | undefined)?.join?.(",") ?? ""}` : edge.in);
        if (outKey.includes(selfKey)) {
          return [inKey];
        }
        if (inKey.includes(selfKey)) {
          return [outKey];
        }
        return [];
      })
      .map((recordKey) => Array.from(nodes.entries()).find(([key]) => recordKey.includes(key))?.[1])
      .filter((node): node is CachedGraphNode => Boolean(node));

    this.dataset.hirselReady = "true";
    if (related.length === 0) {
      this.innerHTML = '<div class="hirsel-code-empty">No related nodes.</div>';
      return;
    }
    this.innerHTML = `<ul>${related.map((node) => `<li>${escapeHtml(node.label || node.node_id)}</li>`).join("")}</ul>`;
  }
}

class HirselDocTargetElement extends HirselNodeRefElement {}

class HirselDocLinkElement extends HTMLElement {
  connectedCallback(): void {
    void this.render();
  }

  async attributeChangedCallback(): Promise<void> {
    void this.render();
  }

  static get observedAttributes(): string[] {
    return ["node"];
  }

  private async render(): Promise<void> {
    const projectId = currentCanvasProjectId(this);
    if (!projectId) {
      this.innerHTML = renderNodeError("Missing canvas project context for doc-link.");
      return;
    }
    const parsed = parseNodeAttr(this.getAttribute("node"));
    if (!parsed) {
      this.innerHTML = renderNodeError("hirsel-doc-link requires a node attribute (e.g. document:architecture).");
      return;
    }
    const node = (await loadGraphNodeMap(projectId)).get(nodeKey(parsed.kind, parsed.nodeId));
    if (!node) {
      this.innerHTML = renderNodeError(`Unknown node ${parsed.kind}:${parsed.nodeId}.`);
      return;
    }
    const label = escapeHtml(node.label || parsed.nodeId);
    const summary = node.summary ? escapeHtml(node.summary) : "";
    const kind = escapeHtml(parsed.kind);
    this.dataset.hirselReady = "true";
    this.innerHTML = `<div class="hirsel-doc-link-card">
      <div class="hirsel-doc-link-head">
        <span class="hirsel-doc-link-kind">${kind}</span>
        <span class="hirsel-doc-link-title">${label}</span>
      </div>
      ${summary ? `<div class="hirsel-doc-link-summary">${summary}</div>` : ""}
    </div>`;
  }
}

class HirselDocEmbedElement extends HTMLElement {
  connectedCallback(): void {
    void this.render();
  }

  async attributeChangedCallback(): Promise<void> {
    void this.render();
  }

  static get observedAttributes(): string[] {
    return ["node"];
  }

  private async render(): Promise<void> {
    const projectId = currentCanvasProjectId(this);
    if (!projectId) {
      this.innerHTML = renderNodeError("Missing canvas project context for doc-embed.");
      return;
    }
    const parsed = parseNodeAttr(this.getAttribute("node"));
    if (!parsed) {
      this.innerHTML = renderNodeError("hirsel-doc-embed requires a node attribute (e.g. document:architecture).");
      return;
    }
    const node = (await loadGraphNodeMap(projectId)).get(nodeKey(parsed.kind, parsed.nodeId));
    if (!node) {
      this.innerHTML = renderNodeError(`Unknown node ${parsed.kind}:${parsed.nodeId}.`);
      return;
    }
    const bodyHtml = (node as { body_html?: string }).body_html ?? "";
    const label = escapeHtml(node.label || parsed.nodeId);
    const kind = escapeHtml(parsed.kind);
    this.dataset.hirselReady = "true";
    if (!bodyHtml.trim()) {
      this.innerHTML = `<div class="hirsel-doc-embed-shell">
        <div class="hirsel-doc-embed-header"><span class="hirsel-doc-link-kind">${kind}</span> ${label}</div>
        <div class="hirsel-code-empty">This document has no content yet.</div>
      </div>`;
      return;
    }
    this.innerHTML = `<div class="hirsel-doc-embed-shell">
      <div class="hirsel-doc-embed-header"><span class="hirsel-doc-link-kind">${kind}</span> ${label}</div>
      <div class="hirsel-doc-embed-body">${bodyHtml}</div>
    </div>`;
  }
}

function defineElement(name: string, ctor: CustomElementConstructor): void {
  if (!customElements.get(name)) {
    customElements.define(name, ctor);
  }
}

export function registerCanvasComponents(): void {
  defineElement("hirsel-card", HirselCardElement);
  defineElement("hirsel-callout", HirselCalloutElement);
  defineElement("hirsel-code", HirselCodeElement);
  defineElement("hirsel-codediff", HirselCodeDiffElement);
  defineElement("hirsel-coderef", HirselCodeRefElement);
  defineElement("hirsel-dia", HirselDiaElement);
  defineElement("hirsel-diagram", HirselDiagramElement);
  defineElement("hirsel-stat-grid", HirselStatGridElement);
  defineElement("hirsel-stat", HirselStatElement);
  defineElement("hirsel-tabs", HirselTabsElement);
  defineElement("hirsel-disclosure", HirselDisclosureElement);
  defineElement("hirsel-fileref", HirselFileRefElement);
  defineElement("hirsel-filelist", HirselFileListElement);
  defineElement("hirsel-node-ref", HirselNodeRefElement);
  defineElement("hirsel-node-field", HirselNodeFieldElement);
  defineElement("hirsel-node-list", HirselNodeListElement);
  defineElement("hirsel-doc-target", HirselDocTargetElement);
  defineElement("hirsel-doc-link", HirselDocLinkElement);
  defineElement("hirsel-doc-embed", HirselDocEmbedElement);
  defineElement("hirsel-patchset", HirselPatchsetElement);
  defineElement("hirsel-progress", HirselProgressElement);
}
