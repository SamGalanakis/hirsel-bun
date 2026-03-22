/**
 * Project setup modal with two-step flow:
 * 1) Project details
 * 2) One or more repository cards
 */
import { invoke } from '../../lib/invoke';
import { emit } from '../../lib/events';
import { type Component, For, Show, createEffect, createSignal, onCleanup, onMount } from 'solid-js';
import type { RepoValidation, StartingPoint } from '../../lib/types';
import { useProject } from '../../stores';
import { Icon } from '../shared';
import { amber } from '../../lib/theme-colors';

type StartingPointType = 'localFolder' | 'gitRepo';
type SetupStep = 'details' | 'repos';

interface RepoDraft {
  id: string;
  type: StartingPointType;
  localPath: string;
  gitUrl: string;
  gitBranch: string;
  branches: string[];
  validation: RepoValidation | null;
  validating: boolean;
}

let repoCounter = 1;

const looksLikeRemoteRepo = (value: string) =>
  /^(https?:\/\/|ssh:\/\/|git:\/\/|git@|github\.com\/|gitlab\.com\/|bitbucket\.org\/)/i.test(
    value.trim()
  );

const createRepoDraft = (): RepoDraft => ({
  id: `repo-${repoCounter++}`,
  type: 'localFolder',
  localPath: '',
  gitUrl: '',
  gitBranch: 'main',
  branches: [],
  validation: null,
  validating: false,
});

