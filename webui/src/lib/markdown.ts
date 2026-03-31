import { Marked } from "marked";

const marked = new Marked({
  breaks: true,
  gfm: true,
});

/** Parse markdown to HTML. The output is from a trusted parser (not raw LLM HTML). */
export function renderMarkdown(text: string): string {
  const result = marked.parse(text);
  return typeof result === "string" ? result : "";
}
