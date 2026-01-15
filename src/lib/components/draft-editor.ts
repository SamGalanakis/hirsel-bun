/**
 * Draft Editor Alpine.js Component
 *
 * A dedicated editor view for configuring draft runs before starting them.
 * Provides controls for spec, worker scale, time limit, HITL mode, and project path.
 */

import { createDraft, updateDraft, startDraft, deleteRun, getRunDetail, readSpecFile, writeSpecFile, readEvalFile, writeEvalFile, validateRepo, initProjectRepo } from '../api';
import type { RunDetail, DraftUpdateRequest, RepoValidation } from '../types';
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
  activeTab: 'spec' | 'eval';
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

    /**
     * Initialize the component
     */
    init(): void {
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

        // Load spec and eval from files (file-first editing)
        const [specContent, evalContent] = await Promise.all([
          readSpecFile(name),
          readEvalFile(name),
        ]);
        this.spec = specContent;
        this.eval = evalContent;

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

      const confirmed = await (window as any).confirmDialog?.delete(this.name, 'draft')
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
      if (!this.runName || !newName.trim() || newName === this.name) {
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
     */
    renderMarkdown(content: string): string {
      if (!content) return '<p class="text-wool-500 italic">No content</p>';
      return DOMPurify.sanitize(marked(content) as string);
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
        const result = await validateRepo(path);

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
  };
}
