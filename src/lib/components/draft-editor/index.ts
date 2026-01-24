/**
 * Draft Editor Alpine.js Component
 *
 * A dedicated editor view for configuring draft runs before starting them.
 * Provides controls for spec, worker scale, time limit, HITL mode, and project path.
 */

import { convertFileSrc } from '@tauri-apps/api/core';
import { type UnlistenFn, listen } from '@tauri-apps/api/event';
import DOMPurify from 'dompurify';
import { marked } from 'marked';
import {
  changeStartingPoint,
  createDraft,
  deleteRun,
  getAssetsPath,
  getRunDetail,
  openAssetsFolder,
  pickFolder,
  readEvalFile,
  readSpecFile,
  startDraft,
  suggestPaths,
  updateDraft,
  validateRepo,
  writeEvalFile,
  writeSpecFile,
} from '../../api';
import { showConfirm } from '../../confirm-dialog';
import type { StartingPoint } from '../../types';
import type { DraftUpdateRequest, RunDetail, RunnerConfig, RunnerEntry } from '../../types';

import {
  calculateInsertPosition,
  insertAtPosition,
  processDroppedFile,
  processNativeFilePath,
} from './file-handling';
import type { DraftEditorComponent, DraftEditorData } from './types';
// Import extracted utilities
import {
  formatTimeLimitDisplay,
  parseTimeLimit,
  validateWorkerScale as validateWorkerScaleUtil,
} from './validation';

// Re-export types and utilities for external use
export * from './types';
export * from './validation';

declare const window: Window & {
  tauriInvoke?: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  toast: {
    success: (message: string, title?: string) => void;
    error: (message: string, title?: string) => void;
    info: (message: string, title?: string) => void;
  };
  confirmDialog?: {
    delete: (name: string, type: string) => Promise<boolean>;
  };
};

/**
 * Alpine.js component factory for draft editor
 */
