/**
 * Shared markdown renderer using `marked` with custom styling
 *
 * Used by DocsPanel and DocsFullView for consistent markdown rendering
 * with proper table support and design-language styling.
 */
import { type Component, createEffect } from 'solid-js';
import { Marked, type RendererObject } from 'marked';
import DOMPurify from 'dompurify';

interface MarkdownContentProps {
  content: string;
  /** Compact mode uses smaller text sizes (for side panel) */
  compact?: boolean;
}

// Create a custom renderer with Hirsel design-language styling
const createRenderer = (compact: boolean): RendererObject => ({
  // Headers
  heading({ text, depth }) {
    const compactStyles: Record<number, string> = {
      1: 'text-base font-bold text-wool-100 mt-4 mb-2 pb-1.5 border-b border-pasture-700',
      2: 'text-sm font-semibold text-wool-100 mt-3 mb-1.5',
      3: 'text-sm font-medium text-wool-200 mt-2.5 mb-1',
      4: 'text-xs font-medium text-wool-300 mt-2 mb-1',
      5: 'text-xs font-medium text-wool-400 mt-1.5 mb-0.5',
      6: 'text-xs font-medium text-wool-500 mt-1.5 mb-0.5',
    };
    const fullStyles: Record<number, string> = {
      1: 'text-xl font-bold text-wool-100 mt-6 mb-3 pb-2 border-b border-pasture-700',
      2: 'text-lg font-semibold text-wool-100 mt-5 mb-2',
      3: 'text-base font-medium text-wool-200 mt-4 mb-2',
      4: 'text-sm font-medium text-wool-300 mt-3 mb-1.5',
      5: 'text-sm font-medium text-wool-400 mt-2 mb-1',
      6: 'text-xs font-medium text-wool-500 mt-2 mb-1',
    };
    const styles = compact ? compactStyles : fullStyles;
    return `<h${depth} class="${styles[depth] || styles[6]}">${text}</h${depth}>`;
  },

  // Paragraphs
  paragraph({ text }) {
    const size = compact ? 'text-xs' : 'text-sm';
    return `<p class="${size} text-wool-300 my-2 leading-relaxed">${text}</p>`;
  },

  // Code blocks
  code({ text, lang }) {
    const size = compact ? 'text-[11px]' : 'text-xs';
    const padding = compact ? 'p-2.5' : 'p-3';
    const margin = compact ? 'my-2.5' : 'my-4';
    const langBadge = lang
      ? `<div class="absolute top-1.5 right-2 text-[9px] text-wool-600 font-mono uppercase">${escapeHtml(lang)}</div>`
      : '';
    return `<div class="relative ${margin}"><pre class="bg-pasture-800 rounded-lg ${padding} overflow-x-auto ${size} font-mono text-wool-300">${langBadge}<code>${escapeHtml(text)}</code></pre></div>`;
  },

  // Inline code
  codespan({ text }) {
    return `<code class="bg-pasture-800 px-1.5 py-0.5 rounded text-xs text-sage-300 font-mono">${escapeHtml(text)}</code>`;
  },

  // Links
  link({ href, title, text }) {
    const titleAttr = title ? ` title="${escapeHtml(title)}"` : '';
    return `<a href="${escapeHtml(href)}"${titleAttr} class="text-sage-400 hover:text-sage-300 hover:underline underline-offset-2" target="_blank" rel="noopener noreferrer">${text}</a>`;
  },

  // Bold
  strong({ text }) {
    return `<strong class="font-semibold text-wool-200">${text}</strong>`;
  },

  // Italic
  em({ text }) {
    return `<em class="italic text-wool-300">${text}</em>`;
  },

  // Blockquotes
  blockquote({ text }) {
    const padding = compact ? 'pl-3 py-0.5 my-2' : 'pl-4 py-1 my-4';
    return `<blockquote class="border-l-4 border-sage ${padding} text-wool-400 italic">${text}</blockquote>`;
  },

  // Horizontal rule
  hr() {
    const margin = compact ? 'my-3' : 'my-6';
    return `<hr class="border-pasture-600 ${margin}" />`;
  },

  // Lists
  list({ items, ordered }) {
    const tag = ordered ? 'ol' : 'ul';
    const listStyle = ordered ? 'list-decimal' : 'list-disc';
    const size = compact ? 'text-xs' : 'text-sm';
    const margin = compact ? 'ml-4 my-1.5' : 'ml-5 my-2';
    // Render each item using the listitem renderer
    const body = items.map(item => this.listitem!(item)).join('');
    return `<${tag} class="${listStyle} ${margin} ${size} text-wool-300 space-y-0.5">${body}</${tag}>`;
  },

  listitem({ text }) {
    return `<li>${text}</li>`;
  },

  // Tables
  table({ header, rows, align }) {
    const margin = compact ? 'my-2.5' : 'my-4';
    const size = compact ? 'text-xs' : 'text-sm';
    const padding = compact ? 'px-2 py-1.5' : 'px-3 py-2';

    // Render header
    const headerHtml = header.map((cell, i) => {
      const alignClass = align[i] ? `text-${align[i]}` : 'text-left';
      return `<th class="${padding} bg-pasture-800 text-wool-200 font-medium border-b border-pasture-600 ${alignClass}">${cell.text}</th>`;
    }).join('');

    // Render body rows
    const bodyHtml = rows.map(row => {
      const cells = row.map((cell, i) => {
        const alignClass = align[i] ? `text-${align[i]}` : 'text-left';
        return `<td class="${padding} text-wool-400 ${alignClass}">${cell.text}</td>`;
      }).join('');
      return `<tr class="border-b border-pasture-700/50">${cells}</tr>`;
    }).join('');

    return `<div class="overflow-x-auto ${margin}"><table class="w-full border-collapse ${size}"><thead><tr class="border-b border-pasture-600">${headerHtml}</tr></thead><tbody>${bodyHtml}</tbody></table></div>`;
  },

  // Images
  image({ href, title, text }) {
    const titleAttr = title ? ` title="${escapeHtml(title)}"` : '';
    const margin = compact ? 'my-2' : 'my-4';
    return `<img src="${escapeHtml(href)}" alt="${escapeHtml(text)}"${titleAttr} class="${margin} rounded-lg max-w-full h-auto" loading="lazy" />`;
  },
});

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#039;');
}

// Create marked instances for compact and full modes
const markedCompact = new Marked({ renderer: createRenderer(true), gfm: true });
const markedFull = new Marked({ renderer: createRenderer(false), gfm: true });

export const MarkdownContent: Component<MarkdownContentProps> = (props) => {
  let ref: HTMLDivElement | undefined;

  // Use effect to properly clear and set innerHTML when content changes
  createEffect(() => {
    if (ref) {
      const marked = props.compact ? markedCompact : markedFull;
      const html = marked.parse(props.content, { async: false }) as string;
      ref.innerHTML = DOMPurify.sanitize(html);
    }
  });

  return <div ref={ref} />;
};
