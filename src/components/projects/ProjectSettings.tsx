/**
 * Project settings modal - Info display, editing, and delete functionality
 *
 * Displays as a centered modal dialog matching ProjectSetup style.
 */
import { invoke } from '@tauri-apps/api/core';
import {
  type Component,
  Show,
  createEffect,
  createSignal,
  onCleanup,
  onMount,
} from 'solid-js';
import type { StartingPoint } from '../../lib/types';
import { useProject } from '../../stores';
import { initLucideIcons } from '../../lib/icons';

export const ProjectSettings: Component = () => {
  const project = useProject();
  const [deleting, setDeleting] = createSignal(false);
  const [editingName, setEditingName] = createSignal(false);
  const [nameValue, setNameValue] = createSignal('');

  let nameInputRef: HTMLInputElement | undefined;

  const selectedProject = () => project.selectedProject();

  // Initialize icons on mount and when content changes
  onMount(() => {
    initLucideIcons();
  });

  createEffect(() => {
    if (project.showProjectSettings()) {
      queueMicrotask(initLucideIcons);
    }
  });

  // Sync name value when project changes
  createEffect(() => {
    const proj = selectedProject();
    if (proj) {
      setNameValue(proj.name);
    }
  });

  // Focus input when editing starts
  createEffect(() => {
    if (editingName()) {
      queueMicrotask(() => {
        nameInputRef?.focus();
        nameInputRef?.select();
      });
    }
  });

  // Get starting point info
  const startingPointInfo = () => {
    const proj = selectedProject();
    if (!proj?.startingPoint) return null;
    const sp = proj.startingPoint as StartingPoint;
    if (sp.type === 'greenfield') {
      return { icon: 'sprout', label: 'Greenfield', detail: 'Empty workspace' };
    }
    if (sp.type === 'localFolder') {
      return { icon: 'folder', label: 'Local Folder', detail: sp.path };
    }
    if (sp.type === 'gitRepo') {
      return {
        icon: 'git-branch',
        label: 'Git Repository',
        detail: `${sp.url}${sp.branch ? ` @ ${sp.branch}` : ''}`,
      };
    }
    return null;
  };

  const handleClose = () => {
    if (!deleting()) {
      setEditingName(false);
      project.setShowProjectSettings(false);
    }
  };

  const handleSaveName = async () => {
    const proj = selectedProject();
    if (!proj || !nameValue().trim()) return;

    try {
      await invoke('update_project_name', {
        projectId: proj.id,
        name: nameValue().trim(),
      });
      await project.loadProjects();
      window.toast?.success('Project renamed');
      setEditingName(false);
    } catch (e) {
      console.error('Failed to rename project:', e);
      window.toast?.error(`Failed to rename: ${e}`);
    }
  };

  const handleDelete = async () => {
    const proj = selectedProject();
    if (!proj) return;

    const confirmed = await window.confirmDialog?.delete(proj.name, 'project');
    if (!confirmed) return;

    setDeleting(true);
    try {
      await invoke('delete_project', { projectId: proj.id });
      project.setShowProjectSettings(false);
      window.dispatchEvent(new CustomEvent('project-deleted'));
      window.toast?.success(`Project "${proj.name}" deleted`);
    } catch (e) {
      console.error('Failed to delete project:', e);
      window.toast?.error(`Failed to delete project: ${e}`);
    } finally {
      setDeleting(false);
    }
  };

  // Handle escape key to close
  const handleEscape = (e: KeyboardEvent) => {
    if (e.key === 'Escape' && !deleting()) {
      if (editingName()) {
        setEditingName(false);
        setNameValue(selectedProject()?.name || '');
      } else {
        handleClose();
      }
    }
  };

  createEffect(() => {
    if (project.showProjectSettings()) {
      document.addEventListener('keydown', handleEscape);
      onCleanup(() => document.removeEventListener('keydown', handleEscape));
    }
  });

  return (
    <Show when={project.showProjectSettings() && selectedProject()}>
      <div
        class="absolute inset-0 flex items-center justify-center z-50"
        style={{
          background: 'rgba(15, 15, 15, 0.8)',
          'backdrop-filter': 'blur(8px)',
        }}
        onClick={(e) => {
          if (e.target === e.currentTarget) {
            handleClose();
          }
        }}
      >
        {/* Modal Card */}
        <div
          class="w-full max-w-md mx-4 rounded-xl overflow-hidden shadow-2xl"
          style={{
            background: 'linear-gradient(180deg, rgba(36, 36, 36, 0.98) 0%, rgba(26, 26, 26, 0.98) 100%)',
            border: '1px solid rgba(63, 63, 70, 0.6)',
            'box-shadow': '0 24px 64px rgba(0, 0, 0, 0.5), 0 0 0 1px rgba(255,255,255,0.03) inset',
          }}
        >
          {/* Header with editable name */}
          <div
            class="px-6 py-5 flex items-center justify-between"
            style={{
              'border-bottom': '1px solid rgba(63, 63, 70, 0.4)',
              background: 'linear-gradient(180deg, rgba(255,255,255,0.02) 0%, transparent 100%)',
            }}
          >
            <div class="flex items-center gap-3 flex-1 min-w-0">
              <div
                class="w-10 h-10 rounded-lg flex items-center justify-center shrink-0"
                style={{
                  background: 'linear-gradient(145deg, rgba(138,133,128,0.15) 0%, rgba(138,133,128,0.05) 100%)',
                  border: '1px solid rgba(138,133,128,0.2)',
                }}
              >
                <i data-lucide="settings" class="w-5 h-5 text-wool-400" />
              </div>

              {/* Editable project name */}
              <Show when={!editingName()}>
                <button
                  class="flex items-center gap-2 group min-w-0 text-left"
                  onClick={() => setEditingName(true)}
                >
                  <h2 class="text-lg font-semibold text-wool-100 truncate">
                    {selectedProject()?.name}
                  </h2>
                  <i data-lucide="pencil" class="w-3.5 h-3.5 text-wool-600 opacity-0 group-hover:opacity-100 transition-opacity shrink-0" />
                </button>
              </Show>
              <Show when={editingName()}>
                <div class="flex items-center gap-2 flex-1">
                  <input
                    ref={nameInputRef}
                    type="text"
                    class="flex-1 bg-pasture-900 border border-pasture-600 rounded-lg px-3 py-1.5 text-lg font-semibold text-wool-100 focus:border-amber-500/50 focus:outline-none"
                    value={nameValue()}
                    onInput={(e) => setNameValue(e.currentTarget.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') handleSaveName();
                      if (e.key === 'Escape') {
                        setEditingName(false);
                        setNameValue(selectedProject()?.name || '');
                      }
                    }}
                    onBlur={() => {
                      // Save on blur if changed, otherwise cancel
                      if (nameValue().trim() && nameValue() !== selectedProject()?.name) {
                        handleSaveName();
                      } else {
                        setEditingName(false);
                        setNameValue(selectedProject()?.name || '');
                      }
                    }}
                  />
                </div>
              </Show>
            </div>

            <button
              type="button"
              class="p-2 rounded-lg text-wool-500 hover:text-wool-300 hover:bg-pasture-700 transition-all shrink-0 ml-2"
              onClick={handleClose}
              disabled={deleting()}
            >
              <i data-lucide="x" class="w-5 h-5" />
            </button>
          </div>

          {/* Content - Starting Point info */}
          <Show when={startingPointInfo()}>
            <div class="p-6">
              <label class="text-xs font-medium text-wool-500 uppercase tracking-wider">
                Starting Point
              </label>
              <div
                class="mt-2 p-3 rounded-lg"
                style={{ background: 'rgba(0,0,0,0.2)' }}
              >
                <div class="flex items-center gap-2 mb-1">
                  <i
                    data-lucide={startingPointInfo()?.icon}
                    class="w-4 h-4 text-amber-400/70"
                  />
                  <span class="text-sm font-medium text-wool-200">
                    {startingPointInfo()?.label}
                  </span>
                </div>
                <Show when={startingPointInfo()?.detail}>
                  <p class="text-xs text-wool-500 pl-6 break-all font-mono">
                    {startingPointInfo()?.detail}
                  </p>
                </Show>
              </div>
            </div>
          </Show>

          {/* Footer / Danger Zone */}
          <div
            class="px-6 py-4"
            style={{
              'border-top': '1px solid rgba(196, 92, 74, 0.2)',
              background: 'linear-gradient(180deg, rgba(196, 92, 74, 0.05) 0%, rgba(196, 92, 74, 0.02) 100%)',
            }}
          >
            <div class="flex items-center justify-between">
              <div>
                <p class="text-sm font-medium text-terra">Delete Project</p>
                <p class="text-xs text-wool-600 mt-0.5">
                  This action cannot be undone
                </p>
              </div>
              <button
                type="button"
                class="px-4 py-2 rounded-lg text-sm font-medium transition-all flex items-center gap-2"
                style={{
                  background: 'linear-gradient(180deg, rgba(196, 92, 74, 0.2) 0%, rgba(196, 92, 74, 0.1) 100%)',
                  border: '1px solid rgba(196, 92, 74, 0.3)',
                  color: 'rgb(212, 120, 106)',
                }}
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
    </Show>
  );
};
