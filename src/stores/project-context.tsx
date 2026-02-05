/**
 * Project context for managing projects list and selection
 */
import { invoke } from '@tauri-apps/api/core';
import {
  type ParentComponent,
  batch,
  createContext,
  createEffect,
  createSignal,
  onCleanup,
  useContext,
} from 'solid-js';

export interface Project {
  id: number;
  name: string;
  startingPoint?: { type: string; path?: string; url?: string; branch?: string };
  description?: string;
  // Run configuration (None = use global defaults)
  workerScale?: string | null;
  timeLimitMinutes?: number | null;
  humanInTheLoop?: boolean;
  docsPath?: string;
  persistDocsChanges?: boolean;
  // Delivery configuration
  targetBranch?: string | null;
  // Runner configuration
  runner?: string | null;
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
  openProjectSetupAt: (x: number, y: number) => void;
  pendingProjectPosition: () => { x: number; y: number } | null;
  setPendingProjectPosition: (pos: { x: number; y: number } | null) => void;
  cancelProjectSetup: () => void;

  showProjectSettings: () => boolean;
  setShowProjectSettings: (show: boolean) => void;

  // Project view state
  activeProjectView: () => 'board' | 'runs';
  setActiveProjectView: (view: 'board' | 'runs') => void;

  // Docs panel state
  docsOpen: () => boolean;
  setDocsOpen: (open: boolean) => void;
  selectedDocFile: () => string | null;
  setSelectedDocFile: (file: string | null) => void;
  docsFullScreen: () => boolean;
  setDocsFullScreen: (fullScreen: boolean) => void;

  // Sheepfold (project messaging) state
  sheepfoldOpen: () => boolean;
  setSheepfoldOpen: (open: boolean) => void;
  activeThread: () => string; // 'chat' or worker name
  setActiveThread: (thread: string) => void;
  projectUnreadCount: () => number;
  openWorkerDM: (workerName: string) => void; // Opens drawer + selects thread

  // Project selector dropdown
  projectSelectorOpen: () => boolean;
  setProjectSelectorOpen: (open: boolean) => void;
  projectSearchQuery: () => string;
  setProjectSearchQuery: (query: string) => void;
  filteredProjects: () => Project[];

  // Actions
  removeProject: (projectId: number) => Promise<void>;
  updateProjectPosition: (projectId: number, routeId: number, x: number, y: number) => Promise<void>;
  updateProjectSettings: (
    projectId: number,
    routeId: number,
    settings: Partial<Project>
  ) => Promise<Project | null>;
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
  const [pendingProjectPosition, setPendingProjectPosition] = createSignal<{
    x: number;
    y: number;
  } | null>(null);
  const [activeProjectView, setActiveProjectView] = createSignal<'board' | 'runs'>('board');
  const [projectSelectorOpen, setProjectSelectorOpen] = createSignal(false);
  const [projectSearchQuery, setProjectSearchQuery] = createSignal('');
  const [docsOpen, setDocsOpen] = createSignal(false);
  const [selectedDocFile, setSelectedDocFile] = createSignal<string | null>(null);
  const [docsFullScreen, setDocsFullScreen] = createSignal(false);

  // Sheepfold state
  const [sheepfoldOpen, setSheepfoldOpen] = createSignal(false);
  const [activeThread, setActiveThread] = createSignal('chat');
  const [projectUnreadCount, setProjectUnreadCount] = createSignal(0);

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
    setPendingProjectPosition(null);
    setShowProjectSetup(true);
    setProjectSelectorOpen(false);
  };

  const openProjectSetupAt = (x: number, y: number) => {
    setPendingProjectPosition({ x, y });
    setShowProjectSetup(true);
    setProjectSelectorOpen(false);
  };

  const cancelProjectSetup = () => {
    setShowProjectSetup(false);
    setPendingProjectPosition(null);
  };

  const removeProject = async (projectId: number) => {
    // Delete the project - this is the critical operation
    await invoke('delete_project', { projectId });

    // Cleanup: reload projects and handle selection
    // These shouldn't fail, but don't let cleanup errors mask successful delete
    try {
      await loadProjects();
      if (selectedProjectId() === projectId) {
        // Auto-select another project if any remain
        const remaining = projects();
        if (remaining.length > 0) {
          selectProject(remaining[0]);
        } else {
          deselectProject();
        }
      }
    } catch (e) {
      console.error('Project deleted but cleanup failed:', e);
    }
  };

  const updateProjectPosition = async (
    projectId: number,
    routeId: number,
    x: number,
    y: number
  ) => {
    try {
      await invoke('update_project', { projectId, routeId, x, y });
      // Update local state
      setProjects((prev) => prev.map((p) => (p.id === projectId ? { ...p, x, y } : p)));
    } catch (e) {
      console.error('Failed to update project position:', e);
    }
  };

  const updateProjectSettings = async (
    projectId: number,
    routeId: number,
    settings: Partial<Project>
  ): Promise<Project | null> => {
    try {
      const updated = await invoke<Project>('update_project', {
        projectId,
        routeId,
        x: settings.x,
        y: settings.y,
        description: settings.description,
        targetBranch: settings.targetBranch,
        workerScale: settings.workerScale,
        timeLimitMinutes: settings.timeLimitMinutes,
        humanInTheLoop: settings.humanInTheLoop,
        runner: settings.runner,
      });
      // Update local state - batch to prevent intermediate reactive states
      batch(() => {
        setProjects((prev) =>
          prev.map((p) => (p.id === projectId ? { ...p, ...updated } : p))
        );
        // Update selected project if it's the one being edited
        const current = selectedProject();
        if (selectedProjectId() === projectId && current) {
          setSelectedProject({ ...current, ...updated });
        }
      });
      return updated;
    } catch (e) {
      console.error('Failed to update project settings:', e);
      window.toast?.error(`Failed to update settings: ${e}`);
      return null;
    }
  };

  const filteredProjects = () => {
    const query = projectSearchQuery().toLowerCase();
    if (!query) return projects();
    return projects().filter((p) => p.name.toLowerCase().includes(query));
  };

  const openWorkerDM = (workerName: string) => {
    setActiveThread(workerName);
    setSheepfoldOpen(true);
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

  // Poll for unread count when project is selected
  createEffect(() => {
    const projectId = selectedProjectId();
    if (!projectId) {
      setProjectUnreadCount(0);
      return;
    }

    const fetchUnread = async () => {
      try {
        const count = await invoke<number>('get_project_unread_count', { projectId });
        setProjectUnreadCount(count);
      } catch (e) {
        console.error('Failed to fetch unread count:', e);
      }
    };

    // Initial fetch
    fetchUnread();

    // Poll every 5 seconds
    const interval = setInterval(fetchUnread, 5000);
    onCleanup(() => clearInterval(interval));
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
    openProjectSetupAt,
    pendingProjectPosition,
    setPendingProjectPosition,
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
    updateProjectSettings,
    docsOpen,
    setDocsOpen,
    selectedDocFile,
    setSelectedDocFile,
    docsFullScreen,
    setDocsFullScreen,
    sheepfoldOpen,
    setSheepfoldOpen,
    activeThread,
    setActiveThread,
    projectUnreadCount,
    openWorkerDM,
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
