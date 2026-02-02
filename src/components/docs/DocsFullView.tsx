/**
 * Full-screen documentation view
 *
 * Shows documentation files with tab navigation and full markdown rendering.
 */
import { invoke } from '@tauri-apps/api/core';
import {
  type Component,
  For,
  Show,
  createEffect,
  createResource,
  createSignal,
  onCleanup,
} from 'solid-js';
import { useProject } from '../../stores';
import { Icon } from '../shared';

interface DocsSource {
  kind: 'workspace' | 'run';
  runName?: string;
  runStatus?: string;
}

interface DocFile {
  name: string;
  content: string;
}

interface ProjectDocsResponse {
  source: DocsSource;
  files: DocFile[];
}

export const DocsFullView: Component = () => {
  const project = useProject();
  const [activeTab, setActiveTab] = createSignal<string | null>(null);

  // Fetch docs
  const [docs, { refetch }] = createResource(
    () => project.docsFullScreen() ? project.selectedProjectId() : null,
    async (projectId) => {
      if (!projectId) return null;
      try {
        return await invoke<ProjectDocsResponse>('get_project_docs', { projectId });
      } catch (e) {
        console.error('Failed to fetch project docs:', e);
        return null;
      }
    }
  );

  // Set active tab when docs load
  createEffect(() => {
    const d = docs();
    if (d && d.files.length > 0) {
      // Try to use the selected file from panel, otherwise first file
      const selected = project.selectedDocFile();
      if (selected && d.files.find(f => f.name === selected)) {
        setActiveTab(selected);
      } else {
        setActiveTab(d.files[0].name);
      }
    }
  });

  // Poll for updates
  createEffect(() => {
    if (!project.docsFullScreen()) return;

    const interval = setInterval(() => {
      refetch();
    }, 5000);

    onCleanup(() => clearInterval(interval));
  });

  const activeFile = () => {
    const d = docs();
    const name = activeTab();
    if (!d || !name) return null;
    return d.files.find(f => f.name === name) || null;
  };

  const handleBack = () => {
    project.setDocsFullScreen(false);
    // Sync the selected file back to the panel
    const tab = activeTab();
    if (tab) {
      project.setSelectedDocFile(tab);
    }
  };

  const handleClose = () => {
    project.setDocsFullScreen(false);
    project.setDocsOpen(false);
  };

  return (
    <div class="absolute inset-0 z-50 flex flex-col bg-pasture-900">
      {/* Header */}
      <div class="flex items-center justify-between px-4 py-2 border-b border-pasture-600/50 bg-pasture-900/95">
        <div class="flex items-center gap-3">
          <button
            type="button"
            onClick={handleBack}
            class="flex items-center gap-1.5 px-2 py-1 rounded text-wool-400 hover:text-wool-200 hover:bg-pasture-800 transition-colors"
          >
            <Icon name="arrow-left" class="w-4 h-4" />
            <span class="text-sm">Back</span>
          </button>
          <div class="flex items-center gap-2">
            <Icon name="book-open" class="w-4 h-4 text-wool-400" />
            <span class="text-sm font-medium text-wool-200">Documentation</span>
            <Show when={docs()}>
              <SourceBadge source={docs()!.source} />
            </Show>
          </div>
        </div>
        <button
          type="button"
          onClick={handleClose}
          class="p-2 rounded text-wool-500 hover:text-wool-300 hover:bg-pasture-800 transition-colors"
          title="Close"
        >
          <Icon name="x" class="w-4 h-4" />
        </button>
      </div>

      {/* Loading state */}
      <Show when={docs.loading}>
        <div class="flex-1 flex items-center justify-center">
          <Icon name="loader-2" class="w-8 h-8 text-wool-500 animate-spin" />
        </div>
      </Show>

      {/* Empty state */}
      <Show when={!docs.loading && (!docs() || docs()!.files.length === 0)}>
        <div class="flex-1 flex flex-col items-center justify-center">
          <Icon name="file-text" class="w-16 h-16 text-wool-600 mb-4" />
          <h3 class="text-lg font-medium text-wool-300 mb-2">No Documentation</h3>
          <p class="text-sm text-wool-500 text-center max-w-md">
            Add markdown files to your project's docs folder to see them here.
          </p>
        </div>
      </Show>

      {/* Tabs and content */}
      <Show when={!docs.loading && docs() && docs()!.files.length > 0}>
        {/* Tab bar */}
        <div class="flex items-center gap-1 px-4 py-2 border-b border-pasture-700/50 bg-pasture-800/50 overflow-x-auto">
          <For each={docs()!.files}>
            {(file) => (
              <button
                type="button"
                onClick={() => setActiveTab(file.name)}
                class="flex items-center gap-2 px-3 py-1.5 rounded text-sm whitespace-nowrap transition-colors"
                classList={{
                  'bg-pasture-700 text-wool-100': activeTab() === file.name,
                  'text-wool-400 hover:text-wool-200 hover:bg-pasture-700/50':
                    activeTab() !== file.name,
                }}
              >
                <Icon name="file-text" class="w-4 h-4" />
                <span>{file.name.replace('.md', '')}</span>
              </button>
            )}
          </For>
        </div>

        {/* Content */}
        <div class="flex-1 overflow-auto">
          <Show when={activeFile()}>
            <div class="max-w-4xl mx-auto px-8 py-6">
              <div class="prose prose-invert prose-lg max-w-none">
                <MarkdownRenderer content={activeFile()!.content} />
              </div>
            </div>
          </Show>
        </div>
      </Show>
    </div>
  );
};

