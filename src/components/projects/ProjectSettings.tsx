import { invoke } from '../../lib/invoke';
import { type Component, Show, createEffect, createSignal, onCleanup } from 'solid-js';
import type { StartingPoint } from '../../lib/types';
import { useProject, useRoute } from '../../stores';
import { Dropdown, Icon, ProjectIcon, type DropdownOption } from '../shared';

export const ProjectSettings: Component = () => {
  const project = useProject();
  const route = useRoute();

  const [deleting, setDeleting] = createSignal(false);
  const [editingName, setEditingName] = createSignal(false);
  const [nameValue, setNameValue] = createSignal('');
  const [description, setDescription] = createSignal('');
  const [timeLimitMinutes, setTimeLimitMinutes] = createSignal('');
  const [humanInTheLoop, setHumanInTheLoop] = createSignal(true);
  const [targetBranch, setTargetBranch] = createSignal('');
  const [defaultRepoId, setDefaultRepoId] = createSignal('');
  const [iconUrl, setIconUrl] = createSignal('');
  const [isDirty, setIsDirty] = createSignal(false);
  const [saving, setSaving] = createSignal(false);

  let nameInputRef: HTMLInputElement | undefined;

  const selectedProject = () => project.selectedProject();
  const selectedRoute = () => route.currentRoute();

  createEffect(() => {
    if (!project.showProjectSettings()) return;
    setEditingName(false);
    setIsDirty(false);
  });

  createEffect(() => {
    const currentProject = selectedProject();
    const currentRoute = selectedRoute();
    if (!currentProject || !currentRoute) return;

    setNameValue(currentProject.name);
    setDescription(currentProject.description || '');
    setIconUrl(currentProject.icon || '');
    setTimeLimitMinutes(currentRoute.timeLimitMinutes?.toString() || '');
    setHumanInTheLoop(currentRoute.humanInTheLoop ?? true);
    setTargetBranch(currentRoute.targetBranch || '');
    setDefaultRepoId(
      currentRoute.defaultRepoId?.toString() || currentRoute.repos[0]?.id?.toString() || ''
    );
    setIsDirty(false);
  });

  createEffect(() => {
    if (!editingName()) return;
    queueMicrotask(() => {
      nameInputRef?.focus();
      nameInputRef?.select();
    });
  });

  const routeOptions = (): DropdownOption[] =>
    route.routes().map((item) => ({
      value: item.id.toString(),
      label: item.name,
    }));

  const repoOptions = (): DropdownOption[] =>
    (selectedRoute()?.repos || []).map((repo) => ({
      value: repo.id.toString(),
      label: repo.name,
    }));

  const selectedDefaultRepo = () => {
    const currentRoute = selectedRoute();
    if (!currentRoute?.repos?.length) return null;

    const selectedId = Number.parseInt(defaultRepoId(), 10);
    return (
      currentRoute.repos.find((repo) => repo.id === selectedId) ||
      currentRoute.repos.find((repo) => repo.id === currentRoute.defaultRepoId) ||
      currentRoute.repos[0]
    );
  };

  const defaultRepoInfo = () => {
    const repo = selectedDefaultRepo();
    if (!repo) return null;

    const startingPoint = repo.startingPoint as StartingPoint;
    if (startingPoint.type === 'greenfield') {
      return { icon: 'sprout', label: repo.name, detail: 'Greenfield workspace' };
    }
    if (startingPoint.type === 'localFolder') {
      return { icon: 'folder', label: repo.name, detail: startingPoint.path };
    }
    if (startingPoint.type === 'gitRepo') {
      return {
        icon: 'git-branch',
        label: repo.name,
        detail: `${startingPoint.url}${startingPoint.branch ? ` @ ${startingPoint.branch}` : ''}`,
      };
    }
    return null;
  };

  const handleClose = () => {
    if (deleting() || saving()) return;
    project.setShowProjectSettings(false);
  };

  const handleSaveName = async () => {
    const currentProject = selectedProject();
    const nextName = nameValue().trim();
    if (!currentProject || !nextName) return;

    try {
      const updated = await invoke<{ id: number; name: string }>('update_project_name', {
        projectId: currentProject.id,
        name: nextName,
      });
      await project.loadProjects();
      project.selectProject({
        ...currentProject,
        ...updated,
      });
      window.toast?.success('Project renamed');
      setEditingName(false);
    } catch (error) {
      console.error('Failed to rename project:', error);
      window.toast?.error(`Failed to rename project: ${error}`);
    }
  };

  const handleSave = async () => {
    const currentProject = selectedProject();
    const currentRoute = selectedRoute();
    if (!currentProject || !currentRoute) return;

    const routeSettings = {
      timeLimitMinutes: timeLimitMinutes().trim()
        ? Number.parseInt(timeLimitMinutes().trim(), 10)
        : null,
      humanInTheLoop: humanInTheLoop(),
      targetBranch: targetBranch().trim() || null,
    };
    const nextDescription = description().trim() || null;
    const nextDefaultRepoId = defaultRepoId().trim() ? Number.parseInt(defaultRepoId(), 10) : null;
    const currentDefaultRepoId = currentRoute.defaultRepoId ?? currentRoute.repos[0]?.id ?? null;

    setSaving(true);
    try {
      const updatedProject = await project.updateProjectDescription(currentProject.id, nextDescription);
      if (!updatedProject) {
        throw new Error('Project details were not saved');
      }

      // Save icon if changed
      const nextIcon = iconUrl().trim() || null;
      if (nextIcon !== (currentProject.icon || null)) {
        await invoke<{ id: number }>('update_project_icon', {
          projectId: currentProject.id,
          icon: nextIcon,
        });
        await project.loadProjects();
      }

      const updatedRoute = await route.updateRouteSettings(currentRoute.id, routeSettings);
      if (!updatedRoute) {
        throw new Error('Route settings were not saved');
      }

      if (nextDefaultRepoId && nextDefaultRepoId !== currentDefaultRepoId) {
        const repoResult = await route.setDefaultRouteRepo(currentRoute.id, nextDefaultRepoId);
        if (!repoResult) {
          throw new Error('Default repo was not updated');
        }
      }
      setSaving(false);
      window.toast?.success('Project and route settings saved');
      requestAnimationFrame(() => project.setShowProjectSettings(false));
    } catch (error) {
      console.error('Failed to save settings:', error);
      window.toast?.error(`Failed to save settings: ${error}`);
      setSaving(false);
    }
  };

  const handleDelete = async () => {
    const currentProject = selectedProject();
    if (!currentProject) return;

    let confirmed: boolean | undefined;
    try {
      confirmed = await window.confirmDialog?.delete(currentProject.name, 'project');
    } catch (error) {
      console.error('[ProjectSettings] Confirm dialog error:', error);
      return;
    }
    if (!confirmed) return;

    setDeleting(true);
    try {
      await project.removeProject(currentProject.id);
      project.setShowProjectSettings(false);
      window.toast?.success(`Project "${currentProject.name}" deleted`);
    } catch (error) {
      console.error('[ProjectSettings] Delete failed:', error);
      window.toast?.error(`Failed to delete project: ${error}`);
      setDeleting(false);
    }
  };

  createEffect(() => {
    if (!project.showProjectSettings()) return;

    const handleEscape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || deleting() || saving()) return;

      if (editingName()) {
        setEditingName(false);
        setNameValue(selectedProject()?.name || '');
        return;
      }

      handleClose();
    };

    document.addEventListener('keydown', handleEscape);
    onCleanup(() => document.removeEventListener('keydown', handleEscape));
  });

  return (
    <Show when={project.showProjectSettings() && selectedProject()}>
      <div
        class="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
        onClick={(event) => {
          if (event.target === event.currentTarget) handleClose();
        }}
      >
        <div class="bg-pasture-800 border border-pasture-600 rounded-none shadow-xl w-full max-w-2xl mx-4 max-h-[85vh] flex flex-col">
          <div class="px-6 py-4 border-b border-pasture-600 flex items-center justify-between shrink-0">
            <div class="flex items-center gap-3 flex-1 min-w-0">
              <ProjectIcon
                name={selectedProject()?.name ?? ''}
                icon={selectedProject()?.icon}
                size={36}
              />

              <Show when={!editingName()}>
                <button
                  class="flex items-center gap-2 group min-w-0 text-left"
                  onClick={() => setEditingName(true)}
                >
                  <h2 class="text-lg font-semibold text-wool-100 truncate">
                    {selectedProject()?.name}
                  </h2>
                  <Icon
                    name="pencil"
                    class="w-3.5 h-3.5 text-wool-600 opacity-0 group-hover:opacity-100 transition-opacity shrink-0"
                  />
                </button>
              </Show>
              <Show when={editingName()}>
                <input
                  ref={nameInputRef}
                  type="text"
                  class="input flex-1 text-lg font-semibold"
                  value={nameValue()}
                  onInput={(event) => setNameValue(event.currentTarget.value)}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter') void handleSaveName();
                    if (event.key === 'Escape') {
                      setEditingName(false);
                      setNameValue(selectedProject()?.name || '');
                    }
                  }}
                  onBlur={() => {
                    const currentName = selectedProject()?.name || '';
                    if (nameValue().trim() && nameValue().trim() !== currentName) {
                      void handleSaveName();
                    } else {
                      setEditingName(false);
                      setNameValue(currentName);
                    }
                  }}
                />
              </Show>
            </div>

            <button
              type="button"
              class="p-2 rounded-none text-wool-500 hover:text-wool-300 hover:bg-pasture-700 transition-all"
              onClick={handleClose}
              disabled={deleting() || saving()}
            >
              <Icon name="x" class="w-5 h-5" />
            </button>
          </div>

          <div class="overflow-y-auto flex-1 p-6 space-y-6">
            <div class="grid gap-6 lg:grid-cols-[1.1fr_0.9fr]">
              <div class="space-y-6">
                <section>
                  <h4 class="text-sm font-medium text-wool-200 mb-3 flex items-center gap-2">
                    <Icon name="folder" class="w-4 h-4 text-wool-500" />
                    Project
                  </h4>
                  <div>
                    <label class="block text-sm font-medium text-wool-300 mb-1.5">
                      Description
                    </label>
                    <textarea
                      class="input w-full min-h-28 resize-y"
                      placeholder="Describe the project at a high level."
                      value={description()}
                      onInput={(event) => {
                        setDescription(event.currentTarget.value);
                        setIsDirty(true);
                      }}
                    />
                    <p class="text-xs text-muted-foreground mt-1">
                      Project-wide context. Route-specific runtime defaults live separately below.
                    </p>
                  </div>
                  <div>
                    <label class="block text-sm font-medium text-wool-300 mb-1.5">
                      Icon URL
                    </label>
                    <div class="flex items-center gap-2">
                      <ProjectIcon
                        name={selectedProject()?.name ?? ''}
                        icon={iconUrl() || null}
                        size={28}
                      />
                      <input
                        type="text"
                        class="input flex-1"
                        placeholder="https://example.com/favicon.ico"
                        value={iconUrl()}
                        onInput={(event) => {
                          setIconUrl(event.currentTarget.value);
                          setIsDirty(true);
                        }}
                      />
                      <Show when={iconUrl()}>
                        <button
                          type="button"
                          class="p-1 text-wool-500 hover:text-wool-300"
                          onClick={() => {
                            setIconUrl('');
                            setIsDirty(true);
                          }}
                          title="Clear icon"
                        >
                          <Icon name="x" class="w-3.5 h-3.5" />
                        </button>
                      </Show>
                    </div>
                    <p class="text-xs text-muted-foreground mt-1">
                      Favicon or avatar URL. Auto-detected from git remote on creation.
                    </p>
                  </div>
                </section>

                <section>
                  <h4 class="text-sm font-medium text-wool-200 mb-3 flex items-center gap-2">
                    <Icon name="git-branch" class="w-4 h-4 text-wool-500" />
                    Selected Route
                  </h4>
                  <div class="space-y-4">
                    <div>
                      <label class="block text-sm font-medium text-wool-300 mb-1.5">Route</label>
                      <Dropdown
                        value={selectedRoute()?.id?.toString() || ''}
                        options={routeOptions()}
                        onChange={(value) => void route.setActiveRoute(Number.parseInt(value, 10))}
                        placeholder="Choose route"
                      />
                      <p class="text-xs text-muted-foreground mt-1">
                        These settings apply only to the current route.
                      </p>
                    </div>

                    <div class="grid grid-cols-2 gap-4">
                      <div>
                        <label class="block text-sm font-medium text-wool-300 mb-1.5">
                          Time Limit (min)
                        </label>
                        <input
                          type="text"
                          class="input w-full"
                          placeholder="No limit"
                          value={timeLimitMinutes()}
                          onInput={(event) => {
                            setTimeLimitMinutes(event.currentTarget.value);
                            setIsDirty(true);
                          }}
                        />
                        <p class="text-xs text-muted-foreground mt-1">
                          Maximum runtime for route workers.
                        </p>
                      </div>

                      <div>
                        <label class="block text-sm font-medium text-wool-300 mb-1.5">
                          Target Branch
                        </label>
                        <input
                          type="text"
                          class="input w-full"
                          placeholder="main"
                          value={targetBranch()}
                          onInput={(event) => {
                            setTargetBranch(event.currentTarget.value);
                            setIsDirty(true);
                          }}
                        />
                        <p class="text-xs text-muted-foreground mt-1">
                          Delivery branch for pushes, PRs, and merges.
                        </p>
                      </div>
                    </div>

                    <div
                      role="group"
                      class="field flex items-start justify-between rounded-none border border-border p-4"
                    >
                      <div class="flex flex-col gap-0.5">
                        <label for="hitl-switch" class="font-medium leading-normal">
                          Human in the Loop
                        </label>
                        <p class="text-muted-foreground text-sm">
                          Pause when worker concerns require approval or a decision.
                        </p>
                      </div>
                      <input
                        id="hitl-switch"
                        type="checkbox"
                        role="switch"
                        checked={humanInTheLoop()}
                        onChange={(event) => {
                          setHumanInTheLoop(event.currentTarget.checked);
                          setIsDirty(true);
                        }}
                      />
                    </div>
                  </div>
                </section>
              </div>

              <div class="space-y-6">
                <section>
                  <h4 class="text-sm font-medium text-wool-200 mb-3 flex items-center gap-2">
                    <Icon name="book-open" class="w-4 h-4 text-wool-500" />
                    Repositories on This Route
                  </h4>
                  <div class="space-y-4">
                    <Show when={(selectedRoute()?.repos?.length || 0) > 0}>
                      <div>
                        <label class="block text-sm font-medium text-wool-300 mb-1.5">
                          Default Repo
                        </label>
                        <Dropdown
                          value={defaultRepoId()}
                          options={repoOptions()}
                          onChange={(value) => {
                            setDefaultRepoId(value);
                            setIsDirty(true);
                          }}
                          placeholder="Choose repo"
                        />
                      </div>
                    </Show>

                    <Show when={defaultRepoInfo()}>
                      <div class="bg-pasture-900 rounded-none p-3 border border-pasture-700">
                        <div class="flex items-center gap-2 mb-1">
                          <Icon
                            name={defaultRepoInfo()?.icon || 'folder'}
                            class="w-4 h-4 text-amber-400/70"
                          />
                          <span class="text-sm font-medium text-wool-200">
                            {defaultRepoInfo()?.label}
                          </span>
                        </div>
                        <Show when={defaultRepoInfo()?.detail}>
                          <p class="text-xs text-wool-500 pl-6 break-all font-mono">
                            {defaultRepoInfo()?.detail}
                          </p>
                        </Show>
                      </div>
                    </Show>
                  </div>
                </section>

                <section>
                  <h4 class="text-sm font-medium text-wool-200 mb-3 flex items-center gap-2">
                    <Icon name="trash-2" class="w-4 h-4 text-wool-500" />
                    Danger Zone
                  </h4>
                  <div class="border border-destructive/30 bg-destructive/5 p-4 space-y-3">
                    <p class="text-sm text-wool-300">
                      Delete the project and all associated routes, workers, artifacts, and history.
                    </p>
                    <button
                      type="button"
                      class="btn btn-secondary text-destructive border-destructive/40 hover:bg-destructive/10"
                      onClick={handleDelete}
                      disabled={deleting() || saving()}
                    >
                      {deleting() ? 'Deleting…' : 'Delete project'}
                    </button>
                  </div>
                </section>
              </div>
            </div>
          </div>

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
              onClick={() => void handleSave()}
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
