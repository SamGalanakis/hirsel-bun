import { Marked } from "marked";
import type { CanvasNode } from "@/lib/api/types";

const marked = new Marked({
  breaks: true,
  gfm: true,
});

/** Parse markdown to HTML. The output is from a trusted parser (not raw LLM HTML). */
export function renderMarkdown(text: string): string {
  const result = marked.parse(text);
  return typeof result === "string" ? result : "";
}

const KINDS = "task|thread|component|entity|decision|fact|goal|convention|document";
const EMBED_RE = new RegExp(`!\\[\\[(${KINDS}):([a-zA-Z0-9_\\-:.]+)\\]\\]`, "g");
const REF_RE = new RegExp(`\\[(${KINDS}):([a-zA-Z0-9_\\-:.]+)\\]`, "g");

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function replaceRefs(
  html: string,
  resolve: (key: string) => CanvasNode | undefined,
): string {
  return html.replace(REF_RE, (_m, kind: string, id: string) => {
    const key = `${kind}:${id}`;
    const node = resolve(key);
    const label = node?.label ?? id;
    return `<a class="canvas-node-ref" data-node-key="${escapeHtml(key)}" href="#${escapeHtml(key)}" title="Go to ${escapeHtml(kind)}: ${escapeHtml(label)}"><span class="canvas-node-ref-kind">${escapeHtml(kind)}</span><span class="canvas-node-ref-label">${escapeHtml(label)}</span></a>`;
  });
}

/**
 * Render markdown with canvas-aware extensions:
 *   [kind:id]    → clickable styled pill that jumps to the referenced node
 *   ![[kind:id]] → inline-embedded card rendering the referenced node's content
 *
 * Embeds render their inner content one level deep (refs only, no nested embeds),
 * so graph cycles can't trigger infinite recursion.
 */
export function renderNodeMarkdown(
  text: string,
  resolve: (key: string) => CanvasNode | undefined,
): string {
  let html = renderMarkdown(text);
  html = html.replace(EMBED_RE, (_m, kind: string, id: string) => {
    const key = `${kind}:${id}`;
    const node = resolve(key);
    const label = node?.label ?? id;
    const innerHtml = node?.content
      ? replaceRefs(renderMarkdown(node.content), resolve)
      : '<em class="canvas-node-embed-empty">No content yet</em>';
    return `<figure class="canvas-node-embed" data-node-key="${escapeHtml(key)}">
      <figcaption class="canvas-node-embed-header">
        <span class="canvas-node-embed-kind">${escapeHtml(kind)}</span>
        <span class="canvas-node-embed-title">${escapeHtml(label)}</span>
        <span class="canvas-node-embed-open" aria-hidden="true">open →</span>
      </figcaption>
      <div class="canvas-node-embed-body markdown-body">${innerHtml}</div>
    </figure>`;
  });
  html = replaceRefs(html, resolve);
  return html;
}
