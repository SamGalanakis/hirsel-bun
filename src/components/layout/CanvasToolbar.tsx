/**
 * CanvasToolbar - Toolbar bar above the canvas
 *
 * Contains:
 * - Sidebar collapse toggle (left)
 * - Run status + workers (center)
 * - Live tree filters: depth + show worker-added tasks + edge toggles (right)
 */
import { type Component, Show, For, createSignal, createEffect, onCleanup } from 'solid-js';
import { invoke } from '../../lib/invoke';
import { emit } from '../../lib/events';
import { useApp, useProject, useRuns } from '../../stores';
import { useDelta } from '../../stores/delta-context';
import { Dropdown, Icon, SheepAvatar } from '../shared';
import { amber } from '../../lib/theme-colors';
import { getWorkerGlowStyle } from '../../lib/utils/status';
import { RunStatusPill } from '../specflow/RunStatusPill';
import { WorkerDetailModal } from '../runs/WorkerDetailModal';
import { WorkerHalfMoon } from './WorkerHalfMoon';
import type { WorkerDisplay } from '../../lib/types';

interface CanvasToolbarProps {
  // Live tree filters
  granularity: () => 'all' | '2';
  setGranularity: (g: 'all' | '2') => void;
  liveFilters: () => string[];
  setLiveFilters: (filters: string[]) => void;
  // Edge toggles
  edgeToggles: () => {
    hierarchy: boolean;
    blockedBy: boolean;
    validates: boolean;
    resolves: boolean;
  };
  setEdgeToggles: (t: { hierarchy: boolean; blockedBy: boolean; validates: boolean; resolves: boolean }) => void;
  // Scope breadcrumb
  scopeName: () => string | null;
  clearScope: () => void;
  // Whether there's a live tree
  hasLiveTree: boolean;
}

