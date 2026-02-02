/**
 * Project documentation side panel
 *
 * Shows documentation files from workspace or active run with markdown preview.
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

export const DocsPanel: Component = () => {
  const project = useProject();
  const [localSelectedFile, setLocalSelectedFile] = createSignal<string | null>(null);

  // Fetch docs when panel is open
  const [docs, { refetch }] = createResource(
    () => project.docsOpen() ? project.selectedProjectId() : null,
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

  // Auto-select first file when docs load
  createEffect(() => {
    const d = docs();
    if (d && d.files.length > 0) {
      const currentSelection = localSelectedFile();
      // Only auto-select if nothing selected or selection not in list
      if (!currentSelection || !d.files.find(f => f.name === currentSelection)) {
        setLocalSelectedFile(d.files[0].name);
      }
    }
  });

  // Poll for updates when panel is open (scribe might be updating docs)
  createEffect(() => {
    if (!project.docsOpen()) return;

    const interval = setInterval(() => {
      refetch();
    }, 5000);

    onCleanup(() => clearInterval(interval));
  });

  const selectedFile = () => {
    const d = docs();
    const name = localSelectedFile();
    if (!d || !name) return null;
    return d.files.find(f => f.name === name) || null;
  };

  const handleFileClick = (fileName: string) => {
    setLocalSelectedFile(fileName);
    project.setSelectedDocFile(fileName);
  };

  const handleExpand = () => {
    project.setDocsFullScreen(true);
  };

  const handleClose = () => {
    project.setDocsOpen(false);
  };

  return (
    <div class="w-80 flex flex-col border-l border-pasture-600/50 bg-pasture-900/95 backdrop-blur-sm">
      {/* Header */}
      <div class="flex items-center justify-between px-3 py-2 border-b border-pasture-600/50">
        <div class="flex items-center gap-2">
          <span class="text-sm font-medium text-wool-200">Docs</span>
          <Show when={docs()}>
            <SourceBadge source={docs()!.source} />
          </Show>
        </div>
        <div class="flex items-center gap-1">
          <button
            type="button"
            onClick={handleExpand}
            class="p-1.5 rounded text-wool-500 hover:text-wool-300 hover:bg-pasture-800 transition-colors"
            title="Expand to full screen"
          >
            <Icon name="maximize-2" class="w-4 h-4" />
          </button>
          <button
            type="button"
            onClick={handleClose}
            class="p-1.5 rounded text-wool-500 hover:text-wool-300 hover:bg-pasture-800 transition-colors"
            title="Close docs panel"
          >
            <Icon name="x" class="w-4 h-4" />
          </button>
        </div>
      </div>

      {/* Loading state */}
      <Show when={docs.loading}>
        <div class="flex items-center justify-center py-8">
          <Icon name="loader-2" class="w-5 h-5 text-wool-500 animate-spin" />
        </div>
      </Show>

      {/* Empty state */}
      <Show when={!docs.loading && (!docs() || docs()!.files.length === 0)}>
        <div class="flex flex-col items-center justify-center py-8 px-4 text-center">
          <Icon name="file-text" class="w-8 h-8 text-wool-600 mb-2" />
          <p class="text-sm text-wool-500">No documentation found</p>
          <p class="text-xs text-wool-600 mt-1">
            Add markdown files to your project's docs folder
          </p>
        </div>
      </Show>

      {/* File list and preview */}
      <Show when={!docs.loading && docs() && docs()!.files.length > 0}>
        {/* File list */}
        <div class="border-b border-pasture-700/50">
          <For each={docs()!.files}>
            {(file) => (
              <button
                type="button"
                onClick={() => handleFileClick(file.name)}
                class="w-full flex items-center justify-between px-3 py-2 text-left transition-colors"
                classList={{
                  'bg-pasture-700/50 text-wool-100': localSelectedFile() === file.name,
                  'text-wool-400 hover:text-wool-200 hover:bg-pasture-800/50':
                    localSelectedFile() !== file.name,
                }}
              >
                <span class="text-sm truncate">{file.name.replace('.md', '')}</span>
                <Show when={localSelectedFile() === file.name}>
                  <Icon name="chevron-right" class="w-4 h-4 text-wool-500" />
                </Show>
              </button>
            )}
          </For>
        </div>

        {/* Markdown preview */}
        <div class="flex-1 overflow-auto p-4">
          <Show when={selectedFile()} fallback={
            <div class="text-sm text-wool-500 text-center py-4">
              Select a file to preview
            </div>
          }>
            <div class="prose prose-invert prose-sm max-w-none">
              <MarkdownPreview content={selectedFile()!.content} />
            </div>
          </Show>
        </div>
      </Show>
    </div>
  );
};

