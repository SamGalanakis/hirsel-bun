/**
 * CanvasToolbar - Toolbar bar above the canvas
 *
 * Contains:
 * - Sidebar collapse toggle (left)
 * - Run status + workers (center)
 * - Live tree filters: depth + show added/deleted (right)
 */
import { type Component, Show, For, createSignal, createEffect, onCleanup } from 'solid-js';
import { invoke } from '@tauri-apps/api/core';
import { useApp, useProject } from '../../stores';
import { useDelta } from '../../stores/delta-context';
import { Icon, SheepAvatar } from '../shared';
import { RunStatusPill } from '../specflow/RunStatusPill';
import { WorkerDetailModal } from '../runs/WorkerDetailModal';
import type { WorkerDisplay } from '../../lib/types';

interface CanvasToolbarProps {
  // Live tree filters
  granularity: () => 'all' | '2' | '1';
  setGranularity: (g: 'all' | '2' | '1') => void;
  liveFilters: () => string[];
  setLiveFilters: (filters: string[]) => void;
  // Whether there's a live tree
  hasLiveTree: boolean;
}

export const CanvasToolbar: Component<CanvasToolbarProps> = (props) => {
  const app = useApp();
  const project = useProject();
  const delta = useDelta();

  // Workers state
  const [workers, setWorkers] = createSignal<WorkerDisplay[]>([]);
  const [runValid, setRunValid] = createSignal(true);
  const [selectedWorker, setSelectedWorker] = createSignal<WorkerDisplay | null>(null);
  const [hoveredWorker, setHoveredWorker] = createSignal<WorkerDisplay | null>(null);

  // Fetch workers when project run changes
  createEffect(() => {
    const run = delta.projectRun();
    if (!run) {
      setWorkers([]);
      setRunValid(true);
      return;
    }

    setRunValid(true);

    const fetchWorkers = async () => {
      try {
        const result = await invoke<WorkerDisplay[]>('get_workers', { runName: run.runName });
        setWorkers(result);
        setRunValid(true);
      } catch (e) {
        const errorStr = String(e);
        if (errorStr.includes('not found') || errorStr.includes('Not found')) {
          setRunValid(false);
          setWorkers([]);
          return;
        }
      }
    };

    fetchWorkers();
    const interval = setInterval(fetchWorkers, 2000);
    onCleanup(() => clearInterval(interval));
  });

  // Compute effective run status from live tree
  const liveRunStatus = () => {
    if (!runValid()) return null;

    const liveTrees = delta.liveTree();
    const run = delta.projectRun();

    if (!run) return null;
    if (!liveTrees || liveTrees.length === 0) return null;

    type LiveNode = (typeof liveTrees)[0];
    const flattenLive = (node: LiveNode): LiveNode[] => {
      return [node, ...(node.children?.flatMap(flattenLive) || [])];
    };
    const liveNodes = liveTrees.flatMap(flattenLive);

    if (liveNodes.length === 0) return null;

    const hasWorkingNode = liveNodes.some(n => n.status === 'working');

    if (run.status === 'failed') return 'failed';
    if (run.status === 'paused') return 'paused';
    if (hasWorkingNode) return 'working';

    const allComplete = liveNodes.every(n => n.status === 'done' || n.status === 'validated' || n.status === 'awaiting_eval');
    if (allComplete) return 'done';

    const allPending = liveNodes.every(n => n.status === 'pending');
    if (run.status === 'working' && allPending) return 'starting';

    if (run.status === 'working' || run.status === 'paused' || run.status === 'failed') {
      return run.status;
    }

    return null;
  };

  const handlePauseRun = async () => {
    const run = delta.projectRun();
    if (!run) return;
    try {
      await invoke('pause_run', { runName: run.runName });
      window.toast?.success('Run paused');
    } catch (e) {
      window.toast?.error(`Failed to pause: ${e}`);
    }
  };

  const handleResumeRun = async () => {
    const run = delta.projectRun();
    if (!run) return;
    try {
      await invoke('resume_run', { runName: run.runName });
      window.toast?.success('Run resumed');
    } catch (e) {
      window.toast?.error(`Failed to resume: ${e}`);
    }
  };

  const handleAttachWorker = () => {
    const worker = selectedWorker();
    const run = delta.projectRun();
    if (!worker || !run) return;

    window.dispatchEvent(new CustomEvent('show-worker-output', {
      detail: { runName: run.runName, workerName: worker.name }
    }));
    setSelectedWorker(null);
  };

  const handleOpenWorkerDM = () => {
    const worker = selectedWorker();
    if (!worker) return;
    project.openWorkerDM(worker.name);
    setSelectedWorker(null);
  };

  // Keyboard shortcuts for quick worker selection (1-5)
  createEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') return;

      const num = parseInt(e.key);
      if (num >= 1 && num <= 5) {
        const workerList = workers();
        if (num <= workerList.length) {
          setSelectedWorker(workerList[num - 1]);
        }
      }
    };

    document.addEventListener('keydown', handler);
    onCleanup(() => document.removeEventListener('keydown', handler));
  });

  return (
    <>
      <div
        class="flex items-center gap-2 px-2 py-1 border-b border-pasture-600/40"
        style={{ background: 'rgba(30, 30, 30, 0.6)' }}
      >
        {/* Left: Sidebar toggle */}
        <div class="flex items-center gap-2">
          {/* Sidebar collapse toggle */}
          <button
            onClick={() => app.toggleSidebar()}
            class="p-1.5 rounded hover:bg-pasture-700 transition-colors"
            title={app.sidebarCollapsed() ? 'Expand sidebar' : 'Collapse sidebar'}
          >
            <Icon
              name={app.sidebarCollapsed() ? 'panel-left-open' : 'panel-left-close'}
              class="w-4 h-4 text-wool-500"
            />
          </button>

        </div>

        {/* Center: Status + Workers */}
        <div class="flex-1 flex items-center justify-center gap-3">
          {/* Run status pill */}
          <Show when={runValid() && delta.projectRun() && liveRunStatus()}>
            <RunStatusPill
              status={liveRunStatus()}
              onPause={handlePauseRun}
              onResume={handleResumeRun}
            />
          </Show>

          {/* Worker avatars */}
          <Show when={runValid() && workers().length > 0}>
            <div class="flex items-center gap-1">
              <For each={workers().slice(0, 5)}>
                {(worker, index) => {
                  const isWorking = () => worker.status === 'working';
                  const isError = () => worker.status === 'error';
                  const isHovered = () => hoveredWorker()?.name === worker.name;
                  return (
                    <div class="relative">
                      <button
                        type="button"
                        class="relative rounded-full transition-transform hover:scale-110 focus:outline-none focus:ring-2 focus:ring-amber-500/50"
                        style={{
                          'box-shadow': isWorking()
                            ? '0 0 8px rgba(212, 165, 116, 0.4)'
                            : isError()
                            ? '0 0 8px rgba(196, 92, 74, 0.4)'
                            : undefined,
                        }}
                        onClick={() => setSelectedWorker(worker)}
                        onMouseEnter={() => setHoveredWorker(worker)}
                        onMouseLeave={() => setHoveredWorker(null)}
                      >
                        <SheepAvatar
                          config={worker.sheepConfig}
                          size={24}
                          status={worker.status}
                        />
                      </button>
                      {/* Hover tooltip */}
                      <Show when={isHovered()}>
                        <div
                          class="absolute z-50 top-full mt-2 left-1/2 -translate-x-1/2 px-2.5 py-1.5 rounded-md whitespace-nowrap pointer-events-none"
                          style={{
                            background: 'rgba(30, 30, 30, 0.95)',
                            border: '1px solid rgba(64, 64, 64, 0.6)',
                            'box-shadow': '0 4px 12px rgba(0,0,0,0.4)',
                          }}
                        >
                          <div class="text-[11px] font-medium text-wool-200">{worker.name}</div>
                          <div class="text-[9px] text-wool-500 mt-0.5">
                            {worker.currentTask ? `Working: ${worker.currentTask.slice(0, 30)}...` : worker.status}
                          </div>
                          <div class="text-[9px] text-wool-600 mt-1 flex items-center gap-1.5">
                            <span class="px-1 py-0.5 rounded bg-pasture-700 text-wool-400">Click</span>
                            <span>details</span>
                            <span class="mx-0.5">·</span>
                            <span class="px-1 py-0.5 rounded bg-pasture-700 text-wool-400">{index() + 1}</span>
                            <span>select</span>
                          </div>
                        </div>
                      </Show>
                    </div>
                  );
                }}
              </For>
              <Show when={workers().length > 5}>
                <span class="text-[10px] text-wool-500 ml-1">
                  +{workers().length - 5}
                </span>
              </Show>
            </div>
          </Show>
        </div>

        {/* Right: Live tree view controls - only show if has live tree */}
        <div class="flex items-center gap-3">
          <Show when={props.hasLiveTree}>
            {/* Depth selector */}
            <div class="flex items-center gap-1.5">
              <span class="text-[9px] text-wool-600 uppercase tracking-wide">Depth</span>
              <div class="flex items-center rounded overflow-hidden" style={{ background: 'rgba(64, 64, 64, 0.4)' }}>
                <button
                  onClick={() => props.setGranularity('all')}
                  class="px-2 py-0.5 text-[10px] font-medium transition-colors"
                  style={{
                    background: props.granularity() === 'all' ? 'rgba(212, 165, 116, 0.25)' : 'transparent',
                    color: props.granularity() === 'all' ? 'var(--amber-400)' : 'var(--wool-500)',
                  }}
                >
                  All
                </button>
                <button
                  onClick={() => props.setGranularity('2')}
                  class="px-2 py-0.5 text-[10px] font-medium transition-colors"
                  style={{
                    background: props.granularity() === '2' ? 'rgba(212, 165, 116, 0.25)' : 'transparent',
                    color: props.granularity() === '2' ? 'var(--amber-400)' : 'var(--wool-500)',
                  }}
                >
                  2
                </button>
                <button
                  onClick={() => props.setGranularity('1')}
                  class="px-2 py-0.5 text-[10px] font-medium transition-colors"
                  style={{
                    background: props.granularity() === '1' ? 'rgba(212, 165, 116, 0.25)' : 'transparent',
                    color: props.granularity() === '1' ? 'var(--amber-400)' : 'var(--wool-500)',
                  }}
                >
                  1
                </button>
              </div>
            </div>

            {/* Show filters */}
            <div class="flex items-center gap-1.5">
              <span class="text-[9px] text-wool-600 uppercase tracking-wide">Show</span>
              <button
                onClick={() => {
                  const current = props.liveFilters();
                  props.setLiveFilters(
                    current.includes('worker-tasks')
                      ? current.filter(f => f !== 'worker-tasks')
                      : [...current, 'worker-tasks']
                  );
                }}
                class="flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-medium transition-colors"
                style={{
                  background: props.liveFilters().includes('worker-tasks') ? 'rgba(212, 165, 116, 0.2)' : 'rgba(64, 64, 64, 0.4)',
                  color: props.liveFilters().includes('worker-tasks') ? 'var(--amber-400)' : 'var(--wool-500)',
                }}
                title="Tasks added by workers"
              >
                <Icon name="sparkles" class="w-3 h-3" />
                <span>Added</span>
              </button>
              <button
                onClick={() => {
                  const current = props.liveFilters();
                  props.setLiveFilters(
                    current.includes('deleted-nodes')
                      ? current.filter(f => f !== 'deleted-nodes')
                      : [...current, 'deleted-nodes']
                  );
                }}
                class="flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-medium transition-colors"
                style={{
                  background: props.liveFilters().includes('deleted-nodes') ? 'rgba(196, 92, 74, 0.15)' : 'rgba(64, 64, 64, 0.4)',
                  color: props.liveFilters().includes('deleted-nodes') ? 'var(--terra)' : 'var(--wool-500)',
                }}
                title="Deleted nodes"
              >
                <Icon name="trash-2" class="w-3 h-3" />
                <span>Deleted</span>
              </button>
            </div>
          </Show>
        </div>
      </div>

      {/* Worker Detail Modal */}
      <Show when={selectedWorker()}>
        {(worker) => (
          <WorkerDetailModal
            worker={worker()}
            metricsAvailable={true}
            runName={delta.projectRun()?.runName || ''}
            onClose={() => setSelectedWorker(null)}
            onAttach={handleAttachWorker}
            onOpenDM={handleOpenWorkerDM}
          />
        )}
      </Show>
    </>
  );
};

export default CanvasToolbar;
