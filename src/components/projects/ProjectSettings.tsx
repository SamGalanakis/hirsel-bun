/**
 * Project settings panel with info display and delete functionality
 */
import { invoke } from '@tauri-apps/api/core';
import { type Component, Show, createSignal } from 'solid-js';
import type { StartingPoint } from '../../lib/types';
import { useProject } from '../../stores';

export const ProjectSettings: Component = () => {
  const project = useProject();
  const [deleting, setDeleting] = createSignal(false);

  const selectedProject = () => project.selectedProject();

  // Get starting point info
  const startingPointInfo = () => {
    const proj = selectedProject();
    if (!proj?.startingPoint) return null;
    const sp = proj.startingPoint as StartingPoint;
    if (sp.type === 'greenfield') {
      return { icon: 'sparkles', label: 'Greenfield', detail: 'Empty workspace' };
    }
    if (sp.type === 'localFolder') {
      return { icon: 'folder', label: 'Local Folder', detail: sp.path };
    }
    if (sp.type === 'gitRepo') {
      return {
        icon: 'git-branch',
        label: 'Git Repository',
        detail: `${sp.url}${sp.branch ? ` (${sp.branch})` : ''}`,
      };
    }
    return null;
  };

  const handleDelete = async () => {
    const proj = selectedProject();
    if (!proj) return;

    const confirmed = await window.confirmDialog?.delete(proj.name, 'project');
    if (!confirmed) return;

    setDeleting(true);
    try {
      await invoke('delete_project', { projectId: proj.id });
      window.dispatchEvent(new CustomEvent('project-deleted'));
      window.toast?.success(`Project "${proj.name}" deleted`);
    } catch (e) {
      console.error('Failed to delete project:', e);
      window.toast?.error(`Failed to delete project: ${e}`);
    } finally {
      setDeleting(false);
    }
  };

  return (
    <Show when={project.showProjectSettings()}>
      <div class="flex-1 flex flex-col p-6 overflow-auto">
        <div class="max-w-2xl mx-auto w-full">
          {/* Header */}
          <div class="flex items-center justify-between mb-6">
            <h2 class="text-xl font-medium text-wool-100">Project Settings</h2>
            <button
              onClick={() => project.setShowProjectSettings(false)}
              class="p-2 rounded hover:bg-pasture-700 text-wool-500 hover:text-wool-300"
            >
              <i data-lucide="x" class="w-4 h-4" />
            </button>
          </div>

          <div class="space-y-6">
            {/* Project Info */}
            <div class="card p-4 space-y-4">
              <h3 class="font-medium text-wool-200">Project Info</h3>

              <div class="grid gap-3">
                <div class="flex justify-between items-baseline">
                  <span class="text-sm text-wool-500">Name</span>
                  <span class="text-sm text-wool-200 font-medium">
                    {selectedProject()?.name}
                  </span>
                </div>

                <Show when={startingPointInfo()}>
                  <div class="flex justify-between items-start">
                    <span class="text-sm text-wool-500">Starting Point</span>
                    <div class="text-right max-w-xs">
                      <div class="flex items-center justify-end gap-1.5 text-wool-200">
                        <i
                          data-lucide={startingPointInfo()?.icon}
                          class="w-3.5 h-3.5 text-wool-400"
                        />
                        <span class="text-sm font-medium">
                          {startingPointInfo()?.label}
                        </span>
                      </div>
                      <Show when={startingPointInfo()?.detail}>
                        <p class="text-xs text-wool-500 mt-0.5 break-all">
                          {startingPointInfo()?.detail}
                        </p>
                      </Show>
                    </div>
                  </div>
                </Show>
              </div>
            </div>

            {/* Danger Zone */}
            <div class="border-t border-pasture-600 pt-6">
              <h3 class="font-medium text-terra mb-4">Danger Zone</h3>
              <div class="card border-terra/30 p-4">
                <div class="flex items-center justify-between">
                  <div>
                    <p class="text-sm font-medium text-wool-200">Delete Project</p>
                    <p class="text-xs text-wool-500 mt-0.5">
                      Permanently delete this project and all its data
                    </p>
                  </div>
                  <button
                    class="btn btn-destructive"
                    onClick={handleDelete}
                    disabled={deleting()}
                  >
                    <Show when={deleting()}>
                      <span class="spinner w-4 h-4" />
                    </Show>
                    <Show when={!deleting()}>
                      <i data-lucide="trash-2" class="w-4 h-4" />
                    </Show>
                    Delete
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </Show>
  );
};