export function draftEditor(): DraftEditorComponent {
  return {
    runName: null,
    name: '',
    spec: '',
    eval: '',
    workerScale: '1',
    timeLimitMinutes: null,
    timeLimitInput: '',
    humanInTheLoop: true,
    // Starting point selection phase
    startingPointChosen: false,
    // Starting point configuration
    startingPointType: 'greenfield' as const,
    localPath: '',
    gitUrl: '',
    gitBranch: '',
    workspacePath: null as string | null,
    saving: false,
    savingSpec: false,
    savingEval: false,
    starting: false,
    loading: false,
    error: null,
    saveTimeout: null,
    specSaveTimeout: null,
    evalSaveTimeout: null,
    isEditing: false,
    activeTab: 'spec' as const,
    previewMode: true,
    isNewDraft: false,
    // Git URL validation state
    gitValidating: false,
    gitError: null as string | null,
    availableBranches: [] as string[],
    gitValidateTimeout: null as ReturnType<typeof setTimeout> | null,
    // Change starting point dialog state
    showChangeStartingPointDialog: false,
    changingStartingPoint: false,
    newStartingPointType: 'greenfield' as 'greenfield' | 'local' | 'git',
    newLocalPath: '',
    newGitUrl: '',
    newGitBranch: '',
    newGitValidating: false,
    newGitError: null as string | null,
    newAvailableBranches: [] as string[],
    newGitValidateTimeout: null as ReturnType<typeof setTimeout> | null,
    // Path autocomplete state
    pathSuggestions: [] as string[],
    pathSuggestionsLoading: false,
    pathSuggestTimeout: null as ReturnType<typeof setTimeout> | null,
    showPathSuggestions: false,
    selectedSuggestionIndex: -1,
    // Field validation errors
    workerScaleError: null,
    timeLimitError: null,
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
    runnerDefault: null,
    workerRunners: {} as Record<string, string>,
    availableRunners: [],

    /**
     * Initialize the component
     */
    init(): void {
      // Load available runners from config
      this.loadRunners();

      // Listen for draft-created to know this is a newly created draft
      window.addEventListener('draft-created', (() => {
        this.isNewDraft = true;
      }) as EventListener);

      // Listen for draft selection events
      window.addEventListener('draft-selected', ((e: CustomEvent<string | null>) => {
        if (e.detail) {
          this.loadDraft(e.detail);
        } else {
          this.clearDraft();
        }
      }) as EventListener);

      // Also listen for run-selected to detect draft selection
      window.addEventListener('run-selected', ((_e: CustomEvent<string | null>) => {
        // The run-list component will dispatch draft-selected when a draft is selected
        // This handler is for cleanup when switching away from a draft
      }) as EventListener);

      // Listen for run deletion (e.g., via CLI) to clear draft if it was deleted
      window.addEventListener('data:run-deleted', ((e: CustomEvent<string>) => {
        if (e.detail === this.runName) {
          this.clearDraft();
        }
      }) as EventListener);

      // Defensive: check if our run still exists when runs list updates
      window.addEventListener('data:runs-updated', ((e: CustomEvent<Array<{ name: string }>>) => {
        if (this.runName && e.detail) {
          const stillExists = e.detail.some((r) => r.name === this.runName);
          if (!stillExists) {
            this.clearDraft();
          }
        }
      }) as EventListener);

      // Listen for draft refresh events (e.g., after Gyp edits spec.md or eval.md)
      window.addEventListener('draft-refresh', ((e: CustomEvent<string>) => {
        if (e.detail === this.runName) {
          this.refreshFiles();
        }
      }) as EventListener);

      // Listen for Tauri native file drop events
      listen<{ paths: string[]; position: { x: number; y: number } }>(
        'tauri://drag-drop',
        (event) => {
          this.handleNativeFileDrop(event.payload.paths, event.payload.position);
        },
      ).then((unlisten) => {
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
      if (this.specSaveTimeout) {
        clearTimeout(this.specSaveTimeout);
        this.specSaveTimeout = null;
      }
      if (this.evalSaveTimeout) {
        clearTimeout(this.evalSaveTimeout);
        this.evalSaveTimeout = null;
      }
      if (this.gitValidateTimeout) {
        clearTimeout(this.gitValidateTimeout);
        this.gitValidateTimeout = null;
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
        const runners: RunnerEntry[] = [{ name: 'local', config: { host: { type: 'local' } } }];

        // Add configured runners
        for (const [name, runnerConfig] of Object.entries(config.runners || {})) {
          runners.push({ name, config: runnerConfig });
        }

        this.availableRunners = runners;

        // Set default runner if not already selected
        if (!this.runnerDefault) {
          this.runnerDefault = config.defaultRunner || 'local';
        }
      } catch (err) {
        console.error('Failed to load runners:', err);
      }
    },

    /**
     * Get worker names based on workerScale
     * workerScale can be "1", "2", "3", or a range like "1-4"
     */
    getWorkerNames(): string[] {
      const scale = this.workerScale || '1';
      let maxWorkers = 1;

      // Handle range format "min-max" or single number
      if (scale.includes('-')) {
        const parts = scale.split('-');
        maxWorkers = Number.parseInt(parts[1], 10) || 1;
      } else {
        maxWorkers = Number.parseInt(scale, 10) || 1;
      }

      // Generate worker names
      const names: string[] = [];
      for (let i = 1; i <= maxWorkers; i++) {
        names.push(`worker-${i}`);
      }
      return names;
    },

    /**
     * Get runner for a specific worker (falls back to run default)
     */
    getWorkerRunner(workerName: string): string {
      return this.workerRunners[workerName] || this.runnerDefault || 'local';
    },

    /**
     * Set runner for a specific worker
     */
    setWorkerRunner(workerName: string, runnerName: string): void {
      // If same as default, remove override
      if (runnerName === this.runnerDefault) {
        delete this.workerRunners[workerName];
      } else {
        this.workerRunners[workerName] = runnerName;
      }
      this.debouncedSave();
    },

    /**
     * Apply current default runner to all workers
     */
    applyRunnerToAll(): void {
      // Clear all overrides - all workers will use the default
      this.workerRunners = {};
      this.debouncedSave();
    },

    /**
     * Check if any workers have custom runner assignments
     */
    hasCustomRunnerAssignments(): boolean {
      return Object.keys(this.workerRunners).length > 0;
    },

    /**
     * Get icon name for runner type (based on host type)
     */
    getRunnerIcon(runner: RunnerEntry | null): string {
      if (!runner) return 'laptop';
      const hostType = runner.config.host.type;
      switch (hostType) {
        case 'local':
          return 'laptop';
        case 'client':
          return 'monitor';
        case 'ssh':
          return 'server';
        case 'fly':
          return 'plane';
        default:
          return 'laptop';
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
        this.workspacePath = detail.projectPath || null;
        // If workspace exists, starting point was already chosen (e.g., from clone_run)
        // Otherwise, show the starting point selection phase
        this.startingPointChosen = Boolean(detail.projectPath);
        this.runnerDefault = detail.runner || 'local';
        this.workerRunners = detail.workerRunners || {};

        // Load spec, eval, and assets path (file-first editing)
        const [specContent, evalContent, assetsPath] = await Promise.all([
          readSpecFile(name),
          readEvalFile(name),
          getAssetsPath(name),
        ]);
        this.spec = specContent;
        this.eval = evalContent;
        this.assetsPath = assetsPath;

        // Set preview mode: edit mode for new drafts, view mode for existing with >5 lines
        if (this.isNewDraft) {
          this.previewMode = false;
          this.isNewDraft = false;
        } else {
          // Count non-empty lines in spec
          const lineCount = specContent.split('\n').filter((line) => line.trim()).length;
          this.previewMode = lineCount > 5;
        }

        this.loading = false;
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
      if (this.specSaveTimeout) {
        clearTimeout(this.specSaveTimeout);
        this.specSaveTimeout = null;
      }
      if (this.evalSaveTimeout) {
        clearTimeout(this.evalSaveTimeout);
        this.evalSaveTimeout = null;
      }
      if (this.gitValidateTimeout) {
        clearTimeout(this.gitValidateTimeout);
        this.gitValidateTimeout = null;
      }
      this.isNewDraft = false;
      this.runName = null;
      this.name = '';
      this.spec = '';
      this.eval = '';
      this.workerScale = '1';
      this.timeLimitMinutes = null;
      this.timeLimitInput = '';
      this.humanInTheLoop = true;
      // Reset starting point phase and configuration
      this.startingPointChosen = false;
      this.startingPointType = 'greenfield';
      this.localPath = '';
      this.gitUrl = '';
      this.gitBranch = '';
      this.workspacePath = null;
      this.loading = false;
      this.saving = false;
      this.savingSpec = false;
      this.savingEval = false;
      this.error = null;
      // Reset git validation state
      this.gitValidating = false;
      this.gitError = null;
      this.availableBranches = [];
      // Reset validation errors
      this.workerScaleError = null;
      this.timeLimitError = null;
      // Reset drag state
      this.specDragOver = false;
      this.evalDragOver = false;
      this.specDragCounter = 0;
      this.evalDragCounter = 0;
      // Reset assets path
      this.assetsPath = '';
      // Reset runner (keep availableRunners, just reset selection to default)
      this.runnerDefault = this.availableRunners.length > 0 ? 'local' : null;
      this.workerRunners = {};
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
      if (this.specSaveTimeout) {
        clearTimeout(this.specSaveTimeout);
      }
      this.specSaveTimeout = setTimeout(() => {
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
      const result = validateWorkerScaleUtil(this.workerScale);
      this.workerScaleError = result.error;
      return result.error === null;
    },

    /**
     * Check if all fields are valid
     */
    hasValidationErrors(): boolean {
      return !!(this.workerScaleError || this.timeLimitError || this.gitError);
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
          // Note: projectPath is NOT saved here - workspace is created during create_draft
          workerScale: this.workerScale,
          timeLimitMinutes: timeLimitResult.value ?? undefined,
          humanInTheLoop: this.humanInTheLoop,
          runner: this.runnerDefault || undefined,
          workerRunners:
            Object.keys(this.workerRunners).length > 0 ? this.workerRunners : undefined,
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

      if (this.workerScaleError) {
        window.toast?.error(this.workerScaleError || 'Invalid workers');
        return;
      }

      if (this.timeLimitError) {
        window.toast?.error(this.timeLimitError || 'Invalid time limit');
        return;
      }

      // For git starting point, validate the URL
      if (this.startingPointType === 'git' && !this.workspacePath) {
        if (!this.gitUrl.trim()) {
          window.toast?.error('Git repository URL is required');
          return;
        }
        if (!this.gitBranch) {
          window.toast?.error('Please select a branch');
          return;
        }
        if (this.gitError) {
          window.toast?.error(this.gitError || 'Invalid repository');
          return;
        }
      }

      // For local folder, validate path exists
      if (this.startingPointType === 'local' && !this.workspacePath) {
        if (!this.localPath.trim()) {
          window.toast?.error('Local folder path is required');
          return;
        }
      }

      this.starting = true;
      this.error = null;

      try {
        // Save any pending changes first
        await this.saveDraft();

        // Build StartingPoint based on selection (only if no workspace exists yet)
        let startingPoint: StartingPoint | undefined;
        if (!this.workspacePath) {
          switch (this.startingPointType) {
            case 'greenfield':
              startingPoint = { type: 'greenfield' };
              break;
            case 'local':
              startingPoint = { type: 'localFolder', path: this.localPath };
              break;
            case 'git':
              startingPoint = {
                type: 'gitRepo',
                url: this.gitUrl,
                branch: this.gitBranch || undefined,
              };
              break;
          }
        }

        // Start the draft with the starting point
        const detail = await startDraft(this.runName, startingPoint);

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

      const confirmed =
        (await window.confirmDialog?.delete(this.name, 'draft')) ??
        confirm(`Delete draft "${this.name}"? This cannot be undone.`);
      if (!confirmed) return;

      try {
        await deleteRun(this.runName);
        window.toast?.info(`Draft "${this.name}" deleted`);

        // Clear selection - dispatch both events
        window.dispatchEvent(new CustomEvent('run-selected', { detail: null }));
        window.dispatchEvent(new CustomEvent('draft-selected', { detail: null }));
        this.clearDraft();
      } catch (err) {
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
          }),
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
        html = html.replace(/src=(["'])assets\/([^"']+)\1/g, (_match, quote, filename) => {
          const filePath = `${this.assetsPath}/${filename}`;
          const fileUrl = convertFileSrc(filePath);
          return `src=${quote}${fileUrl}${quote}`;
        });
      }

      // Sanitize but allow Tauri's asset protocol URLs
      return DOMPurify.sanitize(html, {
        ADD_URI_SAFE_ATTR: ['src'],
        ALLOWED_URI_REGEXP: /^(?:(?:https?|asset|tauri):\/\/|data:image\/)/i,
      });
    },

    /**
     * Validate git URL and fetch available branches
     */
    async validateGitUrl(): Promise<void> {
      const url = this.gitUrl.trim();

      // Reset state if URL is empty
      if (!url) {
        this.availableBranches = [];
        this.gitBranch = '';
        this.gitError = null;
        this.gitValidating = false;
        return;
      }

      this.gitValidating = true;
      this.gitError = null;

      try {
        // Use git ls-remote to list branches (via validateRepo for now, but only for URLs)
        // For the new architecture, we don't need to validate local paths
        // The workspace provider handles that during init
        const timeoutMs = 15000;

        // Import validateRepo only for git URL validation
        const { validateRepo } = await import('../../api');
        const result = await Promise.race([
          validateRepo(url),
          new Promise<never>((_, reject) =>
            setTimeout(() => reject(new Error('Validation timed out')), timeoutMs),
          ),
        ]);

        if (!result.valid) {
          this.gitError = result.error || 'Invalid repository';
          this.availableBranches = [];
          this.gitBranch = '';
        } else {
          this.gitError = null;
          this.availableBranches = result.branches;

          // Set default branch selection
          if (result.urlBranch && result.urlBranchValid) {
            this.gitBranch = result.urlBranch;
          } else if (!this.gitBranch && result.branches.length > 0) {
            // Select first branch (usually main/master)
            this.gitBranch = result.branches[0];
          }
        }
      } catch (err) {
        this.gitError = err instanceof Error ? err.message : 'Failed to validate repository';
        this.availableBranches = [];
        this.gitBranch = '';
      } finally {
        this.gitValidating = false;
      }
    },

    /**
     * Debounced git URL validation - waits 500ms after last change
     */
    debouncedValidateGitUrl(): void {
      if (this.gitValidateTimeout) {
        clearTimeout(this.gitValidateTimeout);
      }
      this.gitValidateTimeout = setTimeout(() => {
        this.validateGitUrl();
      }, 500);
    },

    /**
     * Browse for local folder using native file picker
     */
    async browseLocalFolder(): Promise<void> {
      try {
        const folder = await pickFolder();
        if (folder) {
          this.localPath = folder;
          this.showPathSuggestions = false;
        }
      } catch (err) {
        console.error('Failed to open folder picker:', err);
        window.toast?.error('Failed to open folder picker');
      }
    },

    /**
     * Fetch path suggestions for autocomplete
     */
    async fetchPathSuggestions(): Promise<void> {
      const partial = this.localPath;
      if (!partial || partial.length < 1) {
        this.pathSuggestions = [];
        this.showPathSuggestions = false;
        return;
      }

      this.pathSuggestionsLoading = true;
      try {
        const suggestions = await suggestPaths(partial);
        this.pathSuggestions = suggestions;
        this.showPathSuggestions = suggestions.length > 0;
        this.selectedSuggestionIndex = -1;
      } catch (err) {
        console.error('Failed to fetch path suggestions:', err);
        this.pathSuggestions = [];
        this.showPathSuggestions = false;
      } finally {
        this.pathSuggestionsLoading = false;
      }
    },

    /**
     * Debounced path suggestions
     */
    debouncedFetchPathSuggestions(): void {
      if (this.pathSuggestTimeout) {
        clearTimeout(this.pathSuggestTimeout);
      }
      this.pathSuggestTimeout = setTimeout(() => {
        this.fetchPathSuggestions();
      }, 150);
    },

    /**
     * Select a path suggestion
     */
    selectPathSuggestion(path: string): void {
      this.localPath = path;
      this.showPathSuggestions = false;
      this.selectedSuggestionIndex = -1;
      // Fetch new suggestions for the selected directory
      this.debouncedFetchPathSuggestions();
    },

    /**
     * Handle keyboard navigation in path suggestions
     */
    handlePathKeydown(event: KeyboardEvent): void {
      if (!this.showPathSuggestions || this.pathSuggestions.length === 0) {
        return;
      }

      switch (event.key) {
        case 'ArrowDown':
          event.preventDefault();
          this.selectedSuggestionIndex = Math.min(
            this.selectedSuggestionIndex + 1,
            this.pathSuggestions.length - 1,
          );
          break;
        case 'ArrowUp':
          event.preventDefault();
          this.selectedSuggestionIndex = Math.max(this.selectedSuggestionIndex - 1, -1);
          break;
        case 'Enter':
          if (this.selectedSuggestionIndex >= 0) {
            event.preventDefault();
            this.selectPathSuggestion(this.pathSuggestions[this.selectedSuggestionIndex]);
          }
          break;
        case 'Tab':
          if (this.pathSuggestions.length === 1) {
            event.preventDefault();
            this.selectPathSuggestion(this.pathSuggestions[0]);
          } else if (this.selectedSuggestionIndex >= 0) {
            event.preventDefault();
            this.selectPathSuggestion(this.pathSuggestions[this.selectedSuggestionIndex]);
          }
          break;
        case 'Escape':
          this.showPathSuggestions = false;
          this.selectedSuggestionIndex = -1;
          break;
      }
    },

    /**
     * Hide path suggestions when clicking outside
     */
    hidePathSuggestions(): void {
      // Delay to allow click on suggestion to register
      setTimeout(() => {
        this.showPathSuggestions = false;
        this.selectedSuggestionIndex = -1;
      }, 150);
    },

    /**
     * Check if run can be started
     */
    canStart(): boolean {
      // For existing drafts with workspace already created, can always start
      if (this.workspacePath) {
        return !this.starting && !this.workerScaleError && !this.timeLimitError;
      }

      // For new drafts, check starting point validity
      switch (this.startingPointType) {
        case 'greenfield':
          // Greenfield can always start
          return !this.starting && !this.workerScaleError && !this.timeLimitError;
        case 'local':
          // Local folder needs a path
          return Boolean(
            this.localPath.trim() &&
              !this.starting &&
              !this.workerScaleError &&
              !this.timeLimitError,
          );
        case 'git':
          // Git needs valid URL and selected branch
          return Boolean(
            this.gitUrl.trim() &&
              this.gitBranch &&
              !this.gitError &&
              !this.gitValidating &&
              !this.starting &&
              !this.workerScaleError &&
              !this.timeLimitError,
          );
        default:
          return false;
      }
    },

    /**
     * Check if starting point selection can be confirmed
     */
    canConfirmStartingPoint(): boolean {
      switch (this.startingPointType) {
        case 'greenfield':
          return true;
        case 'local':
          return Boolean(this.localPath.trim());
        case 'git':
          return Boolean(
            this.gitUrl.trim() && this.gitBranch && !this.gitError && !this.gitValidating,
          );
        default:
          return false;
      }
    },

    /**
     * Confirm starting point selection and proceed to main editor
     */
    confirmStartingPoint(): void {
      if (!this.canConfirmStartingPoint()) return;
      this.startingPointChosen = true;
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

          // Set the content and trigger save
          if (type === 'spec') {
            this.spec = content;
            this.debouncedSaveSpec();
          } else {
            this.eval = content;
            this.debouncedSaveEval();
          }

          // Force textarea to update by dispatching input event
          // This ensures x-model binding syncs properly from external changes
          this.$nextTick?.(() => {
            const textarea = document.getElementById(
              type === 'spec' ? 'spec-textarea' : 'eval-textarea',
            ) as HTMLTextAreaElement | null;
            if (textarea) {
              textarea.value = content;
              textarea.dispatchEvent(new Event('input', { bubbles: true }));
            }
          });

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
        const result = await processDroppedFile(this.runName, file);
        if (result) {
          // Insert reference
          if (type === 'spec') {
            this.spec = this.spec ? `${this.spec}\n\n${result.markdownRef}` : result.markdownRef;
          } else {
            this.eval = this.eval ? `${this.eval}\n\n${result.markdownRef}` : result.markdownRef;
          }
          window.toast?.success(`Added: ${result.filename}`);
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
        const result = await processNativeFilePath(this.runName, filePath);
        if (result) {
          markdownRefs.push(result.markdownRef);
          window.toast?.success(`Added: ${result.filename}`);
        }
      }

      if (markdownRefs.length === 0) return;

      const content = type === 'spec' ? this.spec : this.eval;
      const insertion = markdownRefs.join('\n');

      // Try to find the textarea and calculate drop line from position
      const textareaSelector =
        type === 'spec' ? 'textarea[x-model="spec"]' : 'textarea[x-model="eval"]';
      const textarea = document.querySelector(textareaSelector) as HTMLTextAreaElement | null;

      const insertPos = calculateInsertPosition(content, textarea, position.y);
      const newContent = insertAtPosition(content, insertion, insertPos);

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

    /**
     * Open the change starting point dialog
     */
    openChangeStartingPointDialog(): void {
      // Reset dialog state
      this.newStartingPointType = 'greenfield';
      this.newLocalPath = '';
      this.newGitUrl = '';
      this.newGitBranch = '';
      this.newGitValidating = false;
      this.newGitError = null;
      this.newAvailableBranches = [];
      this.showChangeStartingPointDialog = true;
    },

    /**
     * Close the change starting point dialog
     */
    closeChangeStartingPointDialog(): void {
      if (this.newGitValidateTimeout) {
        clearTimeout(this.newGitValidateTimeout);
        this.newGitValidateTimeout = null;
      }
      this.showChangeStartingPointDialog = false;
      this.changingStartingPoint = false;
    },

    /**
     * Validate git URL in the change dialog
     */
    async validateNewGitUrl(): Promise<void> {
      const url = this.newGitUrl.trim();

      if (!url) {
        this.newAvailableBranches = [];
        this.newGitBranch = '';
        this.newGitError = null;
        this.newGitValidating = false;
        return;
      }

      this.newGitValidating = true;
      this.newGitError = null;

      try {
        const timeoutMs = 15000;
        const result = await Promise.race([
          validateRepo(url),
          new Promise<never>((_, reject) =>
            setTimeout(() => reject(new Error('Validation timed out')), timeoutMs),
          ),
        ]);

        if (!result.valid) {
          this.newGitError = result.error || 'Invalid repository';
          this.newAvailableBranches = [];
          this.newGitBranch = '';
        } else {
          this.newGitError = null;
          this.newAvailableBranches = result.branches;

          if (result.urlBranch && result.urlBranchValid) {
            this.newGitBranch = result.urlBranch;
          } else if (!this.newGitBranch && result.branches.length > 0) {
            this.newGitBranch = result.branches[0];
          }
        }
      } catch (err) {
        this.newGitError = err instanceof Error ? err.message : 'Failed to validate repository';
        this.newAvailableBranches = [];
        this.newGitBranch = '';
      } finally {
        this.newGitValidating = false;
      }
    },

    /**
     * Debounced git URL validation for change dialog
     */
    debouncedValidateNewGitUrl(): void {
      if (this.newGitValidateTimeout) {
        clearTimeout(this.newGitValidateTimeout);
      }
      this.newGitValidateTimeout = setTimeout(() => {
        this.validateNewGitUrl();
      }, 500);
    },

    /**
     * Check if the change starting point can be confirmed
     */
    canConfirmChange(): boolean {
      if (this.changingStartingPoint) return false;

      switch (this.newStartingPointType) {
        case 'greenfield':
          return true;
        case 'local':
          return Boolean(this.newLocalPath.trim());
        case 'git':
          return Boolean(
            this.newGitUrl.trim() &&
              this.newGitBranch &&
              !this.newGitError &&
              !this.newGitValidating,
          );
        default:
          return false;
      }
    },

    /**
     * Confirm and execute the starting point change
     */
    async confirmChangeStartingPoint(): Promise<void> {
      if (!this.runName || !this.canConfirmChange()) return;

      this.changingStartingPoint = true;

      try {
        let startingPoint: StartingPoint;
        switch (this.newStartingPointType) {
          case 'greenfield':
            startingPoint = { type: 'greenfield' };
            break;
          case 'local':
            startingPoint = { type: 'localFolder', path: this.newLocalPath };
            break;
          case 'git':
            startingPoint = {
              type: 'gitRepo',
              url: this.newGitUrl,
              branch: this.newGitBranch || undefined,
            };
            break;
        }

        const detail = await changeStartingPoint(this.runName, startingPoint);

        // Update local state with new workspace path
        this.workspacePath = detail.projectPath || null;

        window.toast?.success('Starting point changed successfully');
        this.closeChangeStartingPointDialog();
      } catch (err) {
        const error = err instanceof Error ? err.message : String(err);
        window.toast?.error(`Failed to change starting point: ${error}`);
      } finally {
        this.changingStartingPoint = false;
      }
    },
  };
}
