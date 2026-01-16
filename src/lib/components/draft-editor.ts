/**
 * Draft Editor Alpine.js Component
 *
 * A dedicated editor view for configuring draft runs before starting them.
 * Provides controls for spec, worker scale, time limit, HITL mode, and project path.
 */

import { createDraft, updateDraft, startDraft, deleteRun, getRunDetail, readSpecFile, writeSpecFile, readEvalFile, writeEvalFile, validateRepo, initProjectRepo, saveAsset, importAssetFromPath, openAssetsFolder, getAssetsPath } from '../api';
import { convertFileSrc } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { RunDetail, DraftUpdateRequest, RepoValidation, RunnerConfig, RunnerEntry } from '../types';
import { marked } from 'marked';
import DOMPurify from 'dompurify';
import { showConfirm } from '../confirm-dialog';

declare const window: Window & {
  tauriInvoke?: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  toast: {
    success: (message: string, title?: string) => void;
    error: (message: string, title?: string) => void;
    info: (message: string, title?: string) => void;
  };
};

/**
 * Draft editor component data
 */
export interface DraftEditorData {
  runName: string | null;
  name: string;
  spec: string;
  eval: string;
  workerScale: string;
  timeLimitMinutes: number | null;
  timeLimitInput: string;
  humanInTheLoop: boolean;
  projectPath: string;
  saving: boolean;
  savingSpec: boolean;
  savingEval: boolean;
  starting: boolean;
  loading: boolean;
  error: string | null;
  saveTimeout: ReturnType<typeof setTimeout> | null;
  evalSaveTimeout: ReturnType<typeof setTimeout> | null;
  isEditing: boolean;
  activeTab: 'spec' | 'eval' | 'settings';
  previewMode: boolean;
  // Branch selection state
  selectedBranch: string;
  availableBranches: string[];
  repoValidating: boolean;
  repoError: string | null;
  repoIsRemote: boolean;
  normalizedRepoUrl: string;
  repoValidateTimeout: ReturnType<typeof setTimeout> | null;
  // Field validation errors
  workerScaleError: string | null;
  timeLimitError: string | null;
  // Project setup flags (non-git directory handling)
  needsDirCreate: boolean;
  needsGitInit: boolean;
  // File drop state
  specDragOver: boolean;
  evalDragOver: boolean;
  specDragCounter: number;
  // Assets path for image rendering
  assetsPath: string;
  evalDragCounter: number;
  // Tauri event unlisten function
  _unlistenDragDrop: UnlistenFn | null;
  // Cursor position tracking for insertion
  specCursorPos: number;
  evalCursorPos: number;
  // Runner selection
  selectedRunner: string | null;
  availableRunners: RunnerEntry[];
}

/**
 * Parse time limit string to minutes
 * Returns { value, error } where value is null if empty or invalid
 */
function parseTimeLimit(input: string): { value: number | null; error: string | null } {
  if (!input.trim()) return { value: null, error: null }; // Empty is valid (no limit)
  const s = input.trim().toLowerCase();

  // Check for combined format like "1h30m"
  if (s.includes('h') && s.includes('m')) {
    const hMatch = s.match(/^(\d+(?:\.\d+)?)h(\d+)m$/);
    if (!hMatch) {
      return { value: null, error: 'Invalid format. Use: 30m, 1h, or 1h30m' };
    }
    const hours = parseFloat(hMatch[1]);
    const mins = parseInt(hMatch[2], 10);
    if (mins >= 60) {
      return { value: null, error: 'Minutes should be less than 60' };
    }
    const total = Math.round(hours * 60 + mins);
    if (total <= 0) {
      return { value: null, error: 'Time limit must be positive' };
    }
    return { value: total, error: null };
  }

  // Hours format
  if (s.endsWith('h')) {
    const num = parseFloat(s.slice(0, -1));
    if (isNaN(num)) {
      return { value: null, error: 'Invalid hours value' };
    }
    if (num <= 0) {
      return { value: null, error: 'Time limit must be positive' };
    }
    return { value: Math.round(num * 60), error: null };
  }

  // Minutes format
  if (s.endsWith('m')) {
    const num = parseFloat(s.slice(0, -1));
    if (isNaN(num)) {
      return { value: null, error: 'Invalid minutes value' };
    }
    if (num <= 0) {
      return { value: null, error: 'Time limit must be positive' };
    }
    return { value: Math.round(num), error: null };
  }

  // Plain number - assume minutes
  const num = parseFloat(s);
  if (isNaN(num)) {
    return { value: null, error: 'Invalid format. Use: 30m, 1h, or 1h30m' };
  }
  if (num <= 0) {
    return { value: null, error: 'Time limit must be positive' };
  }
  return { value: Math.round(num), error: null };
}

/**
 * Validate worker scale string
 * Valid formats: "3" (fixed), "1-5" (range), "2+" (min with no max)
 */
