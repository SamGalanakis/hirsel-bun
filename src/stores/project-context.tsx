/**
 * Project context for managing projects list and selection
 */
import { invoke } from '@tauri-apps/api/core';
import {
  type ParentComponent,
  createContext,
  createEffect,
  createSignal,
  onCleanup,
  useContext,
} from 'solid-js';

export interface Project {
  id: number;
  name: string;
  startingPoint?: { type: string; path?: string };
  description?: string;
  // Canvas position (for OneBoard portfolio view)
  x?: number | null;
  y?: number | null;
}

interface ProjectContextValue {
  // Projects list
  projects: () => Project[];
  loading: () => boolean;
  loadProjects: () => Promise<void>;

  // Selection
  selectedProject: () => Project | null;
  selectedProjectId: () => number | null;
  selectProject: (project: Project | null) => void;
  deselectProject: () => void;

  // OneBoard focus (which project is zoomed into)
  focusedProjectId: () => number | null;
  setFocusedProjectId: (id: number | null) => void;

  // Project setup/settings
  showProjectSetup: () => boolean;
  setShowProjectSetup: (show: boolean) => void;
  openProjectSetup: () => void;
  cancelProjectSetup: () => void;

  showProjectSettings: () => boolean;
  setShowProjectSettings: (show: boolean) => void;

  // Project view state
  activeProjectView: () => 'board' | 'runs';
  setActiveProjectView: (view: 'board' | 'runs') => void;

  // Project selector dropdown
  projectSelectorOpen: () => boolean;
  setProjectSelectorOpen: (open: boolean) => void;
  projectSearchQuery: () => string;
  setProjectSearchQuery: (query: string) => void;
  filteredProjects: () => Project[];

  // Actions
  removeProject: (projectId: number) => Promise<void>;
  updateProjectPosition: (projectId: number, x: number, y: number) => Promise<void>;
}

const ProjectContext = createContext<ProjectContextValue>();

const STORAGE_KEY = 'hirsel:selectedProjectId';

export const ProjectProvider: ParentComponent = (props) => {
  const [projects, setProjects] = createSignal<Project[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [selectedProject, setSelectedProject] = createSignal<Project | null>(null);
  const [focusedProjectId, setFocusedProjectId] = createSignal<number | null>(null);
  const [showProjectSetup, setShowProjectSetup] = createSignal(false);
  const [showProjectSettings, setShowProjectSettings] = createSignal(false);
  const [activeProjectView, setActiveProjectView] = createSignal<'board' | 'runs'>('board');
  const [projectSelectorOpen, setProjectSelectorOpen] = createSignal(false);
  const [projectSearchQuery, setProjectSearchQuery] = createSignal('');

  const selectedProjectId = () => selectedProject()?.id ?? null;

  const loadProjects = async () => {
    try {
      const result = await invoke<Project[]>('list_projects', {});
      setProjects(result || []);
    } catch (e) {
      console.error('Failed to load projects:', e);
      setProjects([]);
    } finally {
      setLoading(false);
    }
  };

  const restoreProjectSelection = () => {
    const projectList = projects();

    // No projects - let OneBoard show its empty state (user can trigger setup from there)
    if (projectList.length === 0) {
      return;
    }

    // Try to restore saved selection
    const savedId = localStorage.getItem(STORAGE_KEY);
    if (savedId) {
      const projectId = Number.parseInt(savedId, 10);
      const project = projectList.find((p) => p.id === projectId);
      if (project) {
        selectProject(project);
        return;
      }
    }

    // No saved selection - select most recent
    const latest = projectList[0];
    if (latest) {
      selectProject(latest);
    }
  };

  const selectProject = (project: Project | null) => {
    setSelectedProject(project);
    setProjectSelectorOpen(false);
    setProjectSearchQuery('');

    if (project) {
      localStorage.setItem(STORAGE_KEY, String(project.id));
      window.dispatchEvent(new CustomEvent('project-selected', { detail: project.id }));
    } else {
      localStorage.removeItem(STORAGE_KEY);
    }
  };

  const deselectProject = () => {
    setSelectedProject(null);
    setShowProjectSetup(false);
    setShowProjectSettings(false);
    setActiveProjectView('board');
    localStorage.removeItem(STORAGE_KEY);
    window.dispatchEvent(new CustomEvent('project-deselected'));
  };

  const openProjectSetup = () => {
    setShowProjectSetup(true);
    setProjectSelectorOpen(false);
  };

  const cancelProjectSetup = () => {
    setShowProjectSetup(false);
  };

  const removeProject = async (projectId: number) => {
    try {
      await invoke('delete_project', { projectId });
      await loadProjects();
      if (selectedProjectId() === projectId) {
        deselectProject();
      }
    } catch (e) {
      console.error('Failed to remove project:', e);
      window.toast?.error('Failed to remove project');
    }
  };

  const updateProjectPosition = async (projectId: number, x: number, y: number) => {
    try {
      await invoke('update_project', { projectId, x, y });
      // Update local state
      setProjects((prev) =>
        prev.map((p) => (p.id === projectId ? { ...p, x, y } : p))
      );
    } catch (e) {
      console.error('Failed to update project position:', e);
    }
  };

  const filteredProjects = () => {
    const query = projectSearchQuery().toLowerCase();
    if (!query) return projects();
    return projects().filter((p) => p.name.toLowerCase().includes(query));
  };

  // Initialize
  createEffect(() => {
    loadProjects().then(restoreProjectSelection);
  });

  // Listen for project created
  createEffect(() => {
    const handler = async (e: Event) => {
      const customEvent = e as CustomEvent<{ id: number; name: string }>;
      const project = customEvent.detail;
      if (project) {
        await loadProjects();
        selectProject(project);
        setShowProjectSetup(false);
        setActiveProjectView('board');
      }
    };

    window.addEventListener('project-created', handler);
    onCleanup(() => window.removeEventListener('project-created', handler));
  });

  // Listen for cancel project setup
  createEffect(() => {
    const handler = () => {
      setShowProjectSetup(false);
    };
    window.addEventListener('cancel-project-setup', handler);
    onCleanup(() => window.removeEventListener('cancel-project-setup', handler));
  });

  // Listen for project deleted
  createEffect(() => {
    const handler = async () => {
      await loadProjects();
      deselectProject();
      setShowProjectSettings(false);
    };

    window.addEventListener('project-deleted', handler);
    onCleanup(() => window.removeEventListener('project-deleted', handler));
  });

  const value: ProjectContextValue = {
    projects,
    loading,
    loadProjects,
    selectedProject,
    selectedProjectId,
    selectProject,
    deselectProject,
    focusedProjectId,
    setFocusedProjectId,
    showProjectSetup,
    setShowProjectSetup,
    openProjectSetup,
    cancelProjectSetup,
    showProjectSettings,
    setShowProjectSettings,
    activeProjectView,
    setActiveProjectView,
    projectSelectorOpen,
    setProjectSelectorOpen,
    projectSearchQuery,
    setProjectSearchQuery,
    filteredProjects,
    removeProject,
    updateProjectPosition,
  };

  return <ProjectContext.Provider value={value}>{props.children}</ProjectContext.Provider>;
};

export function useProject(): ProjectContextValue {
  const context = useContext(ProjectContext);
  if (!context) {
    throw new Error('useProject must be used within a ProjectProvider');
  }
  return context;
}
