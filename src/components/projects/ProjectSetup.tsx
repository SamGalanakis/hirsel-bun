/**
 * Project setup form with path validation, autocomplete, and branch selection
 */
import { invoke } from '@tauri-apps/api/core';
import {
  type Component,
  For,
  Show,
  createEffect,
  createSignal,
  onCleanup,
} from 'solid-js';
import type { RepoValidation, StartingPoint } from '../../lib/types';
import { useProject } from '../../stores';

type StartingPointType = 'greenfield' | 'localFolder' | 'gitRepo';

export const ProjectSetup: Component = () => {
  const project = useProject();

  const [name, setName] = createSignal('');
  const [startingPointType, setStartingPointType] = createSignal<StartingPointType>('localFolder');
  const [localPath, setLocalPath] = createSignal('');
  const [gitUrl, setGitUrl] = createSignal('');
  const [gitBranch, setGitBranch] = createSignal('');
  const [suggestions, setSuggestions] = createSignal<string[]>([]);
  const [branches, setBranches] = createSignal<string[]>([]);
  const [validation, setValidation] = createSignal<RepoValidation | null>(null);
  const [validating, setValidating] = createSignal(false);
  const [creating, setCreating] = createSignal(false);
  const [highlightedIndex, setHighlightedIndex] = createSignal(-1);
  const [showSuggestions, setShowSuggestions] = createSignal(false);

  let validateTimeout: ReturnType<typeof setTimeout> | undefined;
  let pathInputRef: HTMLInputElement | undefined;

  // Load path suggestions on mount
  createEffect(() => {
    invoke<string[]>('suggest_paths')
      .then((paths) => setSuggestions(paths || []))
      .catch(() => setSuggestions([]));
  });

  // Debounced path validation
  const validatePath = (path: string) => {
    if (validateTimeout) clearTimeout(validateTimeout);
    setValidation(null);
    setBranches([]);

    if (!path.trim()) return;

    setValidating(true);
    validateTimeout = setTimeout(async () => {
      try {
        const result = await invoke<RepoValidation>('validate_repo', { path });
        setValidation(result);
        if (result.valid && result.branches.length > 0) {
          setBranches(result.branches);
          // Auto-select current branch or first branch
          if (result.currentBranch) {
            setGitBranch(result.currentBranch);
          } else if (result.urlBranch && result.urlBranchValid) {
            setGitBranch(result.urlBranch);
          } else if (result.branches.length > 0) {
            setGitBranch(result.branches[0]);
          }
        }
      } catch (e) {
        setValidation({ valid: false, error: String(e) } as RepoValidation);
      } finally {
        setValidating(false);
      }
    }, 500);
  };

  // Validate git URL
  const validateGitUrl = (url: string) => {
    if (validateTimeout) clearTimeout(validateTimeout);
    setValidation(null);
    setBranches([]);
    setGitBranch('');

    if (!url.trim()) return;

    setValidating(true);
    validateTimeout = setTimeout(async () => {
      try {
        const result = await invoke<RepoValidation>('validate_repo', { path: url });
        setValidation(result);
        if (result.valid && result.branches.length > 0) {
          setBranches(result.branches);
          // Auto-select branch from URL or default
          if (result.urlBranch && result.urlBranchValid) {
            setGitBranch(result.urlBranch);
          } else {
            const defaultBranch = result.branches.find(
              (b) => b === 'main' || b === 'master'
            );
            setGitBranch(defaultBranch || result.branches[0]);
          }
        }
      } catch (e) {
        setValidation({ valid: false, error: String(e) } as RepoValidation);
      } finally {
        setValidating(false);
      }
    }, 500);
  };

  // Handle path input change
  const handlePathInput = (value: string) => {
    setLocalPath(value);
    setHighlightedIndex(-1);
    validatePath(value);
  };

  // Handle git URL input change
  const handleGitUrlInput = (value: string) => {
    setGitUrl(value);
    validateGitUrl(value);
  };

  // Filter suggestions based on input
  const filteredSuggestions = () => {
    const input = localPath().toLowerCase();
    if (!input) return suggestions();
    return suggestions().filter((s) => s.toLowerCase().includes(input));
  };

  // Keyboard navigation for suggestions
  const handleKeyDown = (e: KeyboardEvent) => {
    const filtered = filteredSuggestions();
    if (!showSuggestions() || filtered.length === 0) return;

    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault();
        setHighlightedIndex((i) => Math.min(i + 1, filtered.length - 1));
        break;
      case 'ArrowUp':
        e.preventDefault();
        setHighlightedIndex((i) => Math.max(i - 1, 0));
        break;
      case 'Enter':
        if (highlightedIndex() >= 0) {
          e.preventDefault();
          selectSuggestion(filtered[highlightedIndex()]);
        }
        break;
      case 'Escape':
        setShowSuggestions(false);
        setHighlightedIndex(-1);
        break;
    }
  };

  // Select a suggestion
  const selectSuggestion = (path: string) => {
    setLocalPath(path);
    setShowSuggestions(false);
    setHighlightedIndex(-1);
    validatePath(path);
    pathInputRef?.focus();
  };

  // Open folder picker
  const handleBrowse = async () => {
    try {
      const selected = await invoke<string | null>('pick_folder');
      if (selected) {
        setLocalPath(selected);
        validatePath(selected);
      }
    } catch (e) {
      console.error('Failed to open folder picker:', e);
    }
  };

  // Build starting point from current state
  const buildStartingPoint = (): StartingPoint => {
    const type = startingPointType();
    if (type === 'greenfield') {
      return { type: 'greenfield' };
    }
    if (type === 'localFolder') {
      return { type: 'localFolder', path: localPath() };
    }
    const branch = gitBranch();
    return branch
      ? { type: 'gitRepo', url: gitUrl(), branch }
      : { type: 'gitRepo', url: gitUrl() };
  };

  // Form validation
  const isValid = () => {
    if (!name().trim()) return false;
    const type = startingPointType();
    if (type === 'greenfield') return true;
    if (type === 'localFolder') {
      const v = validation();
      return v?.valid || localPath().trim().length > 0;
    }
    if (type === 'gitRepo') {
      const v = validation();
      return v?.valid && gitBranch().length > 0;
    }
    return false;
  };

  // Create project
  const handleCreate = async () => {
    if (!isValid() || creating()) return;

    setCreating(true);
    try {
      const result = await invoke<{ id: number; name: string }>('create_project', {
        name: name(),
        startingPoint: buildStartingPoint(),
      });
      window.dispatchEvent(
        new CustomEvent('project-created', { detail: result })
      );
      window.toast?.success(`Project "${result.name}" created`);
    } catch (e) {
      console.error('Failed to create project:', e);
      window.toast?.error(`Failed to create project: ${e}`);
    } finally {
      setCreating(false);
    }
  };

  // Cleanup timeout on unmount
  onCleanup(() => {
    if (validateTimeout) clearTimeout(validateTimeout);
  });

  return (
    <div class="flex-1 flex items-center justify-center p-8">
      <div class="max-w-lg w-full">
        <h2 class="text-xl font-medium text-wool-100 mb-6">Create Project</h2>

        <form
          class="form grid gap-6"
          onSubmit={(e) => {
            e.preventDefault();
            handleCreate();
          }}
        >
          {/* Project Name */}
          <div class="grid gap-2">
            <label for="project-name">Project Name</label>
            <input
              id="project-name"
              type="text"
              placeholder="my-project"
              class="input"
              value={name()}
              onInput={(e) => setName(e.currentTarget.value)}
            />
            <p class="text-muted-foreground text-sm">
              A unique name for your project
            </p>
          </div>

          {/* Starting Point Type */}
          <div class="grid gap-3">
            <label>Starting Point</label>
            <div class="flex gap-4">
              <label class="flex items-center gap-2 cursor-pointer">
                <input
                  type="radio"
                  name="startingPointType"
                  value="greenfield"
                  checked={startingPointType() === 'greenfield'}
                  onChange={() => setStartingPointType('greenfield')}
                  class="radio"
                />
                <span class="text-sm">Greenfield</span>
              </label>
              <label class="flex items-center gap-2 cursor-pointer">
                <input
                  type="radio"
                  name="startingPointType"
                  value="localFolder"
                  checked={startingPointType() === 'localFolder'}
                  onChange={() => setStartingPointType('localFolder')}
                  class="radio"
                />
                <span class="text-sm">Local Folder</span>
              </label>
              <label class="flex items-center gap-2 cursor-pointer">
                <input
                  type="radio"
                  name="startingPointType"
                  value="gitRepo"
                  checked={startingPointType() === 'gitRepo'}
                  onChange={() => setStartingPointType('gitRepo')}
                  class="radio"
                />
                <span class="text-sm">Git Repository</span>
              </label>
            </div>
          </div>

          {/* Local Folder Path */}
          <Show when={startingPointType() === 'localFolder'}>
            <div class="grid gap-2">
              <label for="local-path">Folder Path</label>
              <div class="relative">
                <div class="flex gap-2">
                  <div class="relative flex-1">
                    <input
                      ref={pathInputRef}
                      id="local-path"
                      type="text"
                      placeholder="/path/to/project"
                      class="input w-full"
                      value={localPath()}
                      onInput={(e) => handlePathInput(e.currentTarget.value)}
                      onFocus={() => setShowSuggestions(true)}
                      onBlur={() => setTimeout(() => setShowSuggestions(false), 200)}
                      onKeyDown={handleKeyDown}
                    />
                    {/* Autocomplete dropdown */}
                    <Show when={showSuggestions() && filteredSuggestions().length > 0}>
                      <div class="absolute z-10 w-full mt-1 bg-pasture-800 border border-pasture-600 rounded-lg shadow-lg max-h-48 overflow-auto">
                        <For each={filteredSuggestions()}>
                          {(suggestion, index) => (
                            <button
                              type="button"
                              class={`w-full px-3 py-2 text-left text-sm hover:bg-pasture-700 ${
                                index() === highlightedIndex()
                                  ? 'bg-pasture-700'
                                  : ''
                              }`}
                              onMouseDown={() => selectSuggestion(suggestion)}
                            >
                              <i data-lucide="folder" class="w-3.5 h-3.5 inline-block mr-2 text-wool-500" />
                              {suggestion}
                            </button>
                          )}
                        </For>
                      </div>
                    </Show>
                  </div>
                  <button
                    type="button"
                    class="btn-outline px-3"
                    onClick={handleBrowse}
                  >
                    <i data-lucide="folder-open" class="w-4 h-4" />
                  </button>
                </div>
              </div>
              {/* Validation feedback */}
              <Show when={validating()}>
                <p class="text-sm text-wool-500 flex items-center gap-2">
                  <span class="spinner w-3 h-3" />
                  Validating...
                </p>
              </Show>
              <Show when={!validating() && validation()}>
                <Show when={validation()?.valid}>
                  <p class="text-sm text-sage flex items-center gap-2">
                    <i data-lucide="check" class="w-3.5 h-3.5" />
                    {validation()?.needsGitInit
                      ? 'Folder exists (Git will be initialized)'
                      : validation()?.needsDirCreate
                        ? 'Folder will be created'
                        : 'Valid repository'}
                  </p>
                </Show>
                <Show when={!validation()?.valid && validation()?.error}>
                  <p class="text-sm text-terra flex items-center gap-2">
                    <i data-lucide="alert-circle" class="w-3.5 h-3.5" />
                    {validation()?.error}
                  </p>
                </Show>
              </Show>
              <p class="text-muted-foreground text-sm">
                Local folder for your project workspace
              </p>
            </div>
          </Show>

          {/* Git Repository URL */}
          <Show when={startingPointType() === 'gitRepo'}>
            <div class="grid gap-2">
              <label for="git-url">Repository URL</label>
              <input
                id="git-url"
                type="text"
                placeholder="https://github.com/user/repo"
                class="input"
                value={gitUrl()}
                onInput={(e) => handleGitUrlInput(e.currentTarget.value)}
              />
              {/* Validation feedback */}
              <Show when={validating()}>
                <p class="text-sm text-wool-500 flex items-center gap-2">
                  <span class="spinner w-3 h-3" />
                  Validating...
                </p>
              </Show>
              <Show when={!validating() && validation()}>
                <Show when={validation()?.valid}>
                  <p class="text-sm text-sage flex items-center gap-2">
                    <i data-lucide="check" class="w-3.5 h-3.5" />
                    Valid repository
                  </p>
                </Show>
                <Show when={!validation()?.valid && validation()?.error}>
                  <p class="text-sm text-terra flex items-center gap-2">
                    <i data-lucide="alert-circle" class="w-3.5 h-3.5" />
                    {validation()?.error}
                  </p>
                </Show>
              </Show>
              <p class="text-muted-foreground text-sm">
                Git repository URL (HTTPS or SSH)
              </p>
            </div>

            {/* Branch Selection */}
            <Show when={branches().length > 0}>
              <div class="grid gap-2">
                <label for="git-branch">Branch</label>
                <select
                  id="git-branch"
                  class="select"
                  value={gitBranch()}
                  onChange={(e) => setGitBranch(e.currentTarget.value)}
                >
                  <For each={branches()}>
                    {(branch) => <option value={branch}>{branch}</option>}
                  </For>
                </select>
                <p class="text-muted-foreground text-sm">
                  Branch to check out
                </p>
              </div>
            </Show>
          </Show>

          {/* Greenfield info */}
          <Show when={startingPointType() === 'greenfield'}>
            <div class="p-4 bg-pasture-900 rounded-lg border border-pasture-600">
              <div class="flex items-start gap-3">
                <i data-lucide="sparkles" class="w-5 h-5 text-amber-500 mt-0.5" />
                <div>
                  <p class="text-sm text-wool-300 font-medium">Start from scratch</p>
                  <p class="text-sm text-wool-500 mt-1">
                    A new empty workspace will be created for your project.
                    Perfect for brand new projects.
                  </p>
                </div>
              </div>
            </div>
          </Show>

          {/* Action buttons */}
          <div class="flex gap-4">
            <button
              type="submit"
              class="btn"
              disabled={!isValid() || creating()}
            >
              <Show when={creating()}>
                <span class="spinner w-4 h-4" />
              </Show>
              <Show when={!creating()}>
                <i data-lucide="plus" class="w-4 h-4" />
              </Show>
              Create Project
            </button>
            <Show when={project.projects().length > 0}>
              <button
                type="button"
                class="btn-ghost"
                onClick={() => project.cancelProjectSetup()}
                disabled={creating()}
              >
                Cancel
              </button>
            </Show>
          </div>
        </form>
      </div>
    </div>
  );
};
