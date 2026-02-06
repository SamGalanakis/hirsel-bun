/**
 * Project setup modal with path validation, autocomplete, and branch selection
 *
 * Displays as a centered modal dialog over the OneBoard canvas.
 */
import { invoke } from '../../lib/invoke';
import {
  type Component,
  For,
  Show,
  createEffect,
  createSignal,
  onCleanup,
  onMount,
} from 'solid-js';
import type { RepoValidation, StartingPoint } from '../../lib/types';
import { useProject } from '../../stores';
import { Icon } from '../shared';

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
  let nameInputRef: HTMLInputElement | undefined;

  // Focus name input on mount
  onMount(() => {
    nameInputRef?.focus();
  });

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
      const position = project.pendingProjectPosition();
      const result = await invoke<{ id: number; name: string }>('create_project', {
        name: name(),
        startingPoint: buildStartingPoint(),
        x: position?.x ?? null,
        y: position?.y ?? null,
      });
      project.setPendingProjectPosition(null);
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

  // Handle escape key to close
  const handleEscape = (e: KeyboardEvent) => {
    if (e.key === 'Escape' && !creating()) {
      project.cancelProjectSetup();
    }
  };

  createEffect(() => {
    document.addEventListener('keydown', handleEscape);
    onCleanup(() => document.removeEventListener('keydown', handleEscape));
  });

  // Cleanup timeout on unmount
  onCleanup(() => {
    if (validateTimeout) clearTimeout(validateTimeout);
  });

  return (
    <dialog
      open
      class="dialog fixed inset-0 z-50 m-0 h-full w-full max-w-none max-h-none bg-transparent flex items-center justify-center"
      style={{ 'backdrop-filter': 'blur(8px)' }}
      onClick={(e) => {
        if (e.target === e.currentTarget && !creating()) {
          project.cancelProjectSetup();
        }
      }}
    >
      {/* Modal Card */}
      <div class="card w-full max-w-xl mx-4">
        {/* Header */}
        <header>
          <div class="flex items-center gap-3">
            <div class="w-10 h-10 rounded-lg flex items-center justify-center bg-amber-500/15 border border-amber-500/30">
              <svg class="w-5 h-5 text-amber-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
              </svg>
            </div>
            <div>
              <h2 class="text-lg font-semibold text-wool-100">Create Project</h2>
              <p class="text-sm text-wool-500">Set up a new workspace</p>
            </div>
          </div>
          <button
            type="button"
            aria-label="Close"
            class="absolute top-4 right-4 p-2 rounded-lg text-wool-500 hover:text-wool-300 hover:bg-pasture-700 transition-all"
            onClick={() => project.cancelProjectSetup()}
            disabled={creating()}
          >
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </header>

        {/* Content */}
        <section>
          <form
            class="form grid gap-5"
            onSubmit={(e) => {
              e.preventDefault();
              handleCreate();
            }}
          >
          {/* Project Name */}
          <div class="grid gap-2">
            <label for="project-name" class="text-sm font-medium text-wool-300">
              Project Name
            </label>
            <input
              ref={nameInputRef}
              id="project-name"
              type="text"
              placeholder="my-project"
              class="input"
              value={name()}
              onInput={(e) => setName(e.currentTarget.value)}
            />
          </div>

          {/* Starting Point Type - Card Selection */}
          <div class="grid gap-3">
            <label class="text-sm font-medium text-wool-300">Starting Point</label>
            <div class="grid grid-cols-3 gap-3">
              {/* Local Folder */}
              <button
                type="button"
                class="p-4 rounded-lg text-left transition-all group"
                classList={{
                  'ring-2 ring-amber-500/50': startingPointType() === 'localFolder',
                }}
                style={{
                  background: startingPointType() === 'localFolder'
                    ? 'linear-gradient(180deg, rgba(212,165,116,0.15) 0%, rgba(212,165,116,0.05) 100%)'
                    : 'rgba(26, 26, 26, 0.5)',
                  border: startingPointType() === 'localFolder'
                    ? '1px solid rgba(212,165,116,0.3)'
                    : '1px solid rgba(63, 63, 70, 0.5)',
                }}
                onClick={() => setStartingPointType('localFolder')}
              >
                <Icon
                  name="folder"
                  class={`w-5 h-5 mb-2 ${startingPointType() === 'localFolder' ? 'text-amber-400' : 'text-wool-500 group-hover:text-wool-400'}`}
                />
                <div
                  class="text-sm font-medium"
                  classList={{
                    'text-amber-200': startingPointType() === 'localFolder',
                    'text-wool-300': startingPointType() !== 'localFolder',
                  }}
                >
                  Local Folder
                </div>
                <div class="text-xs text-wool-600 mt-0.5">Existing code</div>
              </button>

              {/* Git Repository */}
              <button
                type="button"
                class="p-4 rounded-lg text-left transition-all group"
                classList={{
                  'ring-2 ring-amber-500/50': startingPointType() === 'gitRepo',
                }}
                style={{
                  background: startingPointType() === 'gitRepo'
                    ? 'linear-gradient(180deg, rgba(212,165,116,0.15) 0%, rgba(212,165,116,0.05) 100%)'
                    : 'rgba(26, 26, 26, 0.5)',
                  border: startingPointType() === 'gitRepo'
                    ? '1px solid rgba(212,165,116,0.3)'
                    : '1px solid rgba(63, 63, 70, 0.5)',
                }}
                onClick={() => setStartingPointType('gitRepo')}
              >
                <Icon
                  name="git-branch"
                  class={`w-5 h-5 mb-2 ${startingPointType() === 'gitRepo' ? 'text-amber-400' : 'text-wool-500 group-hover:text-wool-400'}`}
                />
                <div
                  class="text-sm font-medium"
                  classList={{
                    'text-amber-200': startingPointType() === 'gitRepo',
                    'text-wool-300': startingPointType() !== 'gitRepo',
                  }}
                >
                  Git Repo
                </div>
                <div class="text-xs text-wool-600 mt-0.5">Clone remote</div>
              </button>

              {/* Greenfield */}
              <button
                type="button"
                class="p-4 rounded-lg text-left transition-all group"
                classList={{
                  'ring-2 ring-amber-500/50': startingPointType() === 'greenfield',
                }}
                style={{
                  background: startingPointType() === 'greenfield'
                    ? 'linear-gradient(180deg, rgba(212,165,116,0.15) 0%, rgba(212,165,116,0.05) 100%)'
                    : 'rgba(26, 26, 26, 0.5)',
                  border: startingPointType() === 'greenfield'
                    ? '1px solid rgba(212,165,116,0.3)'
                    : '1px solid rgba(63, 63, 70, 0.5)',
                }}
                onClick={() => setStartingPointType('greenfield')}
              >
                <Icon
                  name="sprout"
                  class={`w-5 h-5 mb-2 ${startingPointType() === 'greenfield' ? 'text-amber-400' : 'text-wool-500 group-hover:text-wool-400'}`}
                />
                <div
                  class="text-sm font-medium"
                  classList={{
                    'text-amber-200': startingPointType() === 'greenfield',
                    'text-wool-300': startingPointType() !== 'greenfield',
                  }}
                >
                  Greenfield
                </div>
                <div class="text-xs text-wool-600 mt-0.5">Start fresh</div>
              </button>
            </div>
          </div>

          {/* Local Folder Path */}
          <Show when={startingPointType() === 'localFolder'}>
            <div class="grid gap-2">
              <label for="local-path" class="text-sm font-medium text-wool-300">
                Folder Path
              </label>
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
                      <div
                        data-popover
                        role="listbox"
                        class="absolute z-10 w-full mt-1 max-h-48 overflow-auto"
                      >
                        <For each={filteredSuggestions()}>
                          {(suggestion, index) => (
                            <button
                              type="button"
                              class={`w-full px-3 py-2 text-left text-sm transition-colors ${
                                index() === highlightedIndex()
                                  ? 'bg-amber-500/10 text-amber-200'
                                  : 'text-wool-300 hover:bg-pasture-700'
                              }`}
                              onMouseDown={() => selectSuggestion(suggestion)}
                            >
                              <Icon name="folder" class="w-3.5 h-3.5 inline-block mr-2 text-wool-500" />
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
                    title="Browse folders"
                  >
                    <Icon name="folder-open" class="w-4 h-4" />
                  </button>
                </div>
              </div>
              {/* Validation feedback */}
              <Show when={validating()}>
                <p class="text-xs text-wool-500 flex items-center gap-2">
                  <span class="spinner w-3 h-3" />
                  Validating...
                </p>
              </Show>
              <Show when={!validating() && validation()}>
                <Show when={validation()?.valid}>
                  <p class="text-xs text-sage flex items-center gap-2">
                    <Icon name="check" class="w-3.5 h-3.5" />
                    {validation()?.needsGitInit
                      ? 'Folder exists (Git will be initialized)'
                      : validation()?.needsDirCreate
                        ? 'Folder will be created'
                        : 'Valid repository'}
                  </p>
                </Show>
                <Show when={!validation()?.valid && validation()?.error}>
                  <p class="text-xs text-terra flex items-center gap-2">
                    <Icon name="alert-circle" class="w-3.5 h-3.5" />
                    {validation()?.error}
                  </p>
                </Show>
              </Show>
            </div>
          </Show>

          {/* Git Repository URL */}
          <Show when={startingPointType() === 'gitRepo'}>
            <div class="grid gap-2">
              <label for="git-url" class="text-sm font-medium text-wool-300">
                Repository URL
              </label>
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
                <p class="text-xs text-wool-500 flex items-center gap-2">
                  <span class="spinner w-3 h-3" />
                  Validating...
                </p>
              </Show>
              <Show when={!validating() && validation()}>
                <Show when={validation()?.valid}>
                  <p class="text-xs text-sage flex items-center gap-2">
                    <Icon name="check" class="w-3.5 h-3.5" />
                    Valid repository
                  </p>
                </Show>
                <Show when={!validation()?.valid && validation()?.error}>
                  <p class="text-xs text-terra flex items-center gap-2">
                    <Icon name="alert-circle" class="w-3.5 h-3.5" />
                    {validation()?.error}
                  </p>
                </Show>
              </Show>
            </div>

            {/* Branch Selection */}
            <Show when={branches().length > 0}>
              <div class="grid gap-2">
                <label for="git-branch" class="text-sm font-medium text-wool-300">
                  Branch
                </label>
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
              </div>
            </Show>
          </Show>

          {/* Greenfield info */}
          <Show when={startingPointType() === 'greenfield'}>
            <div class="p-4 rounded-lg bg-amber-500/5 border border-amber-500/15">
              <div class="flex items-start gap-3">
                <Icon name="info" class="w-4 h-4 text-amber-500 mt-0.5 shrink-0" />
                <p class="text-sm text-wool-400">
                  A new empty workspace will be created for your project.
                  Perfect for brand new projects without existing code.
                </p>
              </div>
            </div>
          </Show>
          </form>
        </section>

        {/* Footer */}
        <footer>
          <button
            type="button"
            class="btn-ghost"
            onClick={() => project.cancelProjectSetup()}
            disabled={creating()}
          >
            Cancel
          </button>
          <button
            type="button"
            class="btn"
            disabled={!isValid() || creating()}
            onClick={handleCreate}
          >
            <Show when={creating()}>
              <span class="spinner w-4 h-4" />
            </Show>
            <Show when={!creating()}>
              <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
              </svg>
            </Show>
            Create Project
          </button>
        </footer>
      </div>
    </dialog>
  );
};
