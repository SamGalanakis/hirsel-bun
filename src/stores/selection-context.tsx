/**
 * Selection context for managing UI selection state
 */
import { invoke } from '../lib/invoke';
import { emit, on } from '../lib/events';
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

interface AttachPickerState {
  open: boolean;
  workers: Array<{ name: string; status: string }>;
  evals: Array<{ id: number; evalName: string; status: string }>;
  loading: boolean;
}

interface SelectionContextValue {
  attachPicker: () => AttachPickerState;
  closeAttachPicker: () => void;
  attachToTarget: (type: 'worker' | 'eval', name: string) => void;
}

const SelectionContext = createContext<SelectionContextValue>();

export const SelectionProvider: ParentComponent = (props) => {
  const runsContext = useRuns();

  const [focusedRunIndex, setFocusedRunIndex] = createSignal(-1);
  const [attachPicker, setAttachPicker] = createSignal<AttachPickerState>({
    open: false,
    workers: [],
    evals: [],
    loading: false,
  });

  createEffect(() => {
    const _ = runsContext.selectedRun();
    setFocusedRunIndex(-1);
    closeAttachPicker();
  });

  createEffect(() => {
    const cleanup = on('shortcut-action', (action) => {
      switch (action as ShortcutAction) {
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
      }
    });

    onCleanup(cleanup);
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

    emit('show-worker-output', { runName, workerName: name });
    closeAttachPicker();
  };

  const handleAttach = async () => {
    await openAttachPicker();
  };

  const value: SelectionContextValue = {
    attachPicker,
    closeAttachPicker,
    attachToTarget,
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
