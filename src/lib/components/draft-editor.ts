/**
 * Draft Editor Alpine.js Component
 *
 * A dedicated editor view for configuring draft runs before starting them.
 * Provides controls for spec, worker scale, time limit, HITL mode, and project path.
 */

import { createDraft, updateDraft, startDraft, deleteRun, getRunDetail, readSpecFile, writeSpecFile, readEvalFile, writeEvalFile } from '../api';
import type { RunDetail, DraftUpdateRequest } from '../types';
import { marked } from 'marked';

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
}

/**
 * Parse time limit string to minutes
 */
function parseTimeLimit(input: string): number | null {
  if (!input.trim()) return null;
  const s = input.trim().toLowerCase();

  // Check for combined format like "1h30m"
  if (s.includes('h') && s.includes('m')) {
    const hMatch = s.match(/(\d+)h/);
    const mMatch = s.match(/(\d+)m/);
    const hours = hMatch ? parseInt(hMatch[1], 10) : 0;
    const mins = mMatch ? parseInt(mMatch[1], 10) : 0;
    return hours * 60 + mins;
  }

  // Hours format
  if (s.endsWith('h')) {
    const num = parseFloat(s.slice(0, -1));
    return isNaN(num) ? null : Math.round(num * 60);
  }

  // Minutes format
  if (s.endsWith('m')) {
    const num = parseFloat(s.slice(0, -1));
    return isNaN(num) ? null : Math.round(num);
  }

  // Plain number - assume minutes
  const num = parseFloat(s);
  return isNaN(num) ? null : Math.round(num);
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
          window.toast?.info('Files updated by Gyp', 'Draft refreshed');
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
        window.toast?.error('Failed to save spec', 'Error');
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
        window.toast?.error('Failed to save eval', 'Error');
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
     * Save draft config (not spec/eval - those are file-based)
     */
    async saveDraft(): Promise<void> {
      if (!this.runName || this.saving) return;

      this.saving = true;

      try {
        const updates: DraftUpdateRequest = {
          // Note: spec is NOT saved here - it's file-based via saveSpec()
          workerScale: this.workerScale,
          timeLimitMinutes: parseTimeLimit(this.timeLimitInput) ?? undefined,
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
        this.timeLimitMinutes = parseTimeLimit(this.timeLimitInput);
        this.saving = false;
      } catch (err) {
        this.error = err instanceof Error ? err.message : String(err);
        this.saving = false;
        window.toast?.error(this.error, 'Failed to save draft');
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

      // Validate required fields
      if (!this.projectPath.trim()) {
        window.toast?.error('Project path is required to start a run', 'Missing project');
        return;
      }

      this.starting = true;
      this.error = null;

      try {
        // Save any pending changes first
        await this.saveDraft();

        // Start the draft
        const detail = await startDraft(this.runName);

        window.toast?.success(`Run "${detail.name}" started with workers`, 'Run started');

        // Dispatch event to notify that run has started (triggers view switch)
        window.dispatchEvent(new CustomEvent('run-started', { detail: detail.name }));
        window.dispatchEvent(new CustomEvent('run-selected', { detail: detail.name }));

        this.starting = false;
      } catch (err) {
        this.error = err instanceof Error ? err.message : String(err);
        this.starting = false;
        window.toast?.error(this.error, 'Failed to start run');
      }
    },

    /**
     * Delete the draft
     */
    async deleteDraft(): Promise<void> {
      if (!this.runName) return;

      if (!confirm(`Delete draft "${this.name}"? This cannot be undone.`)) {
        return;
      }

      try {
        await deleteRun(this.runName);
        window.toast?.info(`Draft "${this.name}" deleted`, 'Draft deleted');

        // Clear selection
        window.dispatchEvent(new CustomEvent('run-selected', { detail: null }));
        this.clearDraft();
      } catch (err) {
        const error = err instanceof Error ? err.message : String(err);
        window.toast?.error(error, 'Failed to delete draft');
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
        window.toast?.error(error, 'Failed to rename draft');
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
     * Render markdown content to HTML
     */
    renderMarkdown(content: string): string {
      if (!content) return '<p class="text-wool-500 italic">No content</p>';
      return marked(content) as string;
    },
  };
}
