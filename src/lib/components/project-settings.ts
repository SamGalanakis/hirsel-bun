/**
 * Project Settings component - view and manage project configuration
 */

interface Project {
  id: number;
  name: string;
  createdAt: string;
  updatedAt: string;
  startingPoint: {
    type: 'greenfield' | 'local_folder' | 'git_repo';
    path?: string;
    url?: string;
    branch?: string;
  };
  workerScale?: string;
  timeLimitMinutes?: number;
  maxIterations?: number;
  humanInTheLoop: boolean;
  docsPath: string;
  persistDocsChanges: boolean;
  description?: string;
}

export function projectSettings() {
  return {
    // State
    project: null as Project | null,
    loading: false,
    deleting: false,
    showDeleteConfirm: false,

    async loadProject(projectId: number) {
      if (!projectId) {
        this.project = null;
        return;
      }

      this.loading = true;
      try {
        const project = await window.tauriInvoke<Project>('get_project', { projectId });
        this.project = project;
      } catch (e) {
        console.error('Failed to load project:', e);
        this.project = null;
      } finally {
        this.loading = false;
      }
    },

    getStartingPointDisplay(): string {
      if (!this.project) return '';

      const sp = this.project.startingPoint;
      switch (sp.type) {
        case 'greenfield':
          return 'Greenfield (empty workspace)';
        case 'local_folder':
          return sp.path || 'Local folder';
        case 'git_repo':
          return `${sp.url}${sp.branch ? ` (${sp.branch})` : ''}`;
        default:
          return 'Unknown';
      }
    },

    getStartingPointIcon(): string {
      if (!this.project) return 'folder';

      switch (this.project.startingPoint.type) {
        case 'greenfield':
          return 'file-plus';
        case 'local_folder':
          return 'folder';
        case 'git_repo':
          return 'git-branch';
        default:
          return 'folder';
      }
    },

    formatDate(dateStr: string): string {
      if (!dateStr) return '';
      try {
        const date = new Date(dateStr);
        return date.toLocaleDateString(undefined, {
          year: 'numeric',
          month: 'short',
          day: 'numeric',
          hour: '2-digit',
          minute: '2-digit',
        });
      } catch {
        return dateStr;
      }
    },

    async deleteProject() {
      if (!this.project || this.deleting) return;

      this.deleting = true;
      try {
        await window.tauriInvoke('delete_project', { projectId: this.project.id });
        window.toast?.success('Project deleted');

        // Dispatch event to deselect and refresh projects list
        window.dispatchEvent(new CustomEvent('project-deleted', { detail: this.project.id }));

        this.showDeleteConfirm = false;
        this.project = null;
      } catch (e) {
        console.error('Failed to delete project:', e);
        window.toast?.error(`Failed to delete project: ${e}`);
      } finally {
        this.deleting = false;
      }
    },

    closeSettings() {
      window.dispatchEvent(new CustomEvent('close-project-settings'));
    },
  };
}
