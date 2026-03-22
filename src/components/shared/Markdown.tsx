import { Component, createMemo } from 'solid-js';
import { Marked } from 'marked';
import DOMPurify from 'dompurify';

export interface MarkdownProps {
  content: string;
  class?: string;
  compact?: boolean;
}

const marked = new Marked({
  gfm: true,
  breaks: true,
});

export const Markdown: Component<MarkdownProps> = (props) => {
  const html = createMemo(() => {
    if (!props.content) return '';
    try {
      const normalized = props.content.replace(/\\n/g, '\n');
      return DOMPurify.sanitize(marked.parse(normalized, { async: false }) as string);
    } catch {
      return '';
    }
  });

  return (
    <div
      class={`markdown-preview markdown-content${props.compact ? ' compact' : ''}${props.class ? ` ${props.class}` : ''}`}
      innerHTML={html()}
    />
  );
};