/** Source badge showing where docs come from */
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
      class={`inline-flex items-center gap-1 px-1.5 py-0.5 text-xs rounded border ${badgeClass()}`}
    >
      <Icon name={badgeIcon()} class="w-3 h-3" />
      <span class="truncate max-w-[100px]">{badgeText()}</span>
    </span>
  );
};

/** Simple markdown preview with basic formatting */
const MarkdownPreview: Component<{ content: string }> = (props) => {
  const renderMarkdown = () => {
    const lines = props.content.split('\n');
    const elements: string[] = [];
    let inCodeBlock = false;
    let codeBlockContent: string[] = [];
    let codeBlockLang = '';

    for (const line of lines) {
      // Code block handling
      if (line.startsWith('```')) {
        if (inCodeBlock) {
          elements.push(
            `<pre class="bg-pasture-800 rounded p-3 overflow-x-auto text-xs"><code>${escapeHtml(codeBlockContent.join('\n'))}</code></pre>`
          );
          codeBlockContent = [];
          inCodeBlock = false;
        } else {
          inCodeBlock = true;
          codeBlockLang = line.slice(3);
        }
        continue;
      }

      if (inCodeBlock) {
        codeBlockContent.push(line);
        continue;
      }

      // Headers
      if (line.startsWith('# ')) {
        elements.push(`<h1 class="text-lg font-bold text-wool-100 mt-4 mb-2">${escapeHtml(line.slice(2))}</h1>`);
        continue;
      }
      if (line.startsWith('## ')) {
        elements.push(`<h2 class="text-base font-semibold text-wool-200 mt-3 mb-2">${escapeHtml(line.slice(3))}</h2>`);
        continue;
      }
      if (line.startsWith('### ')) {
        elements.push(`<h3 class="text-sm font-medium text-wool-300 mt-2 mb-1">${escapeHtml(line.slice(4))}</h3>`);
        continue;
      }

      // Horizontal rule
      if (line.match(/^-{3,}$/) || line.match(/^\*{3,}$/)) {
        elements.push('<hr class="border-pasture-600 my-4" />');
        continue;
      }

      // List items
      if (line.match(/^[-*] /)) {
        elements.push(`<li class="text-wool-300 ml-4">${formatInlineMarkdown(line.slice(2))}</li>`);
        continue;
      }

      // Numbered list
      const numMatch = line.match(/^(\d+)\. /);
      if (numMatch) {
        elements.push(`<li class="text-wool-300 ml-4">${formatInlineMarkdown(line.slice(numMatch[0].length))}</li>`);
        continue;
      }

      // Empty line
      if (line.trim() === '') {
        elements.push('<br />');
        continue;
      }

      // Regular paragraph
      elements.push(`<p class="text-wool-300 my-1">${formatInlineMarkdown(line)}</p>`);
    }

    return elements.join('');
  };

  return <div innerHTML={renderMarkdown()} />;
};

/** Escape HTML special characters */
function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#039;');
}

/** Format inline markdown (bold, italic, code, links) */
function formatInlineMarkdown(text: string): string {
  let result = escapeHtml(text);

  // Inline code
  result = result.replace(/`([^`]+)`/g, '<code class="bg-pasture-800 px-1 rounded text-xs">$1</code>');

  // Bold
  result = result.replace(/\*\*([^*]+)\*\*/g, '<strong class="font-semibold text-wool-200">$1</strong>');

  // Italic
  result = result.replace(/\*([^*]+)\*/g, '<em class="italic">$1</em>');

  // Links
  result = result.replace(
    /\[([^\]]+)\]\(([^)]+)\)/g,
    '<a href="$2" class="text-sage-400 hover:underline" target="_blank" rel="noopener">$1</a>'
  );

  return result;
}
