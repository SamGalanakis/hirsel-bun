/**
 * DeliveryDialog - Dialog for delivering board changes to a branch
 *
 * Features:
 * - Target branch input with live validation
 * - Delivery action selector (push/pr/merge) with smart availability
 * - Merge state indicator with yolomerge support
 * - CodeMirror-based summary editor with preview toggle
 * - Delivery status and progress
 * - Action buttons based on status
 */

import { type Component, Show, For, createSignal, createEffect, createMemo, onMount, onCleanup } from 'solid-js';
import { invoke } from '../../lib/invoke';
import { useEscapeKey, useRouteWorkTree } from '../../hooks';
import { useProject, useRoute, useDelivery } from '../../stores';
import { Icon, Markdown } from '../shared';
import { amber, sage, terra } from '../../lib/theme-colors';
import { EditorView, keymap } from '@codemirror/view';
import { EditorState } from '@codemirror/state';
import { markdown } from '@codemirror/lang-markdown';
import { defaultKeymap } from '@codemirror/commands';
import type { BoardDeliveryStatus, DeliveryValidation, WorkItemTree } from '../../lib/types';

type DeliveryAction = 'push' | 'pr' | 'merge';

interface DeliveryDialogProps {
  onClose: () => void;
}

export const DeliveryDialog: Component<DeliveryDialogProps> = (props) => {
  const deliveryState = useDelivery();
  const project = useProject();
  const route = useRoute();
  const { snapshot } = useRouteWorkTree();

  // Seed defaults from active route configuration
  const activeRoute = () => route.currentRoute();
  const defaultRepo = () => {
    const r = activeRoute();
    if (!r?.repos?.length) return null;
    if (r.defaultRepoId) {
      const selected = r.repos.find((repo) => repo.id === r.defaultRepoId);
      if (selected) return selected;
    }
    return r.repos[0];
  };
  const projectRepo = () => {
    const sp = defaultRepo()?.startingPoint;
    if (!sp) return '';
    if (sp.type === 'gitRepo') return sp.url;
    if (sp.type === 'localFolder') return sp.path;
    return '';
  };
  const projectBranch = () => {
    const sp = defaultRepo()?.startingPoint;
    const sourceBranch = sp?.type === 'gitRepo' ? sp.branch : null;
    return activeRoute()?.targetBranch || sourceBranch || 'main';
  };

  // Form state
  const [targetBranch, setTargetBranch] = createSignal(projectBranch());
  const [remoteUrl, setRemoteUrl] = createSignal(projectRepo());
  const [error, setError] = createSignal<string | null>(null);
  const [summary, setSummary] = createSignal('');
  const [summaryEdited, setSummaryEdited] = createSignal(false);
  const [deliveryAction, setDeliveryAction] = createSignal<DeliveryAction>('pr');

  // Validation state
  const [validation, setValidation] = createSignal<DeliveryValidation | null>(null);
  const [validating, setValidating] = createSignal(false);

  // UI state
  const [activeTab, setActiveTab] = createSignal<'edit' | 'preview'>('preview');

  // Branch combobox state
  const [branchDropdownOpen, setBranchDropdownOpen] = createSignal(false);
  const [highlightedIndex, setHighlightedIndex] = createSignal(-1);
  let branchComboRef: HTMLDivElement | undefined;

  const filteredBranches = createMemo(() => {
    const v = validation();
    const branches = v?.remoteBranches ?? [];
    const query = targetBranch().toLowerCase().trim();
    if (!query) return branches;
    return branches.filter((b) => b.toLowerCase().includes(query));
  });

  const isExactMatch = createMemo(() => {
    const query = targetBranch().trim().toLowerCase();
    return filteredBranches().some((b) => b.toLowerCase() === query);
  });

  // CodeMirror ref
  let editorContainerRef: HTMLDivElement | undefined;
  let editorView: EditorView | undefined;

  // Dark theme for CodeMirror
  const darkTheme = EditorView.theme({
    '&': {
      backgroundColor: 'transparent',
      color: 'var(--wool-200)',
      fontSize: '14px',
      height: '100%',
    },
    '.cm-content': {
      fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace',
      padding: '16px',
      caretColor: 'var(--amber-500)',
    },
    '.cm-cursor': {
      borderLeftColor: 'var(--amber-500)',
    },
    '&.cm-focused .cm-selectionBackground, .cm-selectionBackground': {
      backgroundColor: 'var(--cm-selection-bg)',
    },
    '.cm-activeLine': {
      backgroundColor: 'var(--cm-active-line-bg)',
    },
    '.cm-gutters': {
      backgroundColor: 'var(--pasture-800)',
      color: 'var(--wool-600)',
      border: 'none',
      borderRight: '1px solid var(--pasture-600)',
    },
    '.cm-lineNumbers .cm-gutterElement': {
      padding: '0 8px 0 12px',
    },
    // Markdown syntax highlighting
    '.cm-header': { color: 'var(--amber-400)' },
    '.cm-strong': { color: 'var(--wool-100)', fontWeight: 'bold' },
    '.cm-emphasis': { color: 'var(--wool-200)', fontStyle: 'italic' },
    '.cm-link': { color: 'var(--sky-400)' },
    '.cm-url': { color: 'var(--wool-500)' },
    '.cm-quote': { color: 'var(--sage)', fontStyle: 'italic' },
    '.cm-list': { color: 'var(--amber-500)' },
  }, { dark: true });

  // Initialize CodeMirror directly (no solid-codemirror wrapper)
  onMount(() => {
    if (!editorContainerRef) return;
    const state = EditorState.create({
      doc: summary(),
      extensions: [
        markdown(),
        keymap.of(defaultKeymap),
        darkTheme,
        EditorView.lineWrapping,
        EditorView.updateListener.of((update) => {
          if (update.docChanged) {
            setSummary(update.state.doc.toString());
            setSummaryEdited(true);
          }
        }),
      ],
    });
    editorView = new EditorView({ state, parent: editorContainerRef });
  });

  onCleanup(() => editorView?.destroy());

  // Helper to flatten live tree
  const flattenTree = (nodes: WorkItemTree[]): WorkItemTree[] => {
    const result: WorkItemTree[] = [];
    const flatten = (n: WorkItemTree) => {
      result.push(n);
      n.children.forEach(flatten);
    };
    nodes.forEach(flatten);
    return result;
  };

  // Generate summary on mount (only if not edited)
  createEffect(() => {
    if (summaryEdited()) return;

    const completedNodes = flattenTree(snapshot()?.tree ?? [])
      .filter((n) => !n.parentId)
      .filter((n) => ['done', 'validated', 'awaiting_check'].includes(n.status));

    const lines = completedNodes.map((n) => `- ${n.title}`);
    setSummary(lines.join('\n') || 'No completed work items');
  });

  // Validate target branch with debounce
  const runValidation = async (branch: string) => {
    const projectId = project.selectedProject()?.id;
    const routeId = route.currentRouteId();
    if (!projectId || !routeId || !branch.trim()) {
      setValidation(null);
      return;
    }

    setValidating(true);
    try {
      const result = await invoke<DeliveryValidation>('validate_delivery_target', {
        projectId,
        routeId,
        targetBranch: branch.trim(),
        remoteUrl: remoteUrl() || null,
      });
      setValidation(result);

      // Seed remote URL from validation (override initial project seed)
      if (result.remoteUrl && (!remoteUrl() || remoteUrl() === projectRepo())) {
        setRemoteUrl(result.remoteUrl);
      }

      // Auto-select: if only 1 action, pick it; if current is unavailable, pick first
      const actions = result.availableActions;
      if (actions.length === 1) {
        setDeliveryAction(actions[0]);
      } else if (actions.length > 0 && !actions.includes(deliveryAction())) {
        setDeliveryAction(actions[0]);
      }
    } catch (e) {
      console.error('Validation failed:', e);
      setValidation(null);
    } finally {
      setValidating(false);
    }
  };

  // Debounced validation on branch or remote URL change
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;
  createEffect(() => {
    const branch = targetBranch();
    const _url = remoteUrl(); // track remote URL changes too
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => runValidation(branch), 500);
  });
  onCleanup(() => clearTimeout(debounceTimer));

  // Close branch dropdown on click outside
  const handleClickOutside = (e: MouseEvent) => {
    if (branchComboRef && !branchComboRef.contains(e.target as Node)) {
      setBranchDropdownOpen(false);
      setHighlightedIndex(-1);
    }
  };
  onMount(() => document.addEventListener('mousedown', handleClickOutside));
  onCleanup(() => document.removeEventListener('mousedown', handleClickOutside));

  // Run validation immediately on mount
  onMount(() => runValidation(targetBranch()));

  // Known forge hosts for URL detection
  const forgeHosts = ['github.com', 'gitlab.com', 'bitbucket.org', 'codeberg.org', 'sr.ht'];

  // Parse a repo URL: normalize protocol, extract branch from /tree/<branch> paths
  const parseRepoUrl = (raw: string): { url: string; branch?: string } => {
    let url = raw.trim();
    if (!url) return { url };

    // SSH URLs are already clean
    if (url.startsWith('git@')) return { url };

    // Detect bare forge hostnames (no protocol) and add https://
    if (!url.startsWith('http://') && !url.startsWith('https://')) {
      if (forgeHosts.some((h) => url.startsWith(h + '/') || url.startsWith(h + ':'))) {
        url = `https://${url}`;
      } else {
        return { url }; // local path or unknown, leave as-is
      }
    }

    // Parse branch from /tree/<branch> (GitHub/GitLab) or /src/branch/<branch> (Gitea/Codeberg)
    let branch: string | undefined;
    const treeMatch = url.match(/^(https?:\/\/[^/]+\/[^/]+\/[^/]+)\/tree\/(.+?)(?:\/)?$/);
    const srcMatch = url.match(/^(https?:\/\/[^/]+\/[^/]+\/[^/]+)\/src\/branch\/(.+?)(?:\/)?$/);
    const match = treeMatch || srcMatch;
    if (match) {
      url = match[1];
      branch = match[2];
    }

    // Strip trailing .git for display consistency
    return { url, branch };
  };

  // Handle repo input: parse URL, extract branch if present
  const handleRepoInput = (raw: string) => {
    const parsed = parseRepoUrl(raw);
    setRemoteUrl(parsed.url);
    if (parsed.branch) {
      setTargetBranch(parsed.branch);
    }
  };

  // Computed: detected repo type for icon/label feedback
  const repoType = (): 'ssh' | 'https' | 'local' | 'unknown' => {
    const url = remoteUrl();
    if (!url) return 'unknown';
    if (url.startsWith('git@') || url.startsWith('ssh://')) return 'ssh';
    if (url.startsWith('https://') || url.startsWith('http://')) return 'https';
    if (forgeHosts.some((h) => url.includes(h)) || url.includes('.git')) return 'https';
    if (url.startsWith('/') || url.startsWith('~') || url.startsWith('.')) return 'local';
    return 'unknown';
  };

  const repoIcon = () => {
    switch (repoType()) {
      case 'ssh': return 'terminal';
      case 'https': return 'globe';
      case 'local': return 'folder';
      default: return 'git-branch';
    }
  };

  const repoHint = () => {
    switch (repoType()) {
      case 'ssh': return 'SSH';
      case 'https': return 'HTTPS';
      case 'local': return 'Local';
      default: return null;
    }
  };

  // Computed: whether an action is available
  const isActionAvailable = (action: DeliveryAction): boolean => {
    const v = validation();
    if (!v) return false;
    return v.availableActions.includes(action);
  };

  // Computed: whether to show the action selector column
  const hasPostPushActions = () => {
    const v = validation();
    if (!v) return false;
    return v.availableActions.some((a) => a !== 'push');
  };

  // Computed: whether merge would be a yolomerge
  const isYolomerge = () => {
    const v = validation();
    return v?.mergeState === 'conflicts';
  };

  useEscapeKey(() => {
    if (!deliveryState.deliveryPending()) {
      props.onClose();
    }
  });

  // Handle starting delivery
  const handleStartDelivery = async () => {
    setError(null);
    const branch = targetBranch().trim();
    if (!branch) {
      setError('Target branch is required');
      return;
    }

    const result = await deliveryState.startDelivery(branch, false, remoteUrl() || undefined);
    if (!result) {
      setError('Failed to start delivery');
    }
  };

  // Handle completing delivery based on selected action
  const handleCompleteDelivery = async () => {
    setError(null);
    const action = deliveryAction();

    if (action === 'push') {
      props.onClose();
      return;
    }

    const result = await deliveryState.completeDelivery(action, summary(), remoteUrl() || undefined);
    if (!result) {
      setError(`Failed to ${action === 'pr' ? 'create PR' : 'merge'}`);
    }
  };

  // Handle abandoning delivery
  const handleAbandon = async () => {
    setError(null);
    const result = await deliveryState.abandonDelivery();
    if (result) {
      props.onClose();
    } else {
      setError('Failed to abandon delivery');
    }
  };

  // Handle retry
  const handleRetry = async () => {
    setError(null);
    await deliveryState.retryDelivery();
  };

  // Get current delivery status
  const delivery = () => deliveryState.currentDelivery();
  const isActive = () => {
    const d = delivery();
    return d && !['abandoned', 'merged'].includes(d.status);
  };

  // Status display helpers
  const statusColor = (status: BoardDeliveryStatus): string => {
    switch (status) {
      case 'pending':
        return 'var(--wool-500)';
      case 'in_progress':
        return 'var(--amber-500)';
      case 'resolving_conflicts':
        return 'var(--amber-400)';
      case 'pushed':
        return 'var(--sage)';
      case 'pr_open':
        return 'var(--sky-400)';
      case 'merged':
        return 'var(--sage)';
      case 'failed':
        return 'var(--terra)';
      case 'abandoned':
        return 'var(--wool-600)';
      default:
        return 'var(--wool-500)';
    }
  };

  const statusLabel = (status: BoardDeliveryStatus): string => {
    switch (status) {
      case 'pending':
        return 'Pending';
      case 'in_progress':
        return 'Pushing...';
      case 'resolving_conflicts':
        return 'Resolving Conflicts';
      case 'pushed':
        return 'Pushed';
      case 'pr_open':
        return 'PR Open';
      case 'merged':
        return 'Merged';
      case 'failed':
        return 'Failed';
      case 'abandoned':
        return 'Abandoned';
      default:
        return status;
    }
  };

  // Validation status indicator - inline hint for normal states, card for warnings
  const ValidationStatus = () => {
    const v = validation();

    if (validating()) {
      return (
        <div class="flex items-center gap-1.5 mt-2 text-xs text-wool-500">
          <Icon name="loader-2" class="w-3 h-3 text-amber-500 animate-spin" />
          Checking branch...
        </div>
      );
    }

    if (!v) {
      return (
        <div class="flex items-center gap-1.5 mt-2 text-xs text-wool-500">
          <Icon name="git-branch" class="w-3 h-3 text-sage" />
          Creates branch: <span class="text-wool-300 font-mono">hirsel/...</span>
        </div>
      );
    }

    if (v.error) {
      return (
        <div class="flex items-center gap-1.5 mt-2 text-xs text-terra">
          <Icon name="alert-circle" class="w-3 h-3" />
          {v.error}
          <Show when={v.needsInit}>
            <span class="text-wool-500 ml-1">(will initialize on delivery)</span>
          </Show>
        </div>
      );
    }

    if (!v.hasRemote && !v.isLocal) {
      return (
        <div class="flex items-center gap-1.5 mt-2 text-xs text-wool-600">
          <Icon name="wifi-off" class="w-3 h-3" />
          No remote configured
        </div>
      );
    }

    if (v.isLocal) {
      return (
        <div class="flex items-center gap-1.5 mt-2 text-xs text-wool-500">
          <Icon name="folder" class="w-3 h-3 text-sage" />
          Local push to <span class="font-mono text-wool-300">{targetBranch()}</span>
        </div>
      );
    }

    if (!v.targetExistsOnRemote) {
      return (
        <div class="flex items-center gap-1.5 mt-2 text-xs text-wool-500">
          <Icon name="plus-circle" class="w-3 h-3 text-sky-400" />
          <span class="font-mono text-wool-300">{targetBranch()}</span> will be created on remote
        </div>
      );
    }

    if (v.mergeState === 'conflicts') {
      return (
        <div
          class="flex items-center gap-2 mt-2 px-3 py-2 rounded-none text-xs"
          style={{
            background: amber(0.06),
            border: `1px solid ${amber(0.15)}`,
          }}
        >
          <Icon name="alert-triangle" class="w-3.5 h-3.5 text-amber-500" />
          <span class="text-wool-400">
            <span class="text-amber-400 font-medium">{v.conflictingFiles.length} conflicting file{v.conflictingFiles.length !== 1 ? 's' : ''}</span>
            {' '}<span class="text-wool-500">(yolomerge available)</span>
          </span>
        </div>
      );
    }

    return (
      <div class="flex items-center gap-1.5 mt-2 text-xs text-wool-500">
        <Icon name="check-circle" class="w-3 h-3 text-sage" />
        Clean merge possible
      </div>
    );
  };

  return (
    <div
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-[2px]"
      onClick={(e) => {
        if (e.target === e.currentTarget && !deliveryState.deliveryPending()) {
          props.onClose();
        }
      }}
    >
      <div
        class="w-full max-w-4xl flex flex-col rounded-none shadow-2xl overflow-hidden"
        style={{
          height: 'min(85vh, 720px)',
          background: 'var(--pasture-800)',
          border: '1px solid var(--pasture-600)',
          'box-shadow': '0 25px 50px -12px rgba(0, 0, 0, 0.5)',
        }}
      >
        {/* Header */}
        <header
          class="flex items-center justify-between px-6 py-4 flex-shrink-0"
          style={{ 'border-bottom': '1px solid var(--pasture-600)' }}
        >
          <div class="flex items-center gap-3">
            <div
              class="w-10 h-10 rounded-none flex items-center justify-center"
              style={{
                background: sage(0.12),
                border: `1px solid ${sage(0.2)}`,
              }}
            >
              <Icon name="package" class="w-5 h-5 text-sage" />
            </div>
            <div>
              <h2 class="text-base font-semibold text-wool-100" style={{ 'font-family': 'var(--font-primary)' }}>
                {isActive() ? 'Delivery in Progress' : 'Deliver Changes'}
              </h2>
              <p class="text-xs text-wool-500">
                {isActive() ? 'Pushing your work to the repository' : 'Ship completed work to a branch'}
              </p>
            </div>
          </div>
          <button
            onClick={props.onClose}
            disabled={deliveryState.deliveryPending()}
            class="p-2 rounded-none text-wool-500 hover:text-wool-300 hover:bg-pasture-700 transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
          >
            <Icon name="x" class="w-5 h-5" />
          </button>
        </header>

        {/* Content */}
        <div class="flex-1 overflow-y-auto">
          {/* Pre-delivery form */}
          <Show when={!isActive()}>
            <div class="h-full flex flex-col">
              {/* Remote + branch + action */}
              <div class="px-6 py-5 space-y-5" style={{ 'border-bottom': '1px solid var(--pasture-600)' }}>
                {/* Repository */}
                <div class="field">
                  <label class="block text-sm font-medium text-wool-300 mb-2">
                    Repository
                  </label>
                  <div class="relative">
                    <div class="absolute left-3 top-1/2 -translate-y-1/2 pointer-events-none">
                      <Icon name={repoIcon()} class="w-3.5 h-3.5 text-wool-500" />
                    </div>
                    <input
                      type="text"
                      value={remoteUrl()}
                      onInput={(e) => handleRepoInput(e.currentTarget.value)}
                      placeholder="github.com/user/repo or git@github.com:user/repo.git"
                      class="input w-full font-mono text-xs pl-8"
                      style={{ 'padding-right': repoHint() ? '3.5rem' : undefined }}
                    />
                    <Show when={repoHint()}>
                      <div class="absolute right-3 top-1/2 -translate-y-1/2 pointer-events-none">
                        <span class="text-[10px] font-medium uppercase tracking-wide text-wool-600">{repoHint()}</span>
                      </div>
                    </Show>
                  </div>
                </div>

              <div class={`grid gap-6 ${hasPostPushActions() ? 'grid-cols-2' : 'grid-cols-1'}`}>
                {/* Target branch */}
                <div class="field">
                  <label class="block text-sm font-medium text-wool-300 mb-2">
                    Target branch
                  </label>
                  <div class="relative" ref={branchComboRef}>
                    <input
                      type="text"
                      value={targetBranch()}
                      onInput={(e) => {
                        setTargetBranch(e.currentTarget.value);
                        setBranchDropdownOpen(true);
                        setHighlightedIndex(-1);
                      }}
                      onFocus={() => {
                        if ((validation()?.remoteBranches.length ?? 0) > 0) {
                          setBranchDropdownOpen(true);
                        }
                      }}
                      onKeyDown={(e) => {
                        const items = filteredBranches();
                        const hasCustom = !isExactMatch() && targetBranch().trim().length > 0;
                        const totalItems = items.length + (hasCustom ? 1 : 0);

                        if (e.key === 'ArrowDown') {
                          e.preventDefault();
                          setBranchDropdownOpen(true);
                          setHighlightedIndex((i) => (i + 1) % totalItems);
                        } else if (e.key === 'ArrowUp') {
                          e.preventDefault();
                          setBranchDropdownOpen(true);
                          setHighlightedIndex((i) => (i - 1 + totalItems) % totalItems);
                        } else if (e.key === 'Enter' && branchDropdownOpen()) {
                          e.preventDefault();
                          const idx = highlightedIndex();
                          if (idx >= 0 && idx < items.length) {
                            setTargetBranch(items[idx]);
                          }
                          setBranchDropdownOpen(false);
                          setHighlightedIndex(-1);
                        } else if (e.key === 'Escape' && branchDropdownOpen()) {
                          e.preventDefault();
                          e.stopPropagation();
                          setBranchDropdownOpen(false);
                          setHighlightedIndex(-1);
                        }
                      }}
                      placeholder="main"
                      class="input w-full"
                      autocomplete="off"
                    />
                    <Show when={branchDropdownOpen() && (filteredBranches().length > 0 || (!isExactMatch() && targetBranch().trim()))}>
                      <div
                        class="absolute z-50 left-0 right-0 mt-1 max-h-48 overflow-y-auto rounded-none shadow-xl"
                        style={{
                          background: 'var(--pasture-900)',
                          border: '1px solid var(--pasture-600)',
                        }}
                      >
                        <For each={filteredBranches()}>
                          {(branch, i) => (
                            <button
                              type="button"
                              class="w-full text-left px-3 py-2 text-sm flex items-center gap-2 transition-colors"
                              style={{
                                background: highlightedIndex() === i() ? 'var(--pasture-700)' : 'transparent',
                                color: 'var(--wool-200)',
                              }}
                              onMouseEnter={() => setHighlightedIndex(i())}
                              onMouseDown={(e) => {
                                e.preventDefault();
                                setTargetBranch(branch);
                                setBranchDropdownOpen(false);
                                setHighlightedIndex(-1);
                              }}
                            >
                              <Icon name="git-branch" class="w-3.5 h-3.5 text-wool-500 flex-shrink-0" />
                              <span class="truncate font-mono text-xs">{branch}</span>
                              <Show when={branch === targetBranch().trim()}>
                                <Icon name="check" class="w-3.5 h-3.5 text-sage ml-auto flex-shrink-0" />
                              </Show>
                            </button>
                          )}
                        </For>
                        <Show when={!isExactMatch() && targetBranch().trim().length > 0}>
                          <button
                            type="button"
                            class="w-full text-left px-3 py-2 text-sm flex items-center gap-2 transition-colors"
                            style={{
                              background: highlightedIndex() === filteredBranches().length ? 'var(--pasture-700)' : 'transparent',
                              color: 'var(--wool-400)',
                              'border-top': filteredBranches().length > 0 ? '1px solid var(--pasture-600)' : 'none',
                            }}
                            onMouseEnter={() => setHighlightedIndex(filteredBranches().length)}
                            onMouseDown={(e) => {
                              e.preventDefault();
                              setBranchDropdownOpen(false);
                              setHighlightedIndex(-1);
                            }}
                          >
                            <Icon name="plus" class="w-3.5 h-3.5 text-wool-600 flex-shrink-0" />
                            <span class="truncate text-xs">
                              New branch: <span class="font-mono text-wool-300">{targetBranch().trim()}</span>
                            </span>
                          </button>
                        </Show>
                      </div>
                    </Show>
                  </div>
                  <ValidationStatus />
                </div>

                {/* Delivery action selector - only show when there are post-push options */}
                <Show when={hasPostPushActions()}>
                  <div class="field">
                    <label class="block text-sm font-medium text-wool-300 mb-2">
                      After push
                    </label>
                    <div class="space-y-2">
                      {/* Push only */}
                      <Show when={isActionAvailable('push')}>
                        <label
                          class="flex items-center gap-3 p-3 rounded-none transition-all cursor-pointer"
                          style={{
                            background: deliveryAction() === 'push' ? amber(0.08) : 'transparent',
                            border: `1px solid ${deliveryAction() === 'push' ? amber(0.3) : 'var(--pasture-600)'}`,
                          }}
                        >
                          <input
                            type="radio"
                            name="deliveryAction"
                            checked={deliveryAction() === 'push'}
                            onChange={() => setDeliveryAction('push')}
                            class="sr-only"
                          />
                          <div
                            class="w-4 h-4 rounded-none border-2 flex items-center justify-center flex-shrink-0"
                            style={{
                              'border-color': deliveryAction() === 'push' ? 'var(--amber-500)' : 'var(--wool-600)',
                              background: deliveryAction() === 'push' ? 'var(--amber-500)' : 'transparent',
                            }}
                          >
                            <Show when={deliveryAction() === 'push'}>
                              <div class="w-1.5 h-1.5 rounded-none bg-pasture-900" />
                            </Show>
                          </div>
                          <div class="flex-1 min-w-0">
                            <div class="text-sm font-medium text-wool-200">Push only</div>
                            <div class="text-xs text-wool-500">Create PR manually</div>
                          </div>
                        </label>
                      </Show>

                      {/* Create PR */}
                      <Show when={isActionAvailable('pr')}>
                        <label
                          class="flex items-center gap-3 p-3 rounded-none transition-all cursor-pointer"
                          style={{
                            background: deliveryAction() === 'pr' ? 'rgba(56, 189, 248, 0.08)' : 'transparent',
                            border: `1px solid ${deliveryAction() === 'pr' ? 'rgba(56, 189, 248, 0.3)' : 'var(--pasture-600)'}`,
                          }}
                        >
                          <input
                            type="radio"
                            name="deliveryAction"
                            checked={deliveryAction() === 'pr'}
                            onChange={() => setDeliveryAction('pr')}
                            class="sr-only"
                          />
                          <div
                            class="w-4 h-4 rounded-none border-2 flex items-center justify-center flex-shrink-0"
                            style={{
                              'border-color': deliveryAction() === 'pr' ? 'var(--sky-400)' : 'var(--wool-600)',
                              background: deliveryAction() === 'pr' ? 'var(--sky-400)' : 'transparent',
                            }}
                          >
                            <Show when={deliveryAction() === 'pr'}>
                              <div class="w-1.5 h-1.5 rounded-none bg-pasture-900" />
                            </Show>
                          </div>
                          <div class="flex-1 min-w-0">
                            <div class="text-sm font-medium text-wool-200">Create PR</div>
                            <div class="text-xs text-wool-500">Open pull request</div>
                          </div>
                        </label>
                      </Show>

                      {/* Direct merge / Yolomerge */}
                      <Show when={isActionAvailable('merge')}>
                        <label
                          class="flex items-center gap-3 p-3 rounded-none transition-all cursor-pointer"
                          style={{
                            background: deliveryAction() === 'merge'
                              ? (isYolomerge() ? amber(0.08) : sage(0.08))
                              : 'transparent',
                            border: `1px solid ${deliveryAction() === 'merge'
                              ? (isYolomerge() ? amber(0.3) : sage(0.3))
                              : 'var(--pasture-600)'}`,
                          }}
                        >
                          <input
                            type="radio"
                            name="deliveryAction"
                            checked={deliveryAction() === 'merge'}
                            onChange={() => setDeliveryAction('merge')}
                            class="sr-only"
                          />
                          <div
                            class="w-4 h-4 rounded-none border-2 flex items-center justify-center flex-shrink-0"
                            style={{
                              'border-color': deliveryAction() === 'merge'
                                ? (isYolomerge() ? 'var(--amber-500)' : 'var(--sage)')
                                : 'var(--wool-600)',
                              background: deliveryAction() === 'merge'
                                ? (isYolomerge() ? 'var(--amber-500)' : 'var(--sage)')
                                : 'transparent',
                            }}
                          >
                            <Show when={deliveryAction() === 'merge'}>
                              <div class="w-1.5 h-1.5 rounded-none bg-pasture-900" />
                            </Show>
                          </div>
                          <div class="flex-1 min-w-0">
                            <Show when={isYolomerge()} fallback={
                              <>
                                <div class="text-sm font-medium text-wool-200">Direct merge</div>
                                <div class="text-xs text-wool-500">Merge immediately</div>
                              </>
                            }>
                              <div class="text-sm font-medium text-amber-400">Yolomerge</div>
                              <div class="text-xs text-amber-500/70">AI-assisted conflict resolution</div>
                            </Show>
                          </div>
                        </label>
                      </Show>
                    </div>
                  </div>
                </Show>
              </div>
              </div>

              {/* Summary editor */}
              <div class="flex-1 flex flex-col min-h-0 px-6 py-5">
                <div class="flex items-center justify-between mb-3">
                  <label class="text-sm font-medium text-wool-300">
                    Summary
                  </label>
                  <div
                    class="flex items-center p-0.5 rounded-none"
                    style={{ background: 'var(--pasture-900)' }}
                  >
                    <button
                      onClick={() => setActiveTab('edit')}
                      class={`px-3 py-1.5 rounded-none text-xs font-medium transition-all ${
                        activeTab() === 'edit'
                          ? 'bg-pasture-700 text-wool-100 shadow-sm'
                          : 'text-wool-500 hover:text-wool-300'
                      }`}
                    >
                      <Icon name="pencil" class="w-3.5 h-3.5 inline mr-1.5" />
                      Edit
                    </button>
                    <button
                      onClick={() => setActiveTab('preview')}
                      class={`px-3 py-1.5 rounded-none text-xs font-medium transition-all ${
                        activeTab() === 'preview'
                          ? 'bg-pasture-700 text-wool-100 shadow-sm'
                          : 'text-wool-500 hover:text-wool-300'
                      }`}
                    >
                      <Icon name="eye" class="w-3.5 h-3.5 inline mr-1.5" />
                      Preview
                    </button>
                  </div>
                </div>

                <div
                  class="flex-1 rounded-none overflow-hidden"
                  style={{
                    background: 'var(--pasture-900)',
                    border: '1px solid var(--pasture-600)',
                    'min-height': '200px',
                  }}
                >
                  {/* CodeMirror editor */}
                  <div
                    ref={editorContainerRef}
                    class="h-full overflow-auto"
                    style={{ display: activeTab() === 'edit' ? 'block' : 'none' }}
                  />

                  {/* Markdown preview */}
                  <div
                    class="h-full overflow-auto p-4"
                    style={{
                      display: activeTab() === 'preview' ? 'block' : 'none',
                    }}
                  >
                    <Markdown
                      content={summary()}
                      class="prose prose-invert prose-sm max-w-none"
                    />
                  </div>
                </div>

                <p class="text-xs text-wool-600 mt-2">
                  Auto-generated from completed tasks. Switch to Edit to customize.
                </p>
              </div>
            </div>
          </Show>

          {/* Active delivery status */}
          <Show when={isActive()}>
            <div class="p-6 space-y-6">
              {/* Status card */}
              <div
                class="p-5 rounded-none"
                style={{
                  background: 'var(--pasture-900)',
                  border: '1px solid var(--pasture-600)',
                }}
              >
                <div class="flex items-center gap-4">
                  <div
                    class={`w-12 h-12 rounded-none flex items-center justify-center ${
                      delivery()?.status === 'in_progress' ? 'animate-pulse' : ''
                    }`}
                    style={{
                      background: `color-mix(in srgb, ${statusColor(delivery()!.status)} 15%, transparent)`,
                      border: `1px solid color-mix(in srgb, ${statusColor(delivery()!.status)} 30%, transparent)`,
                    }}
                  >
                    <Show when={delivery()?.status === 'in_progress'} fallback={
                      <Show when={delivery()?.status === 'pushed'} fallback={
                        <Show when={delivery()?.status === 'failed'} fallback={
                          <span style={{ color: statusColor(delivery()!.status) }}><Icon name="check" class="w-6 h-6" /></span>
                        }>
                          <span style={{ color: statusColor(delivery()!.status) }}><Icon name="alert-circle" class="w-6 h-6" /></span>
                        </Show>
                      }>
                        <span style={{ color: statusColor(delivery()!.status) }}><Icon name="check" class="w-6 h-6" /></span>
                      </Show>
                    }>
                      <span style={{ color: statusColor(delivery()!.status) }}><Icon name="loader-2" class="w-6 h-6 animate-spin" /></span>
                    </Show>
                  </div>
                  <div>
                    <div class="text-lg font-semibold" style={{ color: statusColor(delivery()!.status) }}>
                      {statusLabel(delivery()!.status)}
                    </div>
                    <div class="text-sm text-wool-500">
                      {delivery()?.status === 'in_progress' && 'Pushing changes to the repository...'}
                      {delivery()?.status === 'pushed' && 'Changes pushed successfully'}
                      {delivery()?.status === 'pr_open' && 'Pull request created'}
                      {delivery()?.status === 'failed' && 'Something went wrong'}
                    </div>
                  </div>
                </div>
              </div>

              {/* Branch info */}
              <div
                class="grid grid-cols-2 gap-4 p-5 rounded-none"
                style={{
                  background: 'var(--pasture-900)',
                  border: '1px solid var(--pasture-600)',
                }}
              >
                <div>
                  <div class="text-xs text-wool-500 mb-1">Target Branch</div>
                  <div class="text-sm text-wool-200 font-mono">{delivery()?.targetBranch}</div>
                </div>
                <Show when={delivery()?.deliveryBranch}>
                  <div>
                    <div class="text-xs text-wool-500 mb-1">Delivery Branch</div>
                    <div class="text-sm text-wool-200 font-mono">{delivery()?.deliveryBranch}</div>
                  </div>
                </Show>
                <Show when={delivery()?.prNumber}>
                  <div class="col-span-2">
                    <div class="text-xs text-wool-500 mb-1">Pull Request</div>
                    <a
                      href={delivery()?.prUrl || '#'}
                      target="_blank"
                      rel="noopener noreferrer"
                      class="inline-flex items-center gap-2 text-sm text-sky-400 hover:text-sky-300 transition-colors"
                    >
                      <Icon name="external-link" class="w-4 h-4" />
                      <span class="font-mono">#{delivery()?.prNumber}</span>
                    </a>
                  </div>
                </Show>
              </div>

              {/* Failure message */}
              <Show when={delivery()?.status === 'failed' && delivery()?.failureReason}>
                <div
                  class="p-4 rounded-none flex items-start gap-3"
                  style={{
                    background: terra(0.08),
                    border: `1px solid ${terra(0.2)}`,
                  }}
                >
                  <Icon name="alert-triangle" class="w-5 h-5 text-terra flex-shrink-0 mt-0.5" />
                  <div class="text-sm text-terra">{delivery()?.failureReason}</div>
                </div>
              </Show>
            </div>
          </Show>

          {/* Error message */}
          <Show when={error()}>
            <div class="px-6 pb-4">
              <div
                class="p-4 rounded-none flex items-start gap-3"
                style={{
                  background: terra(0.08),
                  border: `1px solid ${terra(0.2)}`,
                }}
              >
                <Icon name="alert-triangle" class="w-5 h-5 text-terra flex-shrink-0 mt-0.5" />
                <div class="text-sm text-terra">{error()}</div>
              </div>
            </div>
          </Show>
        </div>

        {/* Footer */}
        <footer
          class="flex items-center justify-end gap-3 px-6 py-4 flex-shrink-0"
          style={{ 'border-top': '1px solid var(--pasture-600)' }}
        >
          {/* Pre-delivery actions */}
          <Show when={!isActive()}>
            <button
              onClick={props.onClose}
              class="btn btn-ghost"
            >
              Cancel
            </button>
            <button
              onClick={handleStartDelivery}
              disabled={deliveryState.deliveryPending()}
              class="btn"
              style={{
                background: 'var(--sage)',
                color: 'var(--pasture-900)',
                opacity: deliveryState.deliveryPending() ? 0.5 : 1,
              }}
            >
              <Show when={deliveryState.deliveryPending()} fallback={
                <>
                  <Icon name="rocket" class="w-4 h-4 mr-2" />
                  Deliver
                </>
              }>
                <Icon name="loader-2" class="w-4 h-4 mr-2 animate-spin" />
                Starting...
              </Show>
            </button>
          </Show>

          {/* Pushed - show action based on selected delivery action */}
          <Show when={delivery()?.status === 'pushed'}>
            <button
              onClick={handleAbandon}
              disabled={deliveryState.deliveryPending()}
              class="btn btn-ghost"
            >
              Close
            </button>
            <Show when={deliveryAction() === 'push'}>
              <button
                onClick={props.onClose}
                class="btn"
                style={{ background: 'var(--sage)', color: 'var(--pasture-900)' }}
              >
                <Icon name="check" class="w-4 h-4 mr-2" />
                Done
              </button>
            </Show>
            <Show when={deliveryAction() === 'pr'}>
              <button
                onClick={handleCompleteDelivery}
                disabled={deliveryState.deliveryPending()}
                class="btn"
                style={{
                  background: 'var(--sky-400)',
                  color: 'var(--pasture-900)',
                  opacity: deliveryState.deliveryPending() ? 0.5 : 1,
                }}
              >
                <Show when={deliveryState.deliveryPending()} fallback={
                  <>
                    <Icon name="git-pull-request" class="w-4 h-4 mr-2" />
                    Create PR
                  </>
                }>
                  <Icon name="loader-2" class="w-4 h-4 mr-2 animate-spin" />
                  Creating...
                </Show>
              </button>
            </Show>
            <Show when={deliveryAction() === 'merge'}>
              <button
                onClick={handleCompleteDelivery}
                disabled={deliveryState.deliveryPending()}
                class="btn"
                style={{
                  background: 'var(--sage)',
                  color: 'var(--pasture-900)',
                  opacity: deliveryState.deliveryPending() ? 0.5 : 1,
                }}
              >
                <Show when={deliveryState.deliveryPending()} fallback={
                  <>
                    <Icon name="git-merge" class="w-4 h-4 mr-2" />
                    Merge Now
                  </>
                }>
                  <Icon name="loader-2" class="w-4 h-4 mr-2 animate-spin" />
                  Merging...
                </Show>
              </button>
            </Show>
          </Show>

          {/* PR Open - done button */}
          <Show when={delivery()?.status === 'pr_open'}>
            <button
              onClick={props.onClose}
              class="btn"
              style={{ background: 'var(--sage)', color: 'var(--pasture-900)' }}
            >
              <Icon name="check" class="w-4 h-4 mr-2" />
              Done
            </button>
          </Show>

          {/* Failed - retry or abandon */}
          <Show when={delivery()?.status === 'failed'}>
            <button
              onClick={handleAbandon}
              disabled={deliveryState.deliveryPending()}
              class="btn btn-ghost"
            >
              Abandon
            </button>
            <button
              onClick={handleRetry}
              disabled={deliveryState.deliveryPending()}
              class="btn"
              style={{
                background: 'var(--amber-500)',
                color: 'var(--pasture-900)',
                opacity: deliveryState.deliveryPending() ? 0.5 : 1,
              }}
            >
              <Show when={deliveryState.deliveryPending()} fallback={
                <>
                  <Icon name="refresh-cw" class="w-4 h-4 mr-2" />
                  Retry
                </>
              }>
                <Icon name="loader-2" class="w-4 h-4 mr-2 animate-spin" />
                Retrying...
              </Show>
            </button>
          </Show>

          {/* In progress - just show cancel */}
          <Show when={delivery()?.status === 'in_progress' || delivery()?.status === 'pending'}>
            <button
              onClick={handleAbandon}
              disabled={deliveryState.deliveryPending()}
              class="btn btn-ghost"
            >
              Cancel
            </button>
          </Show>
        </footer>
      </div>
    </div>
  );
};
