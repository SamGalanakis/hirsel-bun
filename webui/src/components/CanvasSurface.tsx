import { type Component, createEffect, onCleanup } from "solid-js";
import { cn } from "@/lib/cn";
import { sanitizeCanvasHtml, scopeCanvasCss } from "@/lib/canvas-html";

let nextCanvasInstance = 0;

function dispatchCanvasTeardown(root: HTMLDivElement): void {
  root.dispatchEvent(new CustomEvent("hirsel-canvas-teardown"));
}

function runCanvasScripts(root: HTMLDivElement, scopeSelector: string): void {
  const scriptNodes = Array.from(root.querySelectorAll("script"));
  for (const scriptNode of scriptNodes) {
    const type = (scriptNode.getAttribute("type") ?? "").trim().toLowerCase();
    if (type && type !== "text/javascript" && type !== "application/javascript") {
      scriptNode.remove();
      continue;
    }

    const source = scriptNode.textContent ?? "";
    const replacement = document.createElement("script");
    replacement.type = "text/javascript";
    replacement.textContent = [
      "(function(){",
      `const canvasRoot = document.querySelector(${JSON.stringify(scopeSelector)});`,
      "if (!canvasRoot) return;",
      "const mermaid = window.mermaid;",
      source,
      "})();",
    ].join("\n");
    scriptNode.replaceWith(replacement);
  }
}

function scopeCanvasStyles(root: HTMLDivElement, scopeSelector: string): void {
  const styleNodes = Array.from(root.querySelectorAll("style"));
  for (const styleNode of styleNodes) {
    const css = styleNode.textContent ?? "";
    styleNode.textContent = scopeCanvasCss(css, scopeSelector);
  }
}

const CanvasSurface: Component<{ html: string; projectId: number; class?: string }> = (props) => {
  let rootRef: HTMLDivElement | undefined;
  const instanceId = `hirsel-canvas-${++nextCanvasInstance}`;
  const scopeSelector = `[data-canvas-instance="${instanceId}"]`;

  createEffect(() => {
    const root = rootRef;
    if (!root) return;

    dispatchCanvasTeardown(root);
    root.dataset.canvasInstance = instanceId;
    root.dataset.canvasProjectId = String(props.projectId);
    root.innerHTML = sanitizeCanvasHtml(props.html);
    scopeCanvasStyles(root, scopeSelector);
    runCanvasScripts(root, scopeSelector);
  });

  onCleanup(() => {
    if (rootRef) {
      dispatchCanvasTeardown(rootRef);
    }
  });

  return (
    <div
      ref={rootRef}
      class={cn("canvas-html px-5 py-5 md:px-7 md:py-6", props.class)}
    />
  );
};

export default CanvasSurface;