/** Source badge */
const SourceBadge: Component<{ source: DocsSource }> = (props) => {
  const badgeClass = () => {
    if (props.source.kind === 'run') {
      const status = props.source.runStatus?.toLowerCase();
      if (status === 'working' || status === 'eval') {
        return 'bg-amber-500/20 text-amber-400 border-amber-500/30';
      }
      return 'bg-sage-500/20 text-sage-400 border-sage-500/30';
    }
    return 'bg-wool-500/20 text-wool-400 border-wool-500/30';
  };

  const badgeText = () => {
    if (props.source.kind === 'run' && props.source.runName) {
      const status = props.source.runStatus?.toLowerCase();
      if (status === 'done' || status === 'completed') {
        return `${props.source.runName}`;
      }
      return `Run: ${props.source.runName}`;
    }
    return 'Workspace';
  };

  const badgeIcon = () => {
    if (props.source.kind === 'run') {
      const status = props.source.runStatus?.toLowerCase();
      if (status === 'done' || status === 'completed') {
        return 'check';
      }
      return 'play';
    }
    return 'folder';
  };

  return (
    <span
      class={`inline-flex items-center gap-1 px-2 py-0.5 text-xs rounded border ${badgeClass()}`}
    >
      <Icon name={badgeIcon()} class="w-3 h-3" />
      <span>{badgeText()}</span>
    </span>
  );
};

