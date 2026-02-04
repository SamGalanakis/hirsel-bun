/**
 * Project settings modal - Styled to match global SettingsModal
 */
import { invoke } from '@tauri-apps/api/core';
import {
  type Component,
  Show,
  createEffect,
  createSignal,
  onCleanup,
} from 'solid-js';
import type { ConfigDefaults, StartingPoint } from '../../lib/types';
import { useProject, useRoute } from '../../stores';
import { Icon, Dropdown, type DropdownOption } from '../shared';

export const ProjectSettings: Component = () => {
  const project = useProject();
  const route = useRoute();
  const [deleting, setDeleting] = createSignal(false);
  const [editingName, setEditingName] = createSignal(false);
  const [nameValue, setNameValue] = createSignal('');
  const [saving, setSaving] = createSignal(false);

  // Config defaults for showing inherited values
  const [configDefaults, setConfigDefaults] = createSignal<ConfigDefaults | null>(null);

  // Form values (None = use global default)
  const [workerScale, setWorkerScale] = createSignal<string>('');
  const [timeLimitMinutes, setTimeLimitMinutes] = createSignal<string>('');
  const [humanInTheLoop, setHumanInTheLoop] = createSignal<boolean>(true);
  const [runner, setRunner] = createSignal<string>('');
  const [targetBranch, setTargetBranch] = createSignal<string>('');

  // Track dirty state
  const [isDirty, setIsDirty] = createSignal(false);

  let nameInputRef: HTMLInputElement | undefined;

  const selectedProject = () => project.selectedProject();

  // Reset state and load config defaults when modal opens
  createEffect(() => {
    if (project.showProjectSettings()) {
      setEditingName(false);
      setIsDirty(false);
      loadConfigDefaults();
    }
  });

  const loadConfigDefaults = async () => {
    try {
      const defaults = await invoke<ConfigDefaults>('get_config_defaults');
      setConfigDefaults(defaults);
    } catch (e) {
      console.error('Failed to load config defaults:', e);
    }
  };

  // Sync form values when project changes
  createEffect(() => {
    const proj = selectedProject();
    if (proj) {
      setNameValue(proj.name);
      setWorkerScale(proj.workerScale || '');
      setTimeLimitMinutes(proj.timeLimitMinutes?.toString() || '');
      setHumanInTheLoop(proj.humanInTheLoop ?? true);
      setRunner(proj.runner || '');
      setTargetBranch(proj.targetBranch || '');
      setIsDirty(false);
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

  // Get placeholder text for inherited values
  const runnerPlaceholder = () => {
    const defaults = configDefaults();
    return defaults?.defaultRunner || 'local';
  };

  // Runner dropdown options
  const runnerOptions = (): DropdownOption[] => {
    const defaults = configDefaults();
    const options: DropdownOption[] = [
      { value: '', label: `Default (${runnerPlaceholder()})` },
    ];
    for (const name of defaults?.runners || []) {
      options.push({ value: name, label: name });
    }
    return options;
  };

  const handleClose = () => {
    if (deleting() || saving()) return;
    project.setShowProjectSettings(false);
  };

  const handleSaveName = async () => {
    const proj = selectedProject();
    const name = nameValue().trim();
    if (!proj || !name) return;

    const projectId = proj.id;

    try {
      await invoke('update_project_name', { projectId, name });
      await project.loadProjects();
      window.toast?.success('Project renamed');
      setEditingName(false);
    } catch (e) {
      console.error('Failed to rename project:', e);
      window.toast?.error(`Failed to rename: ${e}`);
    }
  };

  const handleSave = async () => {
    const proj = selectedProject();
    if (!proj) return;

    // Capture ALL values before any async operation
    const projectId = proj.id;
    const routeId = route.activeRoute()?.id ?? route.routes()[0]?.id ?? 0;
    const settings = {
      workerScale: workerScale() || null,
      timeLimitMinutes: timeLimitMinutes() ? Number.parseInt(timeLimitMinutes(), 10) : null,
      humanInTheLoop: humanInTheLoop(),
      runner: runner() || null,
      targetBranch: targetBranch() || null,
    };

    setSaving(true);
    try {
      await project.updateProjectSettings(projectId, routeId, settings);
      setSaving(false);
      window.toast?.success('Settings saved');
      // Delay close to let reactive updates from updateProjectSettings settle
      requestAnimationFrame(() => project.setShowProjectSettings(false));
    } catch (e) {
      console.error('Failed to save settings:', e);
      window.toast?.error(`Failed to save: ${e}`);
      setSaving(false);
    }
  };

  const handleDelete = async () => {
    const proj = selectedProject();
    if (!proj) return;

    const projectId = proj.id;
    const projectName = proj.name;

    let confirmed: boolean | undefined;
    try {
      confirmed = await window.confirmDialog?.delete(projectName, 'project');
    } catch (e) {
      console.error('[ProjectSettings] Confirm dialog error:', e);
      return;
    }
    if (!confirmed) return;

    setDeleting(true);

    try {
      await project.removeProject(projectId);
      project.setShowProjectSettings(false);
      window.toast?.success(`Project "${projectName}" deleted`);
    } catch (e) {
      console.error('[ProjectSettings] Delete failed:', e);
      window.toast?.error(`Failed to delete project: ${e}`);
      setDeleting(false);
    }
  };

  // Handle escape key to close
  createEffect(() => {
    if (!project.showProjectSettings()) return;

    const handleEscape = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      if (deleting() || saving()) return;

      if (editingName()) {
        const name = selectedProject()?.name || '';
        setEditingName(false);
        setNameValue(name);
      } else {
        handleClose();
      }
    };

    document.addEventListener('keydown', handleEscape);
    onCleanup(() => document.removeEventListener('keydown', handleEscape));
  });

  return (
    <Show when={project.showProjectSettings() && selectedProject()}>
      <div
        class="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
        onClick={(e) => {
          if (e.target === e.currentTarget) handleClose();
        }}
      >
        {/* Modal Panel - matches SettingsModal styling */}
        <div class="bg-pasture-800 border border-pasture-600 rounded-lg shadow-xl w-full max-w-lg mx-4 max-h-[85vh] flex flex-col">
          {/* Header */}
          <div class="px-6 py-4 border-b border-pasture-600 flex items-center justify-between shrink-0">
            <div class="flex items-center gap-3 flex-1 min-w-0">
              <div class="w-10 h-10 rounded-lg bg-pasture-700 flex items-center justify-center shrink-0">
                <Icon name="folder-cog" class="w-5 h-5 text-wool-400" />
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
                  <Icon name="pencil" class="w-3.5 h-3.5 text-wool-600 opacity-0 group-hover:opacity-100 transition-opacity shrink-0" />
                </button>
              </Show>
              <Show when={editingName()}>
                <input
                  ref={nameInputRef}
                  type="text"
                  class="input flex-1 text-lg font-semibold"
                  value={nameValue()}
                  onInput={(e) => setNameValue(e.currentTarget.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') handleSaveName();
                    if (e.key === 'Escape') {
                      const name = selectedProject()?.name || '';
                      setEditingName(false);
                      setNameValue(name);
                    }
                  }}
                  onBlur={() => {
                    const name = selectedProject()?.name || '';
                    if (nameValue().trim() && nameValue() !== name) {
                      handleSaveName();
                    } else {
                      setEditingName(false);
                      setNameValue(name);
                    }
                  }}
                />
              </Show>
            </div>

            <div class="flex items-center gap-1 shrink-0 ml-2">
              <button
                type="button"
                class="p-2 rounded-lg text-wool-500 hover:text-destructive hover:bg-destructive/10 transition-all"
                onClick={handleDelete}
                disabled={deleting() || saving()}
                title="Delete project"
              >
                <Show when={deleting()}>
                  <span class="spinner w-4 h-4" />
                </Show>
                <Show when={!deleting()}>
                  <Icon name="trash-2" class="w-4 h-4" />
                </Show>
              </button>
              <button
                type="button"
                class="p-2 rounded-lg text-wool-500 hover:text-wool-300 hover:bg-pasture-700 transition-all"
                onClick={handleClose}
                disabled={deleting() || saving()}
              >
                <Icon name="x" class="w-5 h-5" />
              </button>
            </div>
          </div>

          {/* Scrollable content */}
          <div class="overflow-y-auto flex-1 p-6 space-y-6">
            {/* Starting Point */}
            <Show when={startingPointInfo()}>
              <div>
                <h4 class="text-sm font-medium text-wool-200 mb-3">Starting Point</h4>
                <div class="bg-pasture-900 rounded-lg p-3 border border-pasture-700">
                  <div class="flex items-center gap-2 mb-1">
                    <Icon
                      name={startingPointInfo()?.icon || 'folder'}
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

            {/* Run Configuration Section */}
            <div>
              <h4 class="text-sm font-medium text-wool-200 mb-3 flex items-center gap-2">
                <Icon name="play" class="w-4 h-4 text-wool-500" />
                Run Configuration
              </h4>

              <div class="space-y-4">
                <div class="grid grid-cols-2 gap-4">
                  {/* Workers */}
                  <div>
                    <label class="block text-sm font-medium text-wool-300 mb-1.5">Workers</label>
                    <input
                      type="text"
                      class="input w-full"
                      placeholder={configDefaults()?.workerScale || '1'}
                      value={workerScale()}
                      onInput={(e) => {
                        setWorkerScale(e.currentTarget.value);
                        setIsDirty(true);
                      }}
                    />
                    <p class="text-xs text-muted-foreground mt-1">Number of parallel workers</p>
                  </div>

                  {/* Time Limit */}
                  <div>
                    <label class="block text-sm font-medium text-wool-300 mb-1.5">Time Limit (min)</label>
                    <input
                      type="text"
                      class="input w-full"
                      placeholder={configDefaults()?.timeLimitMinutes?.toString() || 'No limit'}
                      value={timeLimitMinutes()}
                      onInput={(e) => {
                        setTimeLimitMinutes(e.currentTarget.value);
                        setIsDirty(true);
                      }}
                    />
                    <p class="text-xs text-muted-foreground mt-1">Maximum run duration</p>
                  </div>

                  {/* Runner - Basecoat Dropdown */}
                  <div>
                    <label class="block text-sm font-medium text-wool-300 mb-1.5">Runner</label>
                    <Dropdown
                      value={runner()}
                      options={runnerOptions()}
                      onChange={(value) => {
                        setRunner(value);
                        setIsDirty(true);
                      }}
                      placeholder={`Default (${runnerPlaceholder()})`}
                    />
                    <p class="text-xs text-muted-foreground mt-1">Where workers execute</p>
                  </div>
                </div>

                {/* Human in the Loop Toggle - Basecoat Switch pattern */}
                <div role="group" class="field flex items-start justify-between rounded-lg border border-border p-4">
                  <div class="flex flex-col gap-0.5">
                    <label for="hitl-switch" class="font-medium leading-normal">Human in the Loop</label>
                    <p class="text-muted-foreground text-sm">Workers pause for approval on critical actions</p>
                  </div>
                  <input
                    id="hitl-switch"
                    type="checkbox"
                    role="switch"
                    checked={humanInTheLoop()}
                    onChange={(e) => {
                      setHumanInTheLoop(e.currentTarget.checked);
                      setIsDirty(true);
                    }}
                  />
                </div>
              </div>
            </div>

            {/* Delivery Section */}
            <div>
              <h4 class="text-sm font-medium text-wool-200 mb-3 flex items-center gap-2">
                <Icon name="git-merge" class="w-4 h-4 text-wool-500" />
                Delivery
              </h4>

              <div>
                <label class="block text-sm font-medium text-wool-300 mb-1.5">Target Branch</label>
                <input
                  type="text"
                  class="input w-full"
                  placeholder="main"
                  value={targetBranch()}
                  onInput={(e) => {
                    setTargetBranch(e.currentTarget.value);
                    setIsDirty(true);
                  }}
                />
                <p class="text-xs text-muted-foreground mt-1">Branch for PRs and merges</p>
              </div>
            </div>
          </div>

          {/* Footer with Save/Cancel */}
          <div class="px-6 py-4 border-t border-pasture-600 flex justify-end gap-2 shrink-0">
            <button
              type="button"
              class="btn btn-secondary"
              onClick={handleClose}
              disabled={deleting() || saving()}
            >
              Cancel
            </button>
            <button
              type="button"
              class="btn"
              onClick={handleSave}
              disabled={deleting() || saving() || !isDirty()}
            >
              {saving() ? 'Saving...' : 'Save'}
            </button>
          </div>
        </div>
      </div>
    </Show>
  );
};
