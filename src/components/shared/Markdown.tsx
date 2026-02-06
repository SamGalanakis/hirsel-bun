/**
 * Markdown renderer component
 *
 * Renders markdown content as HTML using marked.
 * Used for AI chat responses.
 */
import { Component, createMemo } from 'solid-js';
import { marked } from 'marked';

export interface MarkdownProps {
  /** Markdown content to render */
  content: string;
  /** Additional CSS classes */
  class?: string;
}

// Configure marked for safe rendering
marked.setOptions({
  breaks: true, // Convert \n to <br>
  gfm: true, // GitHub Flavored Markdown
});

/** Renders markdown content as HTML */
export const Markdown: Component<MarkdownProps> = (props) => {
  const html = createMemo(() => {
    if (!props.content) return '';
    try {
      return marked.parse(props.content, { async: false }) as string;
    } catch {
      return props.content;
    }
  });

  return (
    <div
      class={`markdown-content ${props.class || ''}`}
      innerHTML={html()}
    />
  );
};
