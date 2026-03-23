import { type ParentComponent, createContext, createEffect, createSignal, useContext } from 'solid-js';
import { useProject } from './project-context';

export type MachineryTab = 'work' | 'workers';

interface WorkspaceContextValue {
  machineryOpen: () => boolean;
  setMachineryOpen: (open: boolean) => void;
  activeMachineryTab: () => MachineryTab;
  setActiveMachineryTab: (tab: MachineryTab) => void;
  shepherdMinimized: () => boolean;
  setShepherdMinimized: (minimized: boolean) => void;
}

const WorkspaceContext = createContext<WorkspaceContextValue>();

export const WorkspaceProvider: ParentComponent = (props) => {
  const project = useProject();
  const [machineryOpen, setMachineryOpen] = createSignal(false);
  const [activeMachineryTab, setActiveMachineryTab] = createSignal<MachineryTab>('work');
  const [shepherdMinimized, setShepherdMinimized] = createSignal(false);

  createEffect(() => {
    const projectId = project.selectedProjectId();
    if (!projectId) {
      setMachineryOpen(false);
      setActiveMachineryTab('work');
    }
  });

  const value: WorkspaceContextValue = {
    machineryOpen,
    setMachineryOpen,
    activeMachineryTab,
    setActiveMachineryTab,
    shepherdMinimized,
    setShepherdMinimized,
  };

  return <WorkspaceContext.Provider value={value}>{props.children}</WorkspaceContext.Provider>;
};

export function useWorkspace(): WorkspaceContextValue {
  const context = useContext(WorkspaceContext);
  if (!context) {
    throw new Error('useWorkspace must be used within a WorkspaceProvider');
  }
  return context;
}