function validateWorkerScale(input: string): { value: string | null; error: string | null } {
  const s = input.trim();
  if (!s) {
    return { value: null, error: 'Workers is required' };
  }

  // Check for "N+" pattern (autoscale from N)
  if (s.endsWith('+')) {
    const num = parseInt(s.slice(0, -1), 10);
    if (isNaN(num) || num < 1) {
      return { value: null, error: 'Minimum workers must be at least 1' };
    }
    return { value: s, error: null };
  }

  // Check for "N-M" pattern (range)
  if (s.includes('-')) {
    const parts = s.split('-');
    if (parts.length !== 2) {
      return { value: null, error: 'Invalid range. Use: 1-5' };
    }
    const min = parseInt(parts[0], 10);
    const max = parseInt(parts[1], 10);
    if (isNaN(min) || isNaN(max)) {
      return { value: null, error: 'Invalid range. Use numbers like: 1-5' };
    }
    if (min < 1) {
      return { value: null, error: 'Minimum workers must be at least 1' };
    }
    if (max < min) {
      return { value: null, error: 'Maximum must be greater than minimum' };
    }
    return { value: s, error: null };
  }

  // Simple number - fixed count
  const num = parseInt(s, 10);
  if (isNaN(num)) {
    return { value: null, error: 'Enter a number (e.g., 1, 1-3, or 2+)' };
  }
  if (num < 1) {
    return { value: null, error: 'Workers must be at least 1' };
  }
  return { value: s, error: null };
}

/**
 * Format minutes to display string
 */
function formatTimeLimitDisplay(minutes: number | null): string {
  if (minutes === null) return '';
  if (minutes < 60) return `${minutes}m`;
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return m > 0 ? `${h}h${m}m` : `${h}h`;
}

/**
 * Alpine.js component factory for draft editor
 */
