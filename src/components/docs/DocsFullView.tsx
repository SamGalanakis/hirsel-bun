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
import { useProject, useRoute } from '../../stores';
import { Icon } from '../shared';
import { MarkdownContent } from './MarkdownContent';

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
  const route = useRoute();
  const [activeTab, setActiveTab] = createSignal<string | null>(null);

  // Fetch docs
  const [docs, { refetch }] = createResource(
    () => {
      if (!project.docsFullScreen()) return null;
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
              <MarkdownContent content={activeFile()!.content} />
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
