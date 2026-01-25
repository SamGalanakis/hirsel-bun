/**
 * Project Setup component - form for creating new projects
 */

import { debounce } from '../utils/debounce';

export function projectSetup() {
  return {
    // Form state
    name: '',
    startingPointType: 'greenfield' as 'greenfield' | 'local' | 'git',
    localPath: '',
    gitUrl: '',
    gitBranch: '',
    availableBranches: [] as string[],

    // UI state
    creating: false,
    gitValidating: false,
    gitError: null as string | null,
    showPathSuggestions: false,
    pathSuggestions: [] as string[],
    selectedSuggestionIndex: -1,

    // Debounced functions
    debouncedFetchPathSuggestions: null as (() => void) | null,
    debouncedValidateGitUrl: null as (() => void) | null,

    init() {
      this.debouncedFetchPathSuggestions = debounce(() => this.fetchPathSuggestions(), 200);
      this.debouncedValidateGitUrl = debounce(() => this.validateGitUrl(), 500);
    },

    get startingPointValid(): boolean {
      switch (this.startingPointType) {
        case 'greenfield':
          return true;
        case 'local':
          return this.localPath.trim().length > 0;
        case 'git':
          return this.gitUrl.trim().length > 0 && !this.gitError && this.gitBranch !== '';
        default:
          return false;
      }
    },

    canCreate(): boolean {
      return this.name.trim().length > 0 && this.startingPointValid;
    },

    async createProject() {
      if (!this.canCreate() || this.creating) return;

      this.creating = true;

      try {
        // Build starting point
        let startingPoint: { type: string; path?: string; url?: string; branch?: string };
        switch (this.startingPointType) {
          case 'greenfield':
            startingPoint = { type: 'greenfield' };
            break;
          case 'local':
            startingPoint = { type: 'local_folder', path: this.localPath };
            break;
          case 'git':
            startingPoint = { type: 'git_repo', url: this.gitUrl, branch: this.gitBranch };
            break;
        }

        const project = await window.tauriInvoke<{ id: number; name: string }>('create_project', {
          name: this.name,
          startingPoint,
        });

        // Dispatch event to select the new project
        window.dispatchEvent(
          new CustomEvent('project-created', {
            detail: project,
          }),
        );

        // Reset form
        this.resetForm();
      } catch (e) {
        console.error('Failed to create project:', e);
        window.toast?.error(`Failed to create project: ${e}`);
      } finally {
        this.creating = false;
      }
    },

    cancelSetup() {
      this.resetForm();
      window.dispatchEvent(new CustomEvent('cancel-project-setup'));
    },

    resetForm() {
      this.name = '';
      this.startingPointType = 'greenfield';
      this.localPath = '';
      this.gitUrl = '';
      this.gitBranch = '';
      this.availableBranches = [];
      this.gitError = null;
    },

    // Path suggestions for local folder
    async fetchPathSuggestions() {
      if (!this.localPath || this.startingPointType !== 'local') {
        this.pathSuggestions = [];
        return;
      }

      try {
        const suggestions = await window.tauriInvoke<string[]>('suggest_paths', {
          partial: this.localPath,
        });
        this.pathSuggestions = suggestions || [];
        this.showPathSuggestions = this.pathSuggestions.length > 0;
        this.selectedSuggestionIndex = -1;
      } catch (e) {
        console.error('Failed to fetch path suggestions:', e);
        this.pathSuggestions = [];
      }
    },

    selectPathSuggestion(suggestion: string) {
      this.localPath = suggestion;
      this.showPathSuggestions = false;
      this.pathSuggestions = [];
    },

    hidePathSuggestions() {
      setTimeout(() => {
        this.showPathSuggestions = false;
      }, 150);
    },

    handlePathKeydown(e: KeyboardEvent) {
      if (!this.showPathSuggestions || this.pathSuggestions.length === 0) return;

      if (e.key === 'ArrowDown') {
        e.preventDefault();
        this.selectedSuggestionIndex = Math.min(
          this.selectedSuggestionIndex + 1,
          this.pathSuggestions.length - 1,
        );
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        this.selectedSuggestionIndex = Math.max(this.selectedSuggestionIndex - 1, 0);
      } else if (e.key === 'Enter' && this.selectedSuggestionIndex >= 0) {
        e.preventDefault();
        this.selectPathSuggestion(this.pathSuggestions[this.selectedSuggestionIndex]);
      } else if (e.key === 'Escape') {
        this.showPathSuggestions = false;
      }
    },

    async browseLocalFolder() {
      try {
        const result = await window.tauriInvoke<string | null>('pick_folder', {});
        if (result) {
          this.localPath = result;
        }
      } catch (e) {
        console.error('Failed to pick folder:', e);
      }
    },

    // Git URL validation
    async validateGitUrl() {
      if (!this.gitUrl || this.startingPointType !== 'git') {
        this.availableBranches = [];
        this.gitError = null;
        return;
      }

      this.gitValidating = true;
      this.gitError = null;

      try {
        const result = await window.tauriInvoke<{
          valid: boolean;
          branches?: string[];
          error?: string;
        }>('validate_repo', { url: this.gitUrl });

        if (result.valid && result.branches) {
          this.availableBranches = result.branches;
          // Auto-select default branch
          if (result.branches.includes('main')) {
            this.gitBranch = 'main';
          } else if (result.branches.includes('master')) {
            this.gitBranch = 'master';
          } else if (result.branches.length > 0) {
            this.gitBranch = result.branches[0];
          }
        } else {
          this.gitError = result.error || 'Invalid repository';
          this.availableBranches = [];
        }
      } catch (e) {
        this.gitError = String(e);
        this.availableBranches = [];
      } finally {
        this.gitValidating = false;
      }
    },
  };
}
