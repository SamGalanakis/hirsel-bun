import { type ParentComponent, createContext, createEffect, createSignal, useContext } from 'solid-js';
import { useProject } from './project-context';

export type MachineryTab = 'board' | 'workers';

interface WorkspaceContextValue {
  machineryOpen: () => boolean;
  setMachineryOpen: (open: boolean) => void;
  activeMachineryTab: () => MachineryTab;
  setActiveMachineryTab: (tab: MachineryTab) => void;
  activeThread: () => string;
  setActiveThread: (thread: string) => void;
  openWorkerDM: (workerName: string) => void;
}

const WorkspaceContext = createContext<WorkspaceContextValue>();

export const WorkspaceProvider: ParentComponent = (props) => {
  const project = useProject();
  const [machineryOpen, setMachineryOpen] = createSignal(false);
  const [activeMachineryTab, setActiveMachineryTab] = createSignal<MachineryTab>('board');
  const [activeThread, setActiveThread] = createSignal('chat');

  createEffect(() => {
    const projectId = project.selectedProjectId();
    if (!projectId) {
      setMachineryOpen(false);
      setActiveMachineryTab('board');
      setActiveThread('chat');
    }
  });

  const openWorkerDM = (workerName: string) => {
    setActiveThread(workerName);
    setActiveMachineryTab('workers');
    setMachineryOpen(true);
  };

  const value: WorkspaceContextValue = {
    machineryOpen,
    setMachineryOpen,
    activeMachineryTab,
    setActiveMachineryTab,
    activeThread,
    setActiveThread,
    openWorkerDM,
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
