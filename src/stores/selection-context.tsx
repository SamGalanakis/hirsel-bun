/**
 * Selection context for managing UI selection state
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
import type { ShortcutAction } from '../lib/shortcuts';
import { useRuns } from './runs-context';

type TabId = 'overview' | 'work' | 'chat' | 'config';

interface AttachPickerState {
  open: boolean;
  workers: Array<{ name: string; status: string }>;
  evals: Array<{ id: number; evalName: string; status: string }>;
  loading: boolean;
}

interface SelectionContextValue {
  // Tab selection
  activeTab: () => TabId;
  setActiveTab: (tab: TabId) => void;

  // Worker selection (in overview/worker panel)
  selectedWorkerId: () => number | null;
  setSelectedWorkerId: (id: number | null) => void;
  highlightedWorker: () => string | null;
  setHighlightedWorker: (name: string | null) => void;

  // Task selection
  selectedTaskId: () => string | null;
  setSelectedTaskId: (id: string | null) => void;

  // Thread selection (in chat tab)
  selectedThread: () => string | null;
  setSelectedThread: (name: string | null) => void;

  // Eval selection
  selectedEvalId: () => number | null;
  setSelectedEvalId: (id: number | null) => void;

  // Run list navigation
  focusedRunIndex: () => number;
  setFocusedRunIndex: (index: number) => void;
  navigateRuns: (direction: number) => void;
  selectFocusedRun: () => void;

  // Attach picker
  attachPicker: () => AttachPickerState;
  openAttachPicker: () => Promise<void>;
  closeAttachPicker: () => void;
  attachToTarget: (type: 'worker' | 'eval', name: string) => void;

  // Run actions
  pauseRun: () => Promise<void>;
  resumeRun: () => Promise<void>;
  handleAttach: () => Promise<void>;
}

const SelectionContext = createContext<SelectionContextValue>();

export const SelectionProvider: ParentComponent = (props) => {
  const runsContext = useRuns();

  // Tab state
  const [activeTab, setActiveTab] = createSignal<TabId>('overview');

  // Selection state
  const [selectedWorkerId, setSelectedWorkerId] = createSignal<number | null>(null);
  const [highlightedWorker, setHighlightedWorker] = createSignal<string | null>(null);
  const [selectedTaskId, setSelectedTaskId] = createSignal<string | null>(null);
  const [selectedThread, setSelectedThread] = createSignal<string | null>(null);
  const [selectedEvalId, setSelectedEvalId] = createSignal<number | null>(null);

  // Run list navigation
  const [focusedRunIndex, setFocusedRunIndex] = createSignal(-1);

  // Attach picker state
  const [attachPicker, setAttachPicker] = createSignal<AttachPickerState>({
    open: false,
    workers: [],
    evals: [],
    loading: false,
  });

  // Clear selection when run changes
  createEffect(() => {
    const _ = runsContext.selectedRun();
    setActiveTab('overview');
    setSelectedWorkerId(null);
    setHighlightedWorker(null);
    setSelectedTaskId(null);
    setSelectedThread(null);
    setSelectedEvalId(null);
  });

  // Listen for tab switch events
  createEffect(() => {
    const handler = (e: Event) => {
      const customEvent = e as CustomEvent<string>;
      if (customEvent.detail) {
        setActiveTab(customEvent.detail as TabId);
      }
    };

    window.addEventListener('switch-tab', handler);
    onCleanup(() => window.removeEventListener('switch-tab', handler));
  });

  // Listen for shortcut actions
  createEffect(() => {
    const handler = (e: Event) => {
      const customEvent = e as CustomEvent<ShortcutAction>;
      const action = customEvent.detail;

      switch (action) {
        case 'navigate-up':
          navigateRuns(-1);
          break;
        case 'navigate-down':
          navigateRuns(1);
          break;
        case 'select-run':
          selectFocusedRun();
          break;
        case 'attach':
          if (runsContext.selectedRun() && !attachPicker().open) {
            handleAttach();
          }
          break;
        case 'pause':
          if (runsContext.selectedRun()) pauseRun();
          break;
        case 'resume':
          if (runsContext.selectedRun()) resumeRun();
          break;
        case 'switch-chat':
          if (runsContext.selectedRun()) setActiveTab('chat');
          break;
        case 'focus-message':
          if (runsContext.selectedRun()) {
            setActiveTab('chat');
            setTimeout(() => {
              const input = document.querySelector('.inline-chat-input') as HTMLElement;
              if (input) input.focus();
            }, 100);
          }
          break;
      }
    };

    window.addEventListener('shortcut-action', handler);
    onCleanup(() => window.removeEventListener('shortcut-action', handler));
  });

  const navigateRuns = (direction: number) => {
    const runItems = document.querySelectorAll('.run-item');
    if (runItems.length === 0) return;

    let newIndex = focusedRunIndex() + direction;
    if (newIndex < 0) newIndex = 0;
    if (newIndex >= runItems.length) newIndex = runItems.length - 1;
    setFocusedRunIndex(newIndex);

    // Update visual focus
    runItems.forEach((item, idx) => {
      item.classList.toggle('ring-2', idx === newIndex);
      item.classList.toggle('ring-amber-500', idx === newIndex);
    });

    // Scroll into view
    runItems[newIndex]?.scrollIntoView({ block: 'nearest' });
  };

  const selectFocusedRun = () => {
    const runItems = document.querySelectorAll('.run-item');
    const index = focusedRunIndex();
    if (index >= 0 && index < runItems.length) {
      (runItems[index] as HTMLElement).click();
    }
  };

  const pauseRun = async () => {
    const runName = runsContext.selectedRun();
    if (!runName) return;
    try {
      await invoke('pause_run', { runName });
      await runsContext.invalidateRuns();
    } catch (e) {
      console.error('Failed to pause run:', e);
    }
  };

  const resumeRun = async () => {
    const runName = runsContext.selectedRun();
    if (!runName) return;
    try {
      await invoke('resume_run', { runName });
      await runsContext.invalidateRuns();
    } catch (e) {
      console.error('Failed to resume run:', e);
    }
  };

  const openAttachPicker = async () => {
    const runName = runsContext.selectedRun();
    if (!runName) return;

    try {
      setAttachPicker((s) => ({ ...s, loading: true }));

      const [workers, evals] = await Promise.all([
        invoke<Array<{ name: string; status: string }>>('get_workers', { runName }),
        invoke<Array<{ id: number; evalName: string; status: string }>>('get_evals', { runName }),
      ]);

      const workerList = workers || [];
      const evalList = evals || [];
      const totalChoices = workerList.length + evalList.length;

      // If only one choice, attach directly
      if (totalChoices === 1) {
        if (workerList.length === 1) {
          attachToTarget('worker', workerList[0].name);
        } else if (evalList.length === 1) {
          attachToTarget('eval', evalList[0].evalName);
        }
        return;
      }

      setAttachPicker({
        open: true,
        workers: workerList,
        evals: evalList,
        loading: false,
      });
    } catch (e) {
      console.error('Failed to load attach picker data:', e);
      setAttachPicker({ open: false, workers: [], evals: [], loading: false });
    }
  };

  const closeAttachPicker = () => {
    setAttachPicker({ open: false, workers: [], evals: [], loading: false });
  };

  const attachToTarget = (type: 'worker' | 'eval', name: string) => {
    const runName = runsContext.selectedRun();
    if (!runName) return;

    window.dispatchEvent(
      new CustomEvent('show-worker-output', {
        detail: { runName, workerName: name },
      }),
    );
    closeAttachPicker();
  };

  const handleAttach = async () => {
    const runName = runsContext.selectedRun();
    if (!runName) return;

    // Check if there's a highlighted worker
    const worker = highlightedWorker();
    if (worker) {
      window.dispatchEvent(
        new CustomEvent('show-worker-output', {
          detail: { runName, workerName: worker },
        }),
      );
      return;
    }

    // Otherwise open the attach picker
    await openAttachPicker();
  };

  const value: SelectionContextValue = {
    activeTab,
    setActiveTab,
    selectedWorkerId,
    setSelectedWorkerId,
    highlightedWorker,
    setHighlightedWorker,
    selectedTaskId,
    setSelectedTaskId,
    selectedThread,
    setSelectedThread,
    selectedEvalId,
    setSelectedEvalId,
    focusedRunIndex,
    setFocusedRunIndex,
    navigateRuns,
    selectFocusedRun,
    attachPicker,
    openAttachPicker,
    closeAttachPicker,
    attachToTarget,
    pauseRun,
    resumeRun,
    handleAttach,
  };

  return <SelectionContext.Provider value={value}>{props.children}</SelectionContext.Provider>;
};

export function useSelection(): SelectionContextValue {
  const context = useContext(SelectionContext);
  if (!context) {
    throw new Error('useSelection must be used within a SelectionProvider');
  }
  return context;
}
