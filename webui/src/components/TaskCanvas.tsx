import { type Component, Show, createEffect, createSignal, onCleanup } from "solid-js";
import { renderMarkdown } from "@/lib/markdown";

interface TaskCanvasProps {
  content: string | null;
  taskTitle?: string;
}

/**
 * Renders task markdown content with:
 * - Standard markdown (via marked)
 * - Mermaid fenced code blocks → rendered as diagrams
 * - [kind:id] references → styled inline chips
 */
const TaskCanvas: Component<TaskCanvasProps> = (props) => {
  let containerRef: HTMLDivElement | undefined;
  const [renderedHtml, setRenderedHtml] = createSignal("");

  // Process markdown and special syntax
  createEffect(() => {
    const raw = props.content ?? "";
    if (!raw.trim()) {
      setRenderedHtml("");
      return;
    }

    // Render markdown to HTML
    let html = renderMarkdown(raw);

    // Post-process: convert [kind:id] references to styled chips
    html = html.replace(
      /\[([\w-]+):([\w./-]+)\]/g,
      '<span class="task-node-ref" data-kind="$1" data-id="$2">$1:$2</span>',
    );

    setRenderedHtml(html);
  });

  // After HTML is inserted, find mermaid code blocks and render them
  createEffect(() => {
    const _html = renderedHtml();
    if (!containerRef || !_html) return;

    requestAnimationFrame(async () => {
      if (!containerRef) return;
      const codeBlocks = containerRef.querySelectorAll("pre > code.language-mermaid");
      if (codeBlocks.length === 0) return;

      try {
        const { default: mermaid } = await import("mermaid");

        // Initialize with theme colors
        const bg = getComputedStyle(document.documentElement)
          .getPropertyValue("--color-background")
          .trim();
        const fg = getComputedStyle(document.documentElement)
          .getPropertyValue("--color-foreground")
          .trim();

        mermaid.initialize({
          startOnLoad: false,
          theme: "base",
          themeVariables: {
            primaryColor: bg || "#1a1a1a",
            primaryTextColor: fg || "#e0e0e0",
            primaryBorderColor: "#3a3a3a",
            lineColor: "#555",
            fontFamily: "Red Hat Mono, monospace",
            fontSize: "13px",
          },
        });

        for (const block of codeBlocks) {
          const source = block.textContent ?? "";
          const pre = block.parentElement;
          if (!pre || !source.trim()) continue;

          const id = `mermaid-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`;
          const container = document.createElement("div");
          container.className = "task-mermaid-container";

          try {
            const { svg } = await mermaid.render(id, source.trim());
            container.innerHTML = svg;
            pre.replaceWith(container);
          } catch {
            // Leave the code block as-is if mermaid fails
          }
        }
      } catch {
        // Mermaid import failed — leave code blocks as-is
      }
    });
  });

  return (
    <div class="h-full overflow-y-auto chassis-scroll">
      <Show
        when={renderedHtml().trim()}
        fallback={
          <div class="flex flex-col items-start gap-5 p-6">
            <div class="font-mono text-[10px] uppercase tracking-[0.18em] text-muted-foreground/40 flex items-center gap-2">
              <span class="inline-block h-px w-6 bg-muted-foreground/30" />
              <span>Task Canvas</span>
            </div>
            <Show when={props.taskTitle}>
              <h3 class="font-display text-xl font-normal tracking-tight text-foreground">
                {props.taskTitle}
              </h3>
            </Show>
            <p class="max-w-xs text-[13px] leading-[1.7] text-muted-foreground">
              Focus a task to see its content here. Use the thread's <code>focus_task</code> tool or click a task in the task list.
            </p>
          </div>
        }
      >
        <div
          ref={containerRef}
          class="task-canvas-content markdown-body px-5 py-4"
          innerHTML={renderedHtml()}
        />
      </Show>
    </div>
  );
};

export default TaskCanvas;