export const CanvasToolbar: Component<CanvasToolbarProps> = (props) => {
  const app = useApp();
  const project = useProject();
  const delta = useDelta();
  const runsCtx = useRuns();

  // Sync selectedRun to projectRun — these signals update independently
  // and the race during startup/transitions can cause workers to appear empty
  createEffect(() => {
    const run = delta.projectRun();
    if (run && runsCtx.selectedRun() !== run.runName) {
      runsCtx.setSelectedRun(run.runName);
    }
  });

  // Workers state — read from RunsContext store (centralized polling)
  const workers = () => {
    const run = delta.projectRun();
    if (!run) return [] as WorkerDisplay[];
    return runsCtx.workers();
  };

  const [runValid, setRunValid] = createSignal(true);
  const [selectedWorker, setSelectedWorker] = createSignal<WorkerDisplay | null>(null);
  const [halfMoonWorker, setHalfMoonWorker] = createSignal<WorkerDisplay | null>(null);
  const [halfMoonVisible, setHalfMoonVisible] = createSignal(false);
  let hoverTimer: ReturnType<typeof setTimeout> | undefined;

  const handleWorkerHover = (worker: WorkerDisplay) => {
    clearTimeout(hoverTimer);
    setHalfMoonWorker(worker);
    hoverTimer = setTimeout(() => setHalfMoonVisible(true), 200);
  };

  const handleWorkerLeave = () => {
    clearTimeout(hoverTimer);
    // Delay closing to allow mouse to reach the half-moon items
    hoverTimer = setTimeout(() => {
      setHalfMoonVisible(false);
      setHalfMoonWorker(null);
    }, 150);
  };

  const closeHalfMoon = () => {
    clearTimeout(hoverTimer);
    setHalfMoonVisible(false);
    setHalfMoonWorker(null);
  };

  // Track run validity
  createEffect(() => {
    const run = delta.projectRun();
    setRunValid(!!run);
  });

  // Compute effective run status from live tree
  const liveRunStatus = () => {
    if (!runValid()) return null;

    const liveTrees = delta.boardTree();
    const run = delta.projectRun();

    if (!run) return null;
    if (!liveTrees || liveTrees.length === 0) return null;

    type BoardNode = (typeof liveTrees)[0];
    const flatten = (node: BoardNode): BoardNode[] => {
      return [node, ...(node.children?.flatMap(flatten) || [])];
    };
    const liveNodes = liveTrees.flatMap(flatten).filter(n => n.status !== 'draft');

    if (liveNodes.length === 0) return null;

    const hasWorkingNode = liveNodes.some(n => n.status === 'working');

    if (run.status === 'failed') return 'failed';
    if (run.status === 'paused') return 'paused';
    if (hasWorkingNode) return 'working';

    const allComplete = liveNodes.every(n => n.status === 'done' || n.status === 'validated' || n.status === 'awaiting_check');
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
    onCleanup(() => {
      document.removeEventListener('keydown', handler);
      clearTimeout(hoverTimer);
    });
  });

  return (
    <>
      <div
        class="items-center gap-2 px-2 py-1.5 border-b border-pasture-600/40"
        style={{ display: 'grid', 'grid-template-columns': '1fr auto 1fr', background: 'rgba(30, 30, 30, 0.6)' }}
      >
        {/* Left: Sidebar toggle */}
        <div class="flex items-center gap-2 justify-start">
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

        {/* Center: Scope + Status + Workers */}
        <div class="flex items-center justify-center gap-3">
          {/* Scope breadcrumb */}
          <div class="hidden md:flex items-center gap-1.5 text-[10px] text-wool-600">
            <span class="uppercase tracking-wide">Scope</span>
            <span class="text-wool-700">:</span>
            <button
              type="button"
              class="px-1.5 py-0.5 rounded hover:bg-white/5 text-wool-400"
              onClick={() => props.clearScope()}
              title="Clear scope"
            >
              All
            </button>
            <Show when={props.scopeName()}>
              <span class="text-wool-700">›</span>
              <span class="text-wool-300 max-w-[220px] truncate">{props.scopeName() || ''}</span>
            </Show>
          </div>

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
            <div
              class="flex items-center gap-2 rounded-lg px-2.5 py-1"
              style={{
                background: 'rgba(40, 40, 40, 0.5)',
                border: '1px solid rgba(64, 64, 64, 0.3)',
              }}
            >
              <For each={workers().slice(0, 5)}>
                {(worker, index) => {
                  const glow = () => getWorkerGlowStyle(worker.status);
                  const isHovered = () => halfMoonWorker()?.name === worker.name && halfMoonVisible();
                  const avatarShadow = () => {
                    const base = glow();
                    const ring = isHovered() ? '0 0 0 2px var(--amber-500), 0 0 12px rgba(var(--amber-500-rgb), 0.3)' : '';
                    if (base && ring) return `${base}, ${ring}`;
                    return ring || base;
                  };
                  return (
                    <div
                      class="relative"
                      onMouseEnter={() => handleWorkerHover(worker)}
                      onMouseLeave={handleWorkerLeave}
                    >
                      <button
                        type="button"
                        class="relative rounded-full focus:outline-none"
                        style={{
                          'box-shadow': avatarShadow(),
                          transform: isHovered() ? 'scale(1.1)' : undefined,
                          transition: 'box-shadow 150ms ease-out, transform 150ms ease-out',
                        }}
                        onClick={() => setSelectedWorker(worker)}
                      >
                        <SheepAvatar
                          config={worker.sheepConfig}
                          size={34}
                          status={worker.status}
                        />
                      </button>
                      {/* Half-moon radial menu */}
                      <Show when={halfMoonWorker()?.name === worker.name && halfMoonVisible()}>
                        <WorkerHalfMoon
                          worker={worker}
                          index={index()}
                          onDetails={() => { setSelectedWorker(worker); closeHalfMoon(); }}
                          onSpectate={() => {
                            const run = delta.projectRun();
                            if (run) {
                              emit('show-worker-output', { runName: run.runName, workerName: worker.name });
                            }
                            closeHalfMoon();
                          }}
                          onMessage={() => { project.openWorkerDM(worker.name); closeHalfMoon(); }}
                          onClose={closeHalfMoon}
                        />
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
        <div class="flex items-center gap-3 justify-end">
          <Show when={props.hasLiveTree}>
            {/* Edge toggles */}
            <div class="hidden lg:flex items-center gap-1.5">
              <span class="text-[9px] text-wool-600 uppercase tracking-wide">Edges</span>
              <div class="flex items-center rounded overflow-hidden" style={{ background: 'rgba(64, 64, 64, 0.4)' }}>
                <button
                  onClick={() => props.setEdgeToggles({ ...props.edgeToggles(), hierarchy: !props.edgeToggles().hierarchy })}
                  class="px-2 py-0.5 text-[10px] font-medium transition-colors"
                  style={{
                    background: props.edgeToggles().hierarchy ? amber(0.22) : 'transparent',
                    color: props.edgeToggles().hierarchy ? 'var(--amber-400)' : 'var(--wool-500)',
                  }}
                  title="Hierarchy edges"
                >
                  H
                </button>
                <button
                  onClick={() => props.setEdgeToggles({ ...props.edgeToggles(), blockedBy: !props.edgeToggles().blockedBy })}
                  class="px-2 py-0.5 text-[10px] font-medium transition-colors"
                  style={{
                    background: props.edgeToggles().blockedBy ? amber(0.22) : 'transparent',
                    color: props.edgeToggles().blockedBy ? 'var(--amber-400)' : 'var(--wool-500)',
                  }}
                  title="BlockedBy edges"
                >
                  B
                </button>
                <button
                  onClick={() => props.setEdgeToggles({ ...props.edgeToggles(), validates: !props.edgeToggles().validates })}
                  class="px-2 py-0.5 text-[10px] font-medium transition-colors"
                  style={{
                    background: props.edgeToggles().validates ? amber(0.22) : 'transparent',
                    color: props.edgeToggles().validates ? 'var(--amber-400)' : 'var(--wool-500)',
                  }}
                  title="Validates edges"
                >
                  V
                </button>
                <button
                  onClick={() => props.setEdgeToggles({ ...props.edgeToggles(), resolves: !props.edgeToggles().resolves })}
                  class="px-2 py-0.5 text-[10px] font-medium transition-colors"
                  style={{
                    background: props.edgeToggles().resolves ? amber(0.22) : 'transparent',
                    color: props.edgeToggles().resolves ? 'var(--amber-400)' : 'var(--wool-500)',
                  }}
                  title="Resolves edges"
                >
                  R
                </button>
              </div>
            </div>

            {/* Depth selector */}
            <div class="flex items-center gap-1.5">
              <span class="text-[9px] text-wool-600 uppercase tracking-wide">Depth</span>
              <Dropdown
                value={props.granularity()}
                options={[
                  { value: 'all', label: 'All' },
                  { value: '2', label: '2' },
                ]}
                onChange={(v) => props.setGranularity(v as 'all' | '2')}
                class="w-[88px]"
                triggerClass="btn-outline w-full justify-between h-7 px-2 py-1 text-[10px] bg-[rgba(64,64,64,0.35)] border border-white/5 hover:bg-[rgba(64,64,64,0.45)] text-wool-300"
                panelClass="absolute z-50 mt-1 w-full rounded-md shadow-md py-1 max-h-60 overflow-auto bg-[rgba(28,28,30,0.98)] border border-white/10 backdrop-blur-xl"
              />
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
                  background: props.liveFilters().includes('worker-tasks') ? amber(0.2) : 'rgba(64, 64, 64, 0.4)',
                  color: props.liveFilters().includes('worker-tasks') ? 'var(--amber-400)' : 'var(--wool-500)',
                }}
                title="Tasks added by workers"
              >
                <Icon name="sparkles" class="w-3 h-3" />
                <span>Added</span>
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
            onAttach={() => {
              const w = selectedWorker();
              const run = delta.projectRun();
              if (w && run) {
                emit('show-worker-output', { runName: run.runName, workerName: w.name });
              }
              setSelectedWorker(null);
            }}
            onOpenDM={() => {
              const w = selectedWorker();
              if (w) project.openWorkerDM(w.name);
              setSelectedWorker(null);
            }}
          />
        )}
      </Show>
    </>
  );
};

export default CanvasToolbar;
