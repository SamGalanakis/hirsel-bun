import { type Component, createMemo } from 'solid-js';
import { Marked } from 'marked';
import DOMPurify from 'dompurify';

const marked = new Marked({ gfm: true });

export const MarkdownContent: Component<{ content: string; compact?: boolean }> = (props) => {
  const html = createMemo(() => {
    if (!props.content) return '';
    try {
      return DOMPurify.sanitize(marked.parse(props.content, { async: false }) as string);
    } catch {
      return '';
    }
  });

  return <div class={`markdown-preview${props.compact ? ' compact' : ''}`} innerHTML={html()} />;
};