/** Full markdown renderer with better formatting */
const MarkdownRenderer: Component<{ content: string }> = (props) => {
  const renderMarkdown = () => {
    const lines = props.content.split('\n');
    const elements: string[] = [];
    let inCodeBlock = false;
    let codeBlockContent: string[] = [];
    let inTable = false;
    let tableRows: string[][] = [];

    for (const line of lines) {
      // Code block handling
      if (line.startsWith('```')) {
        if (inCodeBlock) {
          elements.push(
            `<pre class="bg-pasture-800 rounded-lg p-4 overflow-x-auto text-sm my-4"><code>${escapeHtml(codeBlockContent.join('\n'))}</code></pre>`
          );
          codeBlockContent = [];
          inCodeBlock = false;
        } else {
          inCodeBlock = true;
        }
        continue;
      }

      if (inCodeBlock) {
        codeBlockContent.push(line);
        continue;
      }

      // Table handling
      if (line.includes('|') && line.trim().startsWith('|')) {
        if (!inTable) {
          inTable = true;
          tableRows = [];
        }
        // Skip separator row
        if (line.match(/^\|[\s-:|]+\|$/)) {
          continue;
        }
        const cells = line.split('|').slice(1, -1).map(c => c.trim());
        tableRows.push(cells);
        continue;
      }
      if (inTable) {
        // End of table
        elements.push(renderTable(tableRows));
        tableRows = [];
        inTable = false;
      }

      // Headers
      if (line.startsWith('# ')) {
        elements.push(`<h1 class="text-2xl font-bold text-wool-100 mt-8 mb-4 pb-2 border-b border-pasture-700">${escapeHtml(line.slice(2))}</h1>`);
        continue;
      }
      if (line.startsWith('## ')) {
        elements.push(`<h2 class="text-xl font-semibold text-wool-100 mt-6 mb-3">${escapeHtml(line.slice(3))}</h2>`);
        continue;
      }
      if (line.startsWith('### ')) {
        elements.push(`<h3 class="text-lg font-medium text-wool-200 mt-4 mb-2">${escapeHtml(line.slice(4))}</h3>`);
        continue;
      }
      if (line.startsWith('#### ')) {
        elements.push(`<h4 class="text-base font-medium text-wool-300 mt-3 mb-2">${escapeHtml(line.slice(5))}</h4>`);
        continue;
      }

      // Blockquote
      if (line.startsWith('> ')) {
        elements.push(`<blockquote class="border-l-4 border-sage-500 pl-4 py-1 my-4 text-wool-400 italic">${formatInlineMarkdown(line.slice(2))}</blockquote>`);
        continue;
      }

      // Horizontal rule
      if (line.match(/^-{3,}$/) || line.match(/^\*{3,}$/)) {
        elements.push('<hr class="border-pasture-600 my-6" />');
        continue;
      }

      // List items
      if (line.match(/^[-*] /)) {
        elements.push(`<li class="text-wool-300 ml-6 my-1">${formatInlineMarkdown(line.slice(2))}</li>`);
        continue;
      }

      // Numbered list
      const numMatch = line.match(/^(\d+)\. /);
      if (numMatch) {
        elements.push(`<li class="text-wool-300 ml-6 my-1 list-decimal">${formatInlineMarkdown(line.slice(numMatch[0].length))}</li>`);
        continue;
      }

      // Empty line
      if (line.trim() === '') {
        elements.push('<div class="h-4"></div>');
        continue;
      }

      // Regular paragraph
      elements.push(`<p class="text-wool-300 my-3 leading-relaxed">${formatInlineMarkdown(line)}</p>`);
    }

    // Close any open table
    if (inTable && tableRows.length > 0) {
      elements.push(renderTable(tableRows));
    }

    return elements.join('');
  };

  return <div innerHTML={renderMarkdown()} />;
};

/** Render a markdown table */
function renderTable(rows: string[][]): string {
  if (rows.length === 0) return '';

  const headerRow = rows[0];
  const bodyRows = rows.slice(1);

  let html = '<div class="overflow-x-auto my-4"><table class="w-full border-collapse text-sm">';

  // Header
  html += '<thead><tr class="border-b border-pasture-600">';
  for (const cell of headerRow) {
    html += `<th class="text-left px-3 py-2 text-wool-200 font-medium">${formatInlineMarkdown(cell)}</th>`;
  }
  html += '</tr></thead>';

  // Body
  if (bodyRows.length > 0) {
    html += '<tbody>';
    for (const row of bodyRows) {
      html += '<tr class="border-b border-pasture-700/50">';
      for (const cell of row) {
        html += `<td class="px-3 py-2 text-wool-400">${formatInlineMarkdown(cell)}</td>`;
      }
      html += '</tr>';
    }
    html += '</tbody>';
  }

  html += '</table></div>';
  return html;
}

/** Escape HTML special characters */
function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#039;');
}

/** Format inline markdown */
function formatInlineMarkdown(text: string): string {
  let result = escapeHtml(text);

  // Inline code
  result = result.replace(/`([^`]+)`/g, '<code class="bg-pasture-800 px-1.5 py-0.5 rounded text-sm text-sage-300">$1</code>');

  // Bold
  result = result.replace(/\*\*([^*]+)\*\*/g, '<strong class="font-semibold text-wool-200">$1</strong>');

  // Italic
  result = result.replace(/\*([^*]+)\*/g, '<em class="italic">$1</em>');

  // Links
  result = result.replace(
    /\[([^\]]+)\]\(([^)]+)\)/g,
    '<a href="$2" class="text-sage-400 hover:text-sage-300 hover:underline" target="_blank" rel="noopener">$1</a>'
  );

  return result;
}