export function draftEditor(): DraftEditorData & {
  init(): void;
  destroy(): void;
  loadDraft(name: string): Promise<void>;
  clearDraft(): void;
  refreshFiles(): Promise<void>;
  saveSpec(): Promise<void>;
  saveEval(): Promise<void>;
  saveDraft(): Promise<void>;
  debouncedSaveSpec(): void;
  debouncedSaveEval(): void;
  debouncedSave(): void;
  startRun(): Promise<void>;
  deleteDraft(): Promise<void>;
  renameDraft(newName: string): Promise<void>;
  formatTimeLimitDisplay(minutes: number | null): string;
  handleNameEdit(): void;
  finishNameEdit(): void;
  renderMarkdown(content: string): string;
  validateProjectPath(): Promise<void>;
  debouncedValidateProjectPath(): void;
  onBranchChange(): void;
  canStart(): boolean;
  validateTimeLimit(): boolean;
  validateWorkerScale(): boolean;
  hasValidationErrors(): boolean;
  handleFileDrop(type: 'spec' | 'eval', event: DragEvent): Promise<void>;
  handleDragEnter(type: 'spec' | 'eval', event: DragEvent): void;
  handleDragOver(type: 'spec' | 'eval', event: DragEvent): void;
  handleDragLeave(type: 'spec' | 'eval', event: DragEvent): void;
  handleNativeFileDrop(paths: string[], position: { x: number; y: number }): Promise<void>;
  trackCursorPosition(type: 'spec' | 'eval', event: Event): void;
  openFilePicker(type: 'spec' | 'eval'): void;
  openAssets(): Promise<void>;
} {
  return {
    runName: null,
    name: '',
    spec: '',
    eval: '',
    workerScale: '1',
    timeLimitMinutes: null,
    timeLimitInput: '',
    humanInTheLoop: true,
    projectPath: '',
    saving: false,
    savingSpec: false,
    savingEval: false,
    starting: false,
    loading: false,
    error: null,
    saveTimeout: null,
    evalSaveTimeout: null,
    isEditing: false,
    activeTab: 'spec' as const,
    previewMode: false,
    // Branch selection state
    selectedBranch: '',
    availableBranches: [],
    repoValidating: false,
    repoError: null,
    repoIsRemote: false,
    normalizedRepoUrl: '',
    repoValidateTimeout: null,
    // Field validation errors
    workerScaleError: null,
    timeLimitError: null,
    // Project setup flags (non-git directory handling)
    needsDirCreate: false,
    needsGitInit: false,
    // File drop state
    specDragOver: false,
    evalDragOver: false,
    specDragCounter: 0,
    evalDragCounter: 0,
    assetsPath: '',
    _unlistenDragDrop: null,
    // Cursor position (end of content by default)
    specCursorPos: 0,
    evalCursorPos: 0,
    // Runner selection
    selectedRunner: null,
    availableRunners: [],

    /**
     * Initialize the component
     */
    init(): void {
      // Load available runners from config
      this.loadRunners();

      // Listen for draft selection events
      window.addEventListener('draft-selected', ((e: CustomEvent<string | null>) => {
        if (e.detail) {
          this.loadDraft(e.detail);
        } else {
          this.clearDraft();
        }
      }) as EventListener);

      // Also listen for run-selected to detect draft selection
      window.addEventListener('run-selected', ((e: CustomEvent<string | null>) => {
        // The run-list component will dispatch draft-selected when a draft is selected
        // This handler is for cleanup when switching away from a draft
      }) as EventListener);

      // Listen for draft refresh events (e.g., after Gyp edits spec.md or eval.md)
      window.addEventListener('draft-refresh', ((e: CustomEvent<string>) => {
        if (e.detail === this.runName) {
          this.refreshFiles();
        }
      }) as EventListener);

      // Listen for Tauri native file drop events
      listen<{ paths: string[]; position: { x: number; y: number } }>('tauri://drag-drop', (event) => {
        this.handleNativeFileDrop(event.payload.paths, event.payload.position);
      }).then((unlisten) => {
        this._unlistenDragDrop = unlisten;
      });

      // Debounced visual feedback for native drag events
      let dragShowTimeout: ReturnType<typeof setTimeout> | null = null;
      let dragHideTimeout: ReturnType<typeof setTimeout> | null = null;

      listen('tauri://drag-enter', () => {
        // Clear any pending hide
        if (dragHideTimeout) {
          clearTimeout(dragHideTimeout);
          dragHideTimeout = null;
        }
        // Debounce show - wait 100ms before showing indicator
        if (!dragShowTimeout && this.runName) {
          dragShowTimeout = setTimeout(() => {
            if (this.activeTab === 'spec') {
              this.specDragOver = true;
            } else {
              this.evalDragOver = true;
            }
            dragShowTimeout = null;
          }, 100);
        }
      });

      listen('tauri://drag-leave', () => {
        // Clear any pending show
        if (dragShowTimeout) {
          clearTimeout(dragShowTimeout);
          dragShowTimeout = null;
        }
        // Debounce hide - wait 50ms before hiding (handles flickering)
        if (!dragHideTimeout) {
          dragHideTimeout = setTimeout(() => {
            this.specDragOver = false;
            this.evalDragOver = false;
            dragHideTimeout = null;
          }, 50);
        }
      });
    },

    /**
     * Cleanup when component is destroyed
     */
    destroy(): void {
      if (this.saveTimeout) {
        clearTimeout(this.saveTimeout);
        this.saveTimeout = null;
      }
      if (this.evalSaveTimeout) {
        clearTimeout(this.evalSaveTimeout);
        this.evalSaveTimeout = null;
      }
      if (this.repoValidateTimeout) {
        clearTimeout(this.repoValidateTimeout);
        this.repoValidateTimeout = null;
      }
      if (this._unlistenDragDrop) {
        this._unlistenDragDrop();
        this._unlistenDragDrop = null;
      }
    },

    /**
     * Load available runners from config
     */
    async loadRunners(): Promise<void> {
      try {
        if (!window.tauriInvoke) return;

        const config = await window.tauriInvoke<{
          runners: Record<string, RunnerConfig>;
          defaultRunner: string | null;
        }>('get_config');

        // Build runner list with "Local" always first
        const runners: RunnerEntry[] = [
          { name: 'local', config: { type: 'local' } }
        ];

        // Add configured runners
        for (const [name, runnerConfig] of Object.entries(config.runners || {})) {
          runners.push({ name, config: runnerConfig });
        }

        this.availableRunners = runners;

        // Set default runner if not already selected
        if (!this.selectedRunner) {
          this.selectedRunner = config.defaultRunner || 'local';
        }
      } catch (err) {
        console.error('Failed to load runners:', err);
      }
    },

    /**
     * Get icon name for runner type
     */
    getRunnerIcon(runner: RunnerEntry | null): string {
      if (!runner) return 'laptop';
      switch (runner.config.type) {
        case 'local': return 'laptop';
        case 'ssh': return 'server';
        case 'sprite': return 'cloud';
        default: return 'laptop';
      }
    },

    /**
     * Get display name for runner
     */
    getRunnerDisplayName(runner: RunnerEntry | null): string {
      if (!runner) return 'Local';
      if (runner.name === 'local') return 'Local';
      return runner.name;
    },

    /**
     * Load a draft for editing
     */
    async loadDraft(name: string): Promise<void> {
      this.loading = true;
      this.error = null;
      this.runName = name;

      try {
        // Load run metadata from database
        const detail = await getRunDetail(name);
        this.name = detail.name;
        this.workerScale = detail.workerScale || '1';
        this.timeLimitMinutes = detail.timeLimitMinutes;
        this.timeLimitInput = formatTimeLimitDisplay(detail.timeLimitMinutes);
        this.humanInTheLoop = detail.humanInTheLoop;
        this.projectPath = detail.projectPath || '';
        this.selectedRunner = detail.runner || 'local';

        // Load spec, eval, and assets path (file-first editing)
        const [specContent, evalContent, assetsPath] = await Promise.all([
          readSpecFile(name),
          readEvalFile(name),
          getAssetsPath(name),
        ]);
        this.spec = specContent;
        this.eval = evalContent;
        this.assetsPath = assetsPath;

        this.loading = false;

        // Validate project path if set (this will populate branches)
        if (this.projectPath) {
          this.validateProjectPath();
        }
      } catch (err) {
        this.error = err instanceof Error ? err.message : String(err);
        this.loading = false;
      }
    },

    /**
     * Clear the draft editor
     */
    clearDraft(): void {
      if (this.saveTimeout) {
        clearTimeout(this.saveTimeout);
        this.saveTimeout = null;
      }
      if (this.evalSaveTimeout) {
        clearTimeout(this.evalSaveTimeout);
        this.evalSaveTimeout = null;
      }
      if (this.repoValidateTimeout) {
        clearTimeout(this.repoValidateTimeout);
        this.repoValidateTimeout = null;
      }
      this.runName = null;
      this.name = '';
      this.spec = '';
      this.eval = '';
      this.workerScale = '1';
      this.timeLimitMinutes = null;
      this.timeLimitInput = '';
      this.humanInTheLoop = true;
      this.projectPath = '';
      this.loading = false;
      this.error = null;
      // Reset branch state
      this.selectedBranch = '';
      this.availableBranches = [];
      this.repoValidating = false;
      this.repoError = null;
      this.repoIsRemote = false;
      this.normalizedRepoUrl = '';
      // Reset validation errors
      this.workerScaleError = null;
      this.timeLimitError = null;
      // Reset setup flags
      this.needsDirCreate = false;
      this.needsGitInit = false;
      // Reset drag state
      this.specDragOver = false;
      this.evalDragOver = false;
      this.specDragCounter = 0;
      this.evalDragCounter = 0;
      // Reset assets path
      this.assetsPath = '';
      // Reset runner (keep availableRunners, just reset selection to default)
      this.selectedRunner = this.availableRunners.length > 0 ? 'local' : null;
    },

    /**
     * Refresh spec and eval content from files (after Gyp edits)
     */
    async refreshFiles(): Promise<void> {
      if (!this.runName) return;

      try {
        const [specContent, evalContent] = await Promise.all([
          readSpecFile(this.runName),
          readEvalFile(this.runName),
        ]);

        let updated = false;
        if (specContent !== this.spec) {
          this.spec = specContent;
          updated = true;
        }
        if (evalContent !== this.eval) {
          this.eval = evalContent;
          updated = true;
        }
        if (updated) {
          window.toast?.info('Draft files refreshed');
        }
      } catch (err) {
        console.error('Failed to refresh files:', err);
      }
    },

    /**
     * Save spec to file
     */
    async saveSpec(): Promise<void> {
      if (!this.runName || this.savingSpec) return;
      this.savingSpec = true;
      try {
        await writeSpecFile(this.runName, this.spec);
      } catch (err) {
        console.error('Failed to save spec:', err);
        window.toast?.error('Failed to save spec');
      } finally {
        this.savingSpec = false;
      }
    },

    /**
     * Save eval to file
     */
    async saveEval(): Promise<void> {
      if (!this.runName || this.savingEval) return;
      this.savingEval = true;
      try {
        await writeEvalFile(this.runName, this.eval);
      } catch (err) {
        console.error('Failed to save eval:', err);
        window.toast?.error('Failed to save eval');
      } finally {
        this.savingEval = false;
      }
    },

    /**
     * Debounced save spec - waits 500ms after last change
     */
    debouncedSaveSpec(): void {
      if (this.saveTimeout) {
        clearTimeout(this.saveTimeout);
      }
      this.saveTimeout = setTimeout(() => {
        this.saveSpec();
      }, 500);
    },

    /**
     * Debounced save eval - waits 500ms after last change
     */
    debouncedSaveEval(): void {
      if (this.evalSaveTimeout) {
        clearTimeout(this.evalSaveTimeout);
      }
      this.evalSaveTimeout = setTimeout(() => {
        this.saveEval();
      }, 500);
    },

    /**
     * Validate time limit input and update error state
     */
    validateTimeLimit(): boolean {
      const result = parseTimeLimit(this.timeLimitInput);
      this.timeLimitError = result.error;
      return result.error === null;
    },

    /**
     * Validate worker scale input and update error state
     */
    validateWorkerScale(): boolean {
      const result = validateWorkerScale(this.workerScale);
      this.workerScaleError = result.error;
      return result.error === null;
    },

    /**
     * Check if all fields are valid
     */
    hasValidationErrors(): boolean {
      return !!(this.workerScaleError || this.timeLimitError || this.repoError);
    },

    /**
     * Save draft config (not spec/eval - those are file-based)
     */
    async saveDraft(): Promise<void> {
      if (!this.runName || this.saving) return;

      // Validate fields first
      this.validateWorkerScale();
      this.validateTimeLimit();

      // Don't save if there are validation errors
      if (this.workerScaleError || this.timeLimitError) {
        return;
      }

      this.saving = true;

      try {
        const timeLimitResult = parseTimeLimit(this.timeLimitInput);
        const updates: DraftUpdateRequest = {
          // Note: spec is NOT saved here - it's file-based via saveSpec()
          workerScale: this.workerScale,
          timeLimitMinutes: timeLimitResult.value ?? undefined,
          humanInTheLoop: this.humanInTheLoop,
          projectPath: this.projectPath || undefined,
          runner: this.selectedRunner || undefined,
        };

        // Remove undefined values
        Object.keys(updates).forEach((key) => {
          if (updates[key as keyof DraftUpdateRequest] === undefined) {
            delete updates[key as keyof DraftUpdateRequest];
          }
        });

        await updateDraft(this.runName, updates);
        this.timeLimitMinutes = timeLimitResult.value;
        this.saving = false;
      } catch (err) {
        this.error = err instanceof Error ? err.message : String(err);
        this.saving = false;
        window.toast?.error('Failed to save draft', this.error);
      }
    },

    /**
     * Debounced save - waits 500ms after last change
     */
    debouncedSave(): void {
      if (this.saveTimeout) {
        clearTimeout(this.saveTimeout);
      }
      this.saveTimeout = setTimeout(() => {
        this.saveDraft();
      }, 500);
    },

    /**
     * Start the draft run
     */
    async startRun(): Promise<void> {
      if (!this.runName || this.starting) return;

      // Validate all fields
      this.validateWorkerScale();
      this.validateTimeLimit();

      // Validate required fields
      if (!this.projectPath.trim()) {
        window.toast?.error('Repository path is required');
        return;
      }

      if (!this.selectedBranch) {
        window.toast?.error('Please select a branch');
        return;
      }

      if (this.repoError) {
        window.toast?.error(this.repoError || 'Invalid repository');
        return;
      }

      if (this.workerScaleError) {
        window.toast?.error(this.workerScaleError || 'Invalid workers');
        return;
      }

      if (this.timeLimitError) {
        window.toast?.error(this.timeLimitError || 'Invalid time limit');
        return;
      }

      // If project needs setup, confirm with user first
      if (this.needsDirCreate || this.needsGitInit) {
        const setupActions: string[] = [];
        if (this.needsDirCreate) {
          setupActions.push('create the directory');
        }
        if (this.needsGitInit) {
          setupActions.push('initialize a git repository');
        }

        const confirmed = await showConfirm({
          title: 'Initialize Project Directory?',
          message: `The path "${this.projectPath}" doesn't exist or isn't a git repository. Hirsel will ${setupActions.join(' and ')} with an initial commit on the "main" branch. Continue?`,
          confirmText: 'Initialize & Start',
          cancelText: 'Cancel',
        });

        if (!confirmed) {
          return;
        }

        // Initialize the project
        try {
          const result = await initProjectRepo(this.projectPath);

          if (!result.valid) {
            window.toast?.error(result.error || 'Failed to initialize project');
            return;
          }

          // Update state with initialized repo
          this.needsDirCreate = false;
          this.needsGitInit = false;
          this.availableBranches = result.branches;
          this.selectedBranch = result.currentBranch || 'main';
        } catch (err) {
          window.toast?.error('Failed to initialize project');
          return;
        }
      }

      this.starting = true;
      this.error = null;

      try {
        // Save any pending changes first (including branch)
        await this.saveDraft();

        // Start the draft
        const detail = await startDraft(this.runName);

        window.toast?.success(`Run "${detail.name}" started`);

        // Dispatch event to notify that run has started (triggers view switch)
        window.dispatchEvent(new CustomEvent('run-started', { detail: detail.name }));
        window.dispatchEvent(new CustomEvent('run-selected', { detail: detail.name }));
        // Clear the draft editor since run is no longer a draft
        window.dispatchEvent(new CustomEvent('draft-selected', { detail: null }));

        this.starting = false;
      } catch (err) {
        this.error = err instanceof Error ? err.message : String(err);
        this.starting = false;
        window.toast?.error('Failed to start run');
      }
    },

    /**
     * Delete the draft
     */
    async deleteDraft(): Promise<void> {
      if (!this.runName) return;

      const confirmed = await window.confirmDialog?.delete(this.name, 'draft')
        ?? confirm(`Delete draft "${this.name}"? This cannot be undone.`);
      if (!confirmed) return;

      try {
        await deleteRun(this.runName);
        window.toast?.info(`Draft "${this.name}" deleted`);

        // Clear selection
        window.dispatchEvent(new CustomEvent('run-selected', { detail: null }));
        this.clearDraft();
      } catch (err) {
        const error = err instanceof Error ? err.message : String(err);
        window.toast?.error('Failed to delete draft');
      }
    },

    /**
     * Rename the draft
     */
    async renameDraft(newName: string): Promise<void> {
      if (!this.runName || !newName.trim() || newName.trim() === this.runName) {
        this.name = this.runName || '';
        return;
      }

      try {
        await updateDraft(this.runName, { name: newName.trim() });
        const oldName = this.runName;
        this.runName = newName.trim();
        this.name = newName.trim();

        // Notify that the run was renamed
        window.dispatchEvent(
          new CustomEvent('run-renamed', {
            detail: { oldName, newName: newName.trim() },
          })
        );
        window.dispatchEvent(new CustomEvent('run-selected', { detail: newName.trim() }));
      } catch (err) {
        const error = err instanceof Error ? err.message : String(err);
        window.toast?.error('Failed to rename draft');
        this.name = this.runName || '';
      }
    },

    /**
     * Format time limit for display
     */
    formatTimeLimitDisplay,

    /**
     * Handle starting name edit
     */
    handleNameEdit(): void {
      this.isEditing = true;
    },

    /**
     * Finish editing name
     */
    finishNameEdit(): void {
      this.isEditing = false;
      if (this.name !== this.runName && this.name.trim()) {
        this.renameDraft(this.name);
      }
    },

    /**
     * Render markdown content to HTML (sanitized for XSS protection)
     * Rewrites assets/ image URLs to Tauri asset URLs for webview
     */
    renderMarkdown(content: string): string {
      if (!content) return '<p class="text-wool-500 italic">No content</p>';

      let html = marked(content) as string;

      // Rewrite assets/ URLs to Tauri asset URLs for webview
      if (this.assetsPath) {
        // Match src="assets/..." or src='assets/...'
        html = html.replace(
          /src=(["'])assets\/([^"']+)\1/g,
          (_match, quote, filename) => {
            const filePath = `${this.assetsPath}/${filename}`;
            const fileUrl = convertFileSrc(filePath);
            return `src=${quote}${fileUrl}${quote}`;
          }
        );
      }

      // Sanitize but allow Tauri's asset protocol URLs
      return DOMPurify.sanitize(html, {
        ADD_URI_SAFE_ATTR: ['src'],
        ALLOWED_URI_REGEXP: /^(?:(?:https?|asset|tauri):\/\/|data:image\/)/i,
      });
    },

    /**
     * Validate the project path and populate branch information
     */
    async validateProjectPath(): Promise<void> {
      const path = this.projectPath.trim();

      // Reset state if path is empty
      if (!path) {
        this.availableBranches = [];
        this.selectedBranch = '';
        this.repoError = null;
        this.repoValidating = false;
        this.repoIsRemote = false;
        this.normalizedRepoUrl = '';
        this.needsDirCreate = false;
        this.needsGitInit = false;
        return;
      }

      this.repoValidating = true;
      this.repoError = null;
      this.needsDirCreate = false;
      this.needsGitInit = false;

      try {
        // Add timeout to prevent hanging on slow/unresponsive remotes
        const timeoutMs = 15000;
        const result = await Promise.race([
          validateRepo(path),
          new Promise<never>((_, reject) =>
            setTimeout(() => reject(new Error('Validation timed out')), timeoutMs)
          )
        ]);

        this.repoIsRemote = result.isRemote;
        this.normalizedRepoUrl = result.repoUrl;

        // Check for setup flags (local paths that need initialization)
        this.needsDirCreate = result.needsDirCreate;
        this.needsGitInit = result.needsGitInit;

        if (!result.valid && !this.needsDirCreate && !this.needsGitInit) {
          // Only show error if it's not a "needs setup" situation
          this.repoError = result.error || 'Invalid repository';
          this.availableBranches = result.branches;
          this.selectedBranch = '';
        } else if (this.needsDirCreate || this.needsGitInit) {
          // Path needs initialization - not an error, but needs setup
          this.repoError = null;
          this.availableBranches = ['main']; // Will be created on init
          this.selectedBranch = 'main';
        } else {
          this.repoError = null;
          this.availableBranches = result.branches;

          // Update projectPath to normalized URL (strip branch from URL)
          if (result.repoUrl !== path) {
            this.projectPath = result.repoUrl;
          }

          // Set default branch selection
          if (result.urlBranch && result.urlBranchValid) {
            // Branch was in URL and exists - preselect it
            this.selectedBranch = result.urlBranch;
          } else if (result.currentBranch && !result.isRemote) {
            // Local repo - select currently checked out branch
            this.selectedBranch = result.currentBranch;
          } else {
            // Remote without branch in URL - clear selection (user must choose)
            this.selectedBranch = '';
          }
        }
      } catch (err) {
        this.repoError = err instanceof Error ? err.message : 'Failed to validate repository';
        this.availableBranches = [];
        this.selectedBranch = '';
        this.needsDirCreate = false;
        this.needsGitInit = false;
      } finally {
        this.repoValidating = false;
      }
    },

    /**
     * Debounced validation - waits 500ms after last change
     */
    debouncedValidateProjectPath(): void {
      if (this.repoValidateTimeout) {
        clearTimeout(this.repoValidateTimeout);
      }
      this.repoValidateTimeout = setTimeout(() => {
        this.validateProjectPath();
      }, 500);
    },

    /**
     * Handle branch selection change
     */
    onBranchChange(): void {
      // Save draft when branch changes
      this.debouncedSave();
    },

    /**
     * Check if run can be started
     */
    canStart(): boolean {
      // Can start if we have a valid path + branch OR if path needs setup
      const hasValidRepo = !this.repoError && this.selectedBranch;
      const needsSetup = this.needsDirCreate || this.needsGitInit;

      return Boolean(
        this.projectPath.trim() &&
        (hasValidRepo || needsSetup) &&
        !this.repoValidating &&
        !this.starting &&
        !this.workerScaleError &&
        !this.timeLimitError
      );
    },

    /**
     * Handle drag enter event - increment counter to track nested elements
     */
    handleDragEnter(type: 'spec' | 'eval', event: DragEvent): void {
      event.preventDefault();
      event.stopPropagation();

      // Only handle file drags
      if (!event.dataTransfer?.types.includes('Files')) return;

      if (type === 'spec') {
        this.specDragCounter++;
        this.specDragOver = true;
      } else {
        this.evalDragCounter++;
        this.evalDragOver = true;
      }
    },

    /**
     * Handle drag over event - just prevent default to allow drop
     */
    handleDragOver(type: 'spec' | 'eval', event: DragEvent): void {
      event.preventDefault();
      event.stopPropagation();

      if (event.dataTransfer?.types.includes('Files')) {
        event.dataTransfer.dropEffect = 'copy';
      }
    },

    /**
     * Handle drag leave event - decrement counter
     */
    handleDragLeave(type: 'spec' | 'eval', event: DragEvent): void {
      event.preventDefault();
      event.stopPropagation();

      if (type === 'spec') {
        this.specDragCounter--;
        if (this.specDragCounter <= 0) {
          this.specDragCounter = 0;
          this.specDragOver = false;
        }
      } else {
        this.evalDragCounter--;
        if (this.evalDragCounter <= 0) {
          this.evalDragCounter = 0;
          this.evalDragOver = false;
        }
      }
    },

    /**
     * Open file picker to select a file
     */
    openFilePicker(type: 'spec' | 'eval'): void {
      const input = document.createElement('input');
      input.type = 'file';
      input.accept = '.md,.txt,.markdown,text/*';
      input.onchange = async () => {
        const file = input.files?.[0];
        if (!file) return;

        try {
          const content = await file.text();
          const currentContent = type === 'spec' ? this.spec : this.eval;
          const fieldName = type === 'spec' ? 'Specification' : 'Evaluation';

          if (currentContent.trim()) {
            const confirmed = await showConfirm({
              title: `Replace ${fieldName}?`,
              message: `This will replace the current ${fieldName.toLowerCase()} content with the contents of "${file.name}". This cannot be undone.`,
              confirmText: 'Replace',
              cancelText: 'Cancel',
              danger: true,
            });

            if (!confirmed) return;
          }

          if (type === 'spec') {
            this.spec = content;
            this.debouncedSaveSpec();
          } else {
            this.eval = content;
            this.debouncedSaveEval();
          }

          window.toast?.success(`Loaded "${file.name}" into ${fieldName.toLowerCase()}`);
        } catch (err) {
          console.error('Failed to read file:', err);
          window.toast?.error('Failed to read file');
        }
      };
      input.click();
    },

    /**
     * Handle file drop for spec or eval
     * Any file dropped is saved to assets/ and a reference is inserted.
     * Images use markdown image syntax, other files use link syntax.
     */
    async handleFileDrop(type: 'spec' | 'eval', event: DragEvent): Promise<void> {
      event.preventDefault();
      event.stopPropagation();

      // Reset drag state
      this.specDragOver = false;
      this.evalDragOver = false;
      this.specDragCounter = 0;
      this.evalDragCounter = 0;

      const files = event.dataTransfer?.files;
      if (!files || files.length === 0) return;
      if (!this.runName) {
        window.toast?.error('No run selected');
        return;
      }

      // Process all dropped files
      for (const file of Array.from(files)) {
        try {
          // Save file to assets
          const arrayBuffer = await file.arrayBuffer();
          const data = Array.from(new Uint8Array(arrayBuffer));
          const savedFilename = await saveAsset(this.runName, file.name, data);

          // Determine if it's an image for markdown syntax
          const isImage = file.type.startsWith('image/') ||
            /\.(png|jpg|jpeg|gif|webp|svg|bmp|ico)$/i.test(file.name);

          // Create appropriate markdown reference
          const markdownRef = isImage
            ? `![${savedFilename}](assets/${savedFilename})`
            : `[${savedFilename}](assets/${savedFilename})`;

          // Insert reference
          if (type === 'spec') {
            this.spec = this.spec ? `${this.spec}\n\n${markdownRef}` : markdownRef;
          } else {
            this.eval = this.eval ? `${this.eval}\n\n${markdownRef}` : markdownRef;
          }

          window.toast?.success(`Added: ${savedFilename}`);
        } catch (err) {
          console.error('Failed to save file:', err);
          window.toast?.error(`Failed to save: ${file.name}`);
        }
      }

      // Save after all files processed
      if (type === 'spec') {
        this.debouncedSaveSpec();
      } else {
        this.debouncedSaveEval();
      }
    },

    /**
     * Track cursor position in textarea for insertion
     */
    trackCursorPosition(type: 'spec' | 'eval', event: Event): void {
      const textarea = event.target as HTMLTextAreaElement;
      if (textarea && typeof textarea.selectionStart === 'number') {
        if (type === 'spec') {
          this.specCursorPos = textarea.selectionStart;
        } else {
          this.evalCursorPos = textarea.selectionStart;
        }
      }
    },

    /**
     * Handle native file drop from Tauri (file manager drag-drop)
     * Uses filesystem paths directly instead of transferring file contents
     * Inserts at the drop position (calculated from screen coordinates)
     */
    async handleNativeFileDrop(paths: string[], position: { x: number; y: number }): Promise<void> {
      // Reset drag state
      this.specDragOver = false;
      this.evalDragOver = false;

      if (!paths || paths.length === 0) return;
      if (!this.runName) {
        window.toast?.error('No run selected');
        return;
      }

      // Determine which tab to add to based on active tab
      const type = this.activeTab;

      // Build all markdown references first
      const markdownRefs: string[] = [];
      for (const filePath of paths) {
        try {
          // Import file from path (Tauri handles the file reading)
          const savedFilename = await importAssetFromPath(this.runName, filePath);

          // Determine if it's an image for markdown syntax
          const isImage = /\.(png|jpg|jpeg|gif|webp|svg|bmp|ico)$/i.test(savedFilename);

          // Create appropriate markdown reference
          const markdownRef = isImage
            ? `![${savedFilename}](assets/${savedFilename})`
            : `[${savedFilename}](assets/${savedFilename})`;

          markdownRefs.push(markdownRef);
          window.toast?.success(`Added: ${savedFilename}`);
        } catch (err) {
          console.error('Failed to import file:', err);
          const filename = filePath.split('/').pop() || filePath;
          window.toast?.error(`Failed to import: ${filename}`);
        }
      }

      if (markdownRefs.length === 0) return;

      const content = type === 'spec' ? this.spec : this.eval;
      const insertion = markdownRefs.join('\n');

      // Try to find the textarea and calculate drop line from position
      const textareaSelector = type === 'spec'
        ? 'textarea[x-model="spec"]'
        : 'textarea[x-model="eval"]';
      const textarea = document.querySelector(textareaSelector) as HTMLTextAreaElement | null;

      let insertPos = content.length; // Default to end

      if (textarea) {
        const rect = textarea.getBoundingClientRect();
        const relativeY = position.y - rect.top;

        // Get computed style for line height
        const style = window.getComputedStyle(textarea);
        const lineHeight = parseFloat(style.lineHeight) || parseFloat(style.fontSize) * 1.2;
        const paddingTop = parseFloat(style.paddingTop) || 0;

        // Calculate which line was dropped on (accounting for scroll)
        const scrollTop = textarea.scrollTop;
        const adjustedY = relativeY + scrollTop - paddingTop;
        const targetLine = Math.max(0, Math.floor(adjustedY / lineHeight));

        // Find the character position at the start of that line
        const lines = content.split('\n');
        let charPos = 0;
        for (let i = 0; i < Math.min(targetLine, lines.length); i++) {
          charPos += lines[i].length + 1; // +1 for newline
        }
        insertPos = Math.min(charPos, content.length);
      }

      // Get the line at insert position
      const before = content.slice(0, insertPos);
      const after = content.slice(insertPos);

      // Check if we're at the start of a line
      const atLineStart = insertPos === 0 || content[insertPos - 1] === '\n';

      // Find end of current line
      const nextNewline = after.indexOf('\n');
      const currentLineContent = nextNewline === -1 ? after : after.slice(0, nextNewline);
      const isLineEmpty = currentLineContent.trim() === '';

      let newContent: string;

      if (atLineStart && isLineEmpty) {
        // At start of empty line - just insert
        newContent = before + insertion + after;
      } else if (atLineStart) {
        // At start of non-empty line - insert before with newline after
        newContent = before + insertion + '\n' + after;
      } else {
        // In middle of content - insert on new line
        newContent = before + '\n' + insertion + after;
      }

      // Update content
      if (type === 'spec') {
        this.spec = newContent;
        this.debouncedSaveSpec();
      } else {
        this.eval = newContent;
        this.debouncedSaveEval();
      }
    },

    /**
     * Open the assets folder for this run in the system file browser
     */
    async openAssets(): Promise<void> {
      if (!this.runName) {
        window.toast?.error('No run selected');
        return;
      }
      try {
        await openAssetsFolder(this.runName);
      } catch (err) {
        console.error('Failed to open assets folder:', err);
        window.toast?.error('Failed to open assets folder');
      }
    },
  };
}