export const ProjectSetup: Component = () => {
  const project = useProject();

  const [step, setStep] = createSignal<SetupStep>('details');
  const [name, setName] = createSignal('');
  const [repos, setRepos] = createSignal<RepoDraft[]>([createRepoDraft()]);
  const [defaultRepoId, setDefaultRepoId] = createSignal(repos()[0].id);
  const [creating, setCreating] = createSignal(false);

  let nameInputRef: HTMLInputElement | undefined;
  const validateTimeouts = new Map<string, ReturnType<typeof setTimeout>>();

  onMount(() => {
    nameInputRef?.focus();
  });

  createEffect(() => {
    if (step() === 'details') {
      setTimeout(() => nameInputRef?.focus(), 0);
    }
  });

  const clearValidationTimeout = (repoId: string) => {
    const timeout = validateTimeouts.get(repoId);
    if (!timeout) return;
    clearTimeout(timeout);
    validateTimeouts.delete(repoId);
  };

  const updateRepo = (repoId: string, updater: (repo: RepoDraft) => RepoDraft) => {
    setRepos((prev) => prev.map((repo) => (repo.id === repoId ? updater(repo) : repo)));
  };

  const validatePath = (repoId: string, path: string) => {
    clearValidationTimeout(repoId);
    updateRepo(repoId, (repo) => ({
      ...repo,
      validation: null,
      branches: [],
      validating: false,
    }));

    if (!path.trim()) return;

    updateRepo(repoId, (repo) => ({ ...repo, validating: true }));
    const timeout = setTimeout(async () => {
      try {
        const result = await invoke<RepoValidation>('validate_repo', { path });
        updateRepo(repoId, (repo) => {
          const next: RepoDraft = {
            ...repo,
            validation: result,
            branches: result.branches || [],
            validating: false,
          };
          if (result.valid) {
            if (result.currentBranch) {
              next.gitBranch = result.currentBranch;
            } else if (!next.gitBranch.trim()) {
              next.gitBranch = 'main';
            }
          }

          return next;
        });
      } catch (e) {
        updateRepo(repoId, (repo) => ({
          ...repo,
          validation: { valid: false, error: String(e) } as RepoValidation,
          validating: false,
        }));
      } finally {
        validateTimeouts.delete(repoId);
      }
    }, 500);

    validateTimeouts.set(repoId, timeout);
  };

  const validateGitUrl = (repoId: string, url: string) => {
    clearValidationTimeout(repoId);
    updateRepo(repoId, (repo) => ({
      ...repo,
      validation: null,
      branches: [],
      gitBranch: repo.gitBranch || 'main',
      validating: false,
    }));

    if (!url.trim()) return;

    updateRepo(repoId, (repo) => ({ ...repo, validating: true }));
    const timeout = setTimeout(async () => {
      try {
        const result = await invoke<RepoValidation>('validate_repo', { path: url });
        updateRepo(repoId, (repo) => {
          const next: RepoDraft = {
            ...repo,
            validation: result,
            branches: result.branches || [],
            validating: false,
          };

          if (result.valid && result.urlBranch && result.urlBranchValid) {
            next.gitBranch = result.urlBranch;
          } else if (repo.gitBranch && result.branches.includes(repo.gitBranch)) {
            next.gitBranch = repo.gitBranch;
          } else if (!repo.gitBranch.trim()) {
            next.gitBranch = 'main';
          }

          return next;
        });
      } catch (e) {
        updateRepo(repoId, (repo) => ({
          ...repo,
          validation: { valid: false, error: String(e) } as RepoValidation,
          validating: false,
        }));
      } finally {
        validateTimeouts.delete(repoId);
      }
    }, 500);

    validateTimeouts.set(repoId, timeout);
  };

  const handleBrowse = async (repoId: string) => {
    try {
      const selected = await invoke<string | null>('pick_folder');
      if (!selected) return;
      updateRepo(repoId, (repo) => ({
        ...repo,
        type: 'localFolder',
        localPath: selected,
        gitUrl: '',
      }));
      validatePath(repoId, selected);
    } catch (e) {
      console.error('Failed to open folder picker:', e);
    }
  };

  const addRepo = () => {
    const repo = createRepoDraft();
    setRepos((prev) => [...prev, repo]);
  };

  const removeRepo = (repoId: string) => {
    const current = repos();
    if (current.length <= 1) return;

    clearValidationTimeout(repoId);
    const next = current.filter((repo) => repo.id !== repoId);
    setRepos(next);

    if (defaultRepoId() === repoId) {
      setDefaultRepoId(next[0].id);
    }
  };

  const buildStartingPoint = (repo: RepoDraft): StartingPoint => {
    if (repo.type === 'localFolder') {
      return { type: 'localFolder', path: repo.localPath };
    }

    return repo.gitBranch
      ? { type: 'gitRepo', url: repo.gitUrl, branch: repo.gitBranch }
      : { type: 'gitRepo', url: repo.gitUrl };
  };

  const isRepoValid = (repo: RepoDraft) => {
    if (repo.type === 'localFolder') {
      if (looksLikeRemoteRepo(repo.localPath)) return false;
      return repo.validation?.valid || repo.localPath.trim().length > 0;
    }

    if (repo.type === 'gitRepo') {
      if (!looksLikeRemoteRepo(repo.gitUrl)) return false;
      return !!repo.validation?.valid;
    }

    return false;
  };

  const canContinue = () => name().trim().length > 0;
  const canCreate = () => canContinue() && repos().length > 0 && repos().every(isRepoValid);

  const goToReposStep = () => {
    if (!canContinue()) return;
    setStep('repos');
  };

  const handleCreate = async () => {
    if (!canCreate() || creating()) return;

    setCreating(true);
    try {
      const repoList = repos();
      const defaultRepoIndex = Math.max(
        0,
        repoList.findIndex((repo) => repo.id === defaultRepoId())
      );

      const position = project.pendingProjectPosition();
      const result = await invoke<{ id: number; name: string }>('create_project', {
        name: name().trim(),
        repos: repoList.map((repo) => ({
          startingPoint: buildStartingPoint(repo),
          targetBranch: repo.gitBranch.trim() || null,
        })),
        defaultRepoIndex,
        x: position?.x ?? null,
        y: position?.y ?? null,
      });

      project.setPendingProjectPosition(null);
      emit('project-created', result);
      window.toast?.success(`Project "${result.name}" created`);
    } catch (e) {
      console.error('Failed to create project:', e);
      window.toast?.error(`Failed to create project: ${e}`);
    } finally {
      setCreating(false);
    }
  };

  const handleEscape = (e: KeyboardEvent) => {
    if (e.key === 'Escape' && !creating()) {
      project.cancelProjectSetup();
    }
  };

  createEffect(() => {
    document.addEventListener('keydown', handleEscape);
    onCleanup(() => document.removeEventListener('keydown', handleEscape));
  });

  onCleanup(() => {
    for (const timeout of validateTimeouts.values()) {
      clearTimeout(timeout);
    }
    validateTimeouts.clear();
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
      <div class="card w-full max-w-3xl mx-4">
        <header>
          <div class="flex items-center gap-3">
            <div class="w-10 h-10 rounded-none flex items-center justify-center bg-amber-500/15 border border-amber-500/30">
              <svg class="w-5 h-5 text-amber-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
              </svg>
            </div>
            <div>
              <h2 class="text-lg font-semibold text-wool-100">Create Project</h2>
              <p class="text-sm text-wool-500">
                Step {step() === 'details' ? '1' : '2'} of 2
                <Show when={step() === 'details'}> · Project details</Show>
                <Show when={step() === 'repos'}> · Add repositories</Show>
              </p>
            </div>
          </div>
          <button
            type="button"
            aria-label="Close"
            class="absolute top-4 right-4 p-2 rounded-none text-wool-500 hover:text-wool-300 hover:bg-pasture-700 transition-all"
            onClick={() => project.cancelProjectSetup()}
            disabled={creating()}
          >
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </header>

        <section>
          <form
            class="form grid gap-5"
            onSubmit={(e) => {
              e.preventDefault();
              if (step() === 'details') {
                goToReposStep();
                return;
              }
              handleCreate();
            }}
          >
            <Show when={step() === 'details'}>
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
                <p class="text-xs text-wool-500">
                  Next, you will add one or more repositories for this project.
                </p>
              </div>
            </Show>

            <Show when={step() === 'repos'}>
              <div class="flex items-center justify-between">
                <div>
                  <h3 class="text-sm font-semibold text-wool-200">Repositories</h3>
                  <p class="text-xs text-wool-500 mt-1">
                    One repository is prefilled. Choose local path or remote repo URL.
                  </p>
                </div>
                <button type="button" class="btn-outline" onClick={addRepo}>
                  <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
                  </svg>
                  Add Repo
                </button>
              </div>

              <div class="grid gap-4 max-h-[52vh] overflow-y-auto pr-1">
                <For each={repos()}>
                  {(repo, index) => (
                    <div
                      class="rounded-none p-4 grid gap-4"
                      style={{
                        background:
                          defaultRepoId() === repo.id
                            ? `linear-gradient(180deg, ${amber(0.1)} 0%, ${amber(0.03)} 100%)`
                            : 'rgba(26, 26, 26, 0.45)',
                        border:
                          defaultRepoId() === repo.id
                            ? `1px solid ${amber(0.25)}`
                            : '1px solid rgba(63, 63, 70, 0.55)',
                      }}
                    >
                      <Show when={repos().length > 1}>
                        <div class="flex items-center justify-between gap-4">
                          <label class="inline-flex items-center gap-2 text-xs text-wool-400 cursor-pointer">
                            <input
                              type="radio"
                              name="default-repo"
                              class="radio radio-sm"
                              checked={defaultRepoId() === repo.id}
                              onChange={() => setDefaultRepoId(repo.id)}
                            />
                            Use as default
                          </label>
                          <button
                            type="button"
                            class="btn-ghost px-2 py-1 text-xs"
                            onClick={() => removeRepo(repo.id)}
                            title="Remove repo"
                          >
                            Remove
                          </button>
                        </div>
                      </Show>

                      <div class="grid gap-2">
                        <label class="text-sm font-medium text-wool-300">Source</label>
                        <p class="text-xs text-wool-500">Enter local path or remote repo URL.</p>
                        <div class="flex gap-2">
                          <input
                            type="text"
                            placeholder="/path/to/project or https://github.com/user/repo"
                            class="input flex-1"
                            value={repo.type === 'gitRepo' ? repo.gitUrl : repo.localPath}
                            onInput={(e) => {
                              const value = e.currentTarget.value;
                              if (looksLikeRemoteRepo(value)) {
                                updateRepo(repo.id, (current) => ({
                                  ...current,
                                  type: 'gitRepo',
                                  gitUrl: value,
                                  localPath: '',
                                }));
                                validateGitUrl(repo.id, value);
                                return;
                              }

                              updateRepo(repo.id, (current) => ({
                                ...current,
                                type: 'localFolder',
                                localPath: value,
                                gitUrl: '',
                              }));
                              validatePath(repo.id, value);
                            }}
                          />
                          <button type="button" class="btn-outline px-3" onClick={() => handleBrowse(repo.id)}>
                            <Icon name="folder-open" class="w-4 h-4" />
                          </button>
                        </div>

                        <Show when={repo.validating}>
                          <p class="text-xs text-wool-500 flex items-center gap-2">
                            <span class="spinner w-3 h-3" />
                            Validating...
                          </p>
                        </Show>

                        <Show when={!repo.validating && repo.validation}>
                          <Show when={repo.validation?.valid}>
                            <p class="text-xs text-sage flex items-center gap-2">
                              <Icon name="check" class="w-3.5 h-3.5" />
                              {repo.type === 'localFolder'
                                ? repo.validation?.needsGitInit
                                  ? 'Folder exists (Git will be initialized)'
                                  : repo.validation?.needsDirCreate
                                    ? 'Folder will be created'
                                    : 'Valid repository'
                                : 'Valid repository'}
                            </p>
                          </Show>
                          <Show when={!repo.validation?.valid && repo.validation?.error}>
                            <p class="text-xs text-terra flex items-center gap-2">
                              <Icon name="alert-circle" class="w-3.5 h-3.5" />
                              {repo.validation?.error}
                            </p>
                          </Show>
                        </Show>

                        <div class="grid gap-2">
                          <label class="text-sm font-medium text-wool-300">Branch</label>
                          <input
                            type="text"
                            class="input"
                            placeholder="main"
                            value={repo.gitBranch}
                            onInput={(e) => {
                              const value = e.currentTarget.value;
                              updateRepo(repo.id, (current) => ({ ...current, gitBranch: value }));
                            }}
                          />
                        </div>
                      </div>
                    </div>
                  )}
                </For>
              </div>
            </Show>
          </form>
        </section>

        <footer>
          <button
            type="button"
            class="btn-ghost"
            onClick={() => {
              if (step() === 'details') {
                project.cancelProjectSetup();
              } else {
                setStep('details');
              }
            }}
            disabled={creating()}
          >
            {step() === 'details' ? 'Cancel' : 'Back'}
          </button>

          <button
            type="button"
            class="btn"
            disabled={(step() === 'details' && !canContinue()) || (step() === 'repos' && (!canCreate() || creating()))}
            onClick={() => {
              if (step() === 'details') {
                goToReposStep();
              } else {
                handleCreate();
              }
            }}
          >
            <Show when={step() === 'repos' && creating()}>
              <span class="spinner w-4 h-4" />
            </Show>
            <Show when={step() === 'details'}>
              Next
            </Show>
            <Show when={step() === 'repos' && !creating()}>
              <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
              </svg>
              Create Project
            </Show>
          </button>
        </footer>
      </div>
    </dialog>
  );
};
