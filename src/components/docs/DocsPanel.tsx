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
import { useProject, useRoute } from '../../stores';
import { Icon } from '../shared';
import { MarkdownContent } from './MarkdownContent';

interface DocFile {
  name: string;
  content: string;
}

interface ProjectDocsResponse {
  files: DocFile[];
}

export const DocsPanel: Component = () => {
  const project = useProject();
  const route = useRoute();
  const [localSelectedFile, setLocalSelectedFile] = createSignal<string | null>(null);

  // Fetch docs when panel is open
  const [docs, { refetch }] = createResource(
    () => {
      if (!project.docsOpen()) return null;
      const projectId = project.selectedProjectId();
      const routeId = route.activeRoute()?.id ?? route.routes()[0]?.id;
      if (!projectId || !routeId) return null;
      return { projectId, routeId };
    },
    async (params) => {
      if (!params) return null;
      try {
        return await invoke<ProjectDocsResponse>('get_project_docs', params);
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
        <span class="text-sm font-medium text-wool-200">Docs</span>
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
        {/* Horizontal tab bar */}
        <div class="flex items-center gap-1 px-2 py-1.5 border-b border-pasture-700/50 overflow-x-auto">
          <For each={docs()!.files}>
            {(file) => (
              <button
                type="button"
                onClick={() => handleFileClick(file.name)}
                class="flex items-center gap-1.5 px-2.5 py-1 rounded-t text-xs whitespace-nowrap transition-colors border-b-2"
                classList={{
                  'border-sage text-wool-100 bg-pasture-800/50': localSelectedFile() === file.name,
                  'border-transparent text-wool-500 hover:text-wool-300 hover:bg-pasture-800/30': localSelectedFile() !== file.name,
                }}
              >
                <Icon name="file-text" class="w-3 h-3" />
                <span>{file.name.replace('.md', '')}</span>
              </button>
            )}
          </For>
        </div>

        {/* Markdown preview */}
        <div class="flex-1 overflow-auto p-3">
          <Show when={selectedFile()} keyed>
            {(file) => <MarkdownContent content={file.content} compact />}
          </Show>
        </div>
      </Show>
    </div>
  );
};
