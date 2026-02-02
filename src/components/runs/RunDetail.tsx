/**
 * Run detail view with all tabs (Overview, Work, Chat, Config)
 * Full port from Alpine.js to SolidJS with Basecoat UI
 */
import { invoke } from '@tauri-apps/api/core';
import {
  type Component,
  For,
  type JSX,
  Show,
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
} from 'solid-js';
import { generateSheepSvg } from '../../lib/sheep-avatar';
import type {
  Eval,
  HistoryEntry,
  Message,
  Task,
  TaskDisplay,
  ThreadSummary,
  WorkerDisplay,
} from '../../lib/types';
import {
  calculateTimeProgress,
  formatDuration,
  formatElapsed,
  formatRelativeTime,
  formatTimeRemaining,
} from '../../lib/utils/formatters';
import { useRuns, useSelection } from '../../stores';
import { Icon } from '../shared';
import { ActivityLog } from './ActivityLog';
import { TaskDetailModal } from './TaskDetailModal';
import { TaskTreeView } from './TaskTreeView';
import { WorkerCard } from './WorkerCard';
import { WorkerDetailModal } from './WorkerDetailModal';

// =============================================================================
// Helper Components
// =============================================================================

/** Status badge component */
const StatusBadge: Component<{ status: string }> = (props) => {
  const colorMap: Record<string, string> = {
    draft: 'bg-sky-500/20 text-sky-400',
    working: 'bg-amber-500/20 text-amber-400',
    paused: 'bg-golden/20 text-golden',
    failed: 'bg-terra/20 text-terra',
    eval: 'bg-amber-400/20 text-amber-300',
    done: 'bg-sage/20 text-sage',
    delivered: 'bg-sage/20 text-sage',
    waiting: 'bg-golden/20 text-golden',
    merged: 'bg-sage/20 text-sage',
    idle: 'bg-wool-500/20 text-wool-400',
    runaway: 'bg-terra/20 text-terra',
    timed_out: 'bg-terra/20 text-terra',
    eval_failed: 'bg-terra/20 text-terra',
    // Task statuses
    todo: 'bg-wool-500/20 text-wool-400',
    doing: 'bg-amber-500/20 text-amber-400',
    // Eval statuses
    running: 'bg-amber-500/20 text-amber-400',
    passed: 'bg-sage/20 text-sage',
  };

  return (
    <span
      class={`px-2 py-0.5 text-xs rounded ${colorMap[props.status] || 'bg-wool-600/20 text-wool-400'}`}
    >
      {props.status}
    </span>
  );
};

/** Tab button with icon */
const TabButton: Component<{
  active: boolean;
  onClick: () => void;
  icon: string;
  label: string;
  badge?: number;
  badgeHighlight?: boolean;
}> = (props) => (
  <button
    role="tab"
    aria-selected={props.active}
    onClick={props.onClick}
    class="px-3 py-2 text-sm flex items-center gap-1.5"
  >
    <Icon name={props.icon} class="w-4 h-4" />
    {props.label}
    <Show when={props.badge && props.badge > 0}>
      <span
        class={`ml-1 px-1.5 py-0.5 text-xs rounded-full ${
          props.badgeHighlight ? 'bg-amber-500 text-pasture-900' : 'bg-pasture-600'
        }`}
      >
        {props.badge}
      </span>
    </Show>
  </button>
);

/** Config row for key-value display */
const ConfigRow: Component<{
  label: string;
  value?: string;
  children?: JSX.Element;
}> = (props) => (
  <div class="flex items-center justify-between py-2.5 border-b border-pasture-700/50 last:border-0">
    <span class="text-sm text-wool-400">{props.label}</span>
    <Show when={props.children} fallback={
      <span class="text-sm text-wool-100 font-medium">{props.value}</span>
    }>
      {props.children}
    </Show>
  </div>
);

// =============================================================================
// Main Component
// =============================================================================

export const RunDetail: Component = () => {
  const runs = useRuns();
  const selection = useSelection();

  // Lazy-loaded tab content
  const [diffStats, setDiffStats] = createSignal<{
    filesChanged: number;
    insertions: number;
    deletions: number;
  } | null>(null);

  // Evals (loaded in work tab)
  const [evals, setEvals] = createSignal<Eval[]>([]);

  // Chat
  const [messages, setMessages] = createSignal<Message[]>([]);
  const [messageInput, setMessageInput] = createSignal('');
  const [sendingMessage, setSendingMessage] = createSignal(false);

  // Deliver modal
  const [showDeliverModal, setShowDeliverModal] = createSignal(false);
  const [deliverBranch, setDeliverBranch] = createSignal('');
  const [delivering, setDelivering] = createSignal(false);

  // Worker selection
  const [selectedWorkerId, setSelectedWorkerId] = createSignal<number | null>(null);
  const [showWorkerModal, setShowWorkerModal] = createSignal(false);

  // Task selection
  const [selectedTaskId, setSelectedTaskId] = createSignal<string | null>(null);
  const [showTaskModal, setShowTaskModal] = createSignal(false);
  const [collapsedTasks, setCollapsedTasks] = createSignal<Set<string>>(new Set());
  const [showAllTasks, setShowAllTasks] = createSignal(true); // Toggle for task source filter

  // Run actions menu
  const [showRunMenu, setShowRunMenu] = createSignal(false);


  const detail = () => runs.runDetail();
  const workers = () => runs.workers();
  const tasks = () => runs.tasks();
  const threads = () => runs.threads();
  const history = () => runs.history();
  const runName = () => runs.selectedRun();

  // =============================================================================
  // Task Tree Building
  // =============================================================================

  const taskTree = createMemo((): TaskDisplay[] => {
    const taskList = tasks();
    const taskMap = new Map<string, TaskDisplay>();
    const roots: TaskDisplay[] = [];

    // First pass: create TaskDisplay objects
    for (const task of taskList) {
      const display: TaskDisplay = {
        ...task,
        isBlocked: (task.blockedBy || []).length > 0,
        children: [],
        depth: 0,
      };
      taskMap.set(task.id, display);
    }

    // Second pass: build hierarchy
    for (const task of taskList) {
      const display = taskMap.get(task.id)!;
      if (task.parentId && taskMap.has(task.parentId)) {
        const parent = taskMap.get(task.parentId)!;
        display.depth = parent.depth + 1;
        parent.children.push(display);
      } else {
        roots.push(display);
      }
    }

    // Flatten for display (respecting collapse state)
    const flatList: TaskDisplay[] = [];
    const collapsed = collapsedTasks();

    const flatten = (items: TaskDisplay[]) => {
      for (const item of items) {
        flatList.push(item);
        if (!collapsed.has(item.id) && item.children.length > 0) {
          flatten(item.children);
        }
      }
    };
    flatten(roots);
    return flatList;
  });

  // Task stats
  const taskStats = createMemo(() => {
    const all = tasks();
    const done = all.filter((t) => t.status === 'done').length;
    const total = all.length;
    const percent = total > 0 ? Math.round((done / total) * 100) : 0;
    return { done, total, percent };
  });

  // =============================================================================
  // Data Loading Effects
  // =============================================================================

  // Load evals when work tab is selected
  createEffect(() => {
    if (selection.activeTab() === 'work' && runName()) {
      const name = runName()!;
      invoke<Eval[]>('get_evals', { runName: name })
        .then((result) => setEvals(result || []))
        .catch(() => setEvals([]));
    }
  });

  // Load messages when thread is selected
  createEffect(() => {
    const thread = selection.selectedThread();
    const name = runName();
    if (!thread || !name) {
      setMessages([]);
      return;
    }

    invoke<Message[]>('get_messages', { runName: name, threadName: thread })
      .then((result) => {
        setMessages(result || []);
        invoke('mark_messages_read', { runName: name, threadName: thread, reader: 'human' }).catch(() => {});
      })
      .catch(() => setMessages([]));
  });

  // Auto-select first thread
  createEffect(() => {
    if (
      selection.activeTab() === 'chat' &&
      !selection.selectedThread() &&
      threads().length > 0
    ) {
      selection.setSelectedThread(threads()[0].name);
    }
  });

  // Load diff stats
  createEffect(() => {
    if (selection.activeTab() === 'overview' && runName()) {
      invoke<{ filesChanged: number; insertions: number; deletions: number }>(
        'get_diff_stats',
        { runName: runName()! }
      )
        .then(setDiffStats)
        .catch(() => setDiffStats(null));
    }
  });

  // Re-create Lucide icons on tab change
  createEffect(() => {
    const _tab = selection.activeTab();
    requestAnimationFrame(() => {
      window.lucide?.createIcons({ inTemplates: true });
    });
  });

  // =============================================================================
  // Actions
  // =============================================================================

  const sendMessage = async () => {
    const thread = selection.selectedThread();
    const name = runName();
    const content = messageInput().trim();
    if (!thread || !name || !content || sendingMessage()) return;

    setSendingMessage(true);
    try {
      await invoke('send_message', { runName: name, threadName: thread, content });
      setMessageInput('');
      const result = await invoke<Message[]>('get_messages', { runName: name, threadName: thread });
      setMessages(result || []);
    } catch (e) {
      console.error('Failed to send message:', e);
      window.toast?.error(`Failed to send message: ${e}`);
    } finally {
      setSendingMessage(false);
    }
  };

  const completeTask = async (taskId: string) => {
    const name = runName();
    if (!name) return;
    try {
      await invoke('complete_task', { runName: name, taskId });
      await runs.invalidateTasks();
    } catch (e) {
      window.toast?.error(`Failed to complete task: ${e}`);
    }
  };

  const reopenTask = async (taskId: string) => {
    const name = runName();
    if (!name) return;
    try {
      await invoke('reopen_task', { runName: name, taskId });
      await runs.invalidateTasks();
    } catch (e) {
      window.toast?.error(`Failed to reopen task: ${e}`);
    }
  };

  const unclaimTask = async (taskId: string) => {
    const name = runName();
    if (!name) return;
    try {
      await invoke('unclaim_task', { runName: name, taskId });
      await runs.invalidateTasks();
    } catch (e) {
      window.toast?.error(`Failed to unclaim task: ${e}`);
    }
  };

  const deleteTask = async (taskId: string) => {
    const name = runName();
    if (!name) return;
    const confirmed = await window.confirmDialog?.delete('this task', 'task');
    if (!confirmed) return;
    try {
      await invoke('delete_task', { runName: name, taskId });
      await runs.invalidateTasks();
      setShowTaskModal(false);
    } catch (e) {
      window.toast?.error(`Failed to delete task: ${e}`);
    }
  };

  const pauseRun = async () => {
    const name = runName();
    if (!name) return;
    try {
      await invoke('pause_run', { runName: name });
      await runs.invalidateRuns();
      window.toast?.success('Run paused');
    } catch (e) {
      window.toast?.error(`Failed to pause run: ${e}`);
    }
  };

  const resumeRun = async () => {
    const name = runName();
    if (!name) return;
    try {
      await invoke('resume_run', { runName: name });
      await runs.invalidateRuns();
      window.toast?.success('Run resumed');
    } catch (e) {
      window.toast?.error(`Failed to resume run: ${e}`);
    }
  };

  const deliverRun = async () => {
    const name = runName();
    if (!name) return;
    setDelivering(true);
    try {
      const branch = deliverBranch().trim() || undefined;
      await invoke('deliver_run', { runName: name, branch });
      await runs.invalidateRuns();
      setShowDeliverModal(false);
      window.toast?.success('Run delivered');
    } catch (e) {
      window.toast?.error(`Failed to deliver run: ${e}`);
    } finally {
      setDelivering(false);
    }
  };

  const deleteRun = async () => {
    const name = runName();
    if (!name) return;
    const confirmed = await window.confirmDialog?.delete(`run "${name}"`, 'run');
    if (!confirmed) return;
    try {
      await invoke('delete_run', { runName: name });
      runs.setSelectedRun(null);
      await runs.invalidateRuns();
      window.toast?.success('Run deleted');
    } catch (e) {
      window.toast?.error(`Failed to delete run: ${e}`);
    }
  };

  const openWorkerOutput = (worker: WorkerDisplay) => {
    const name = runName();
    if (!name) return;
    window.dispatchEvent(
      new CustomEvent('show-worker-output', {
        detail: { runName: name, workerName: worker.name },
      })
    );
  };

  const toggleTaskCollapse = (taskId: string) => {
    setCollapsedTasks((prev) => {
      const next = new Set(prev);
      if (next.has(taskId)) {
        next.delete(taskId);
      } else {
        next.add(taskId);
      }
      return next;
    });
  };

  // Get selected worker for modal
  const selectedWorker = () => {
    const id = selectedWorkerId();
    if (!id) return null;
    return workers().find((w) => w.id === id) || null;
  };

  // Get selected task for modal
  const selectedTask = () => {
    const id = selectedTaskId();
    if (!id) return null;
    return tasks().find((t) => t.id === id) || null;
  };

  // Thread categorization - group vs DM (worker-specific)
  // Worker threads are those whose name matches a worker name
  const workerNames = () => new Set(workers().map((w) => w.name));
  const groupThreads = () => threads().filter((t) => !workerNames().has(t.name));
  const workerThreads = () => threads().filter((t) => workerNames().has(t.name));

  // Get thread icon
  const getThreadIcon = (threadName: string) => {
    if (threadName === 'human') return 'user';
    if (threadName === 'broadcast') return 'radio';
    return 'message-square';
  };

  return (
    <div class="flex-1 flex flex-col overflow-hidden">
      {/* ========== HEADER ========== */}
      <div class="p-4 border-b border-pasture-600">
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-3">
            <h2 class="text-lg font-medium text-wool-100">{runName()}</h2>
            <Show when={detail()}>
              <StatusBadge status={detail()!.status} />
            </Show>
            {/* HITL Indicator */}
            <Show when={detail()?.humanInTheLoop}>
              <span
                class="text-xs text-amber-400 flex items-center gap-1"
                data-tooltip="Human in the Loop"
              >
                <Icon name="user" class="w-3 h-3" /> HITL
              </span>
            </Show>
          </div>
          {/* Actions menu */}
          <div class="relative">
            <button
              class="p-2 rounded-md hover:bg-pasture-700 text-wool-400 hover:text-wool-200 transition-colors"
              onClick={() => setShowRunMenu(!showRunMenu())}
              title="Run actions"
            >
              <Icon name="more-horizontal" class="w-5 h-5" />
            </button>

            <Show when={showRunMenu()}>
              {/* Backdrop */}
              <div class="fixed inset-0 z-40" onClick={() => setShowRunMenu(false)} />

              {/* Menu dropdown */}
              <div class="absolute right-0 top-full mt-1 w-48 bg-pasture-800 border border-pasture-600 rounded-lg shadow-xl overflow-hidden z-50">
                {/* Pause - only when working */}
                <Show when={detail()?.status === 'working'}>
                  <button
                    class="w-full flex items-center gap-2.5 px-3 py-2 text-sm text-wool-300 hover:bg-pasture-700 hover:text-wool-100 transition-colors"
                    onClick={() => {
                      setShowRunMenu(false);
                      pauseRun();
                    }}
                  >
                    <Icon name="pause" class="w-4 h-4 text-golden" />
                    <span>Pause</span>
                  </button>
                </Show>

                {/* Resume - when paused or waiting */}
                <Show when={detail()?.status === 'paused' || detail()?.status === 'waiting'}>
                  <button
                    class="w-full flex items-center gap-2.5 px-3 py-2 text-sm text-wool-300 hover:bg-pasture-700 hover:text-wool-100 transition-colors"
                    onClick={() => {
                      setShowRunMenu(false);
                      resumeRun();
                    }}
                  >
                    <Icon name="play" class="w-4 h-4 text-sage" />
                    <span>Resume</span>
                  </button>
                </Show>

                {/* Deliver - when done or eval */}
                <Show when={detail()?.status === 'done' || detail()?.status === 'eval'}>
                  <button
                    class="w-full flex items-center gap-2.5 px-3 py-2 text-sm text-wool-300 hover:bg-pasture-700 hover:text-wool-100 transition-colors"
                    onClick={() => {
                      setShowRunMenu(false);
                      setShowDeliverModal(true);
                    }}
                  >
                    <Icon name="git-pull-request" class="w-4 h-4 text-amber-400" />
                    <span>Deliver</span>
                  </button>
                </Show>

                {/* Divider */}
                <div class="border-t border-pasture-600 my-1" />

                {/* Delete - always available */}
                <button
                  class="w-full flex items-center gap-2.5 px-3 py-2 text-sm text-terra hover:bg-terra/10 transition-colors"
                  onClick={() => {
                    setShowRunMenu(false);
                    deleteRun();
                  }}
                >
                  <Icon name="trash-2" class="w-4 h-4" />
                  <span>Delete run</span>
                </button>
              </div>
            </Show>
          </div>
        </div>

        {/* Time Progress Bar */}
        <Show when={detail()?.timeLimitMinutes}>
          <div class="mt-3">
            <div class="flex justify-between text-xs text-wool-500 mb-1">
              <span>{formatElapsed(detail()?.elapsedMinutes)} elapsed</span>
              <span>
                {formatTimeRemaining(detail()?.timeLimitMinutes, detail()?.elapsedMinutes)}
              </span>
            </div>
            <div class="h-1.5 bg-pasture-600 rounded-full overflow-hidden">
              <div
                class={`h-full rounded-full transition-all ${
                  calculateTimeProgress(detail()?.elapsedMinutes, detail()?.timeLimitMinutes) > 80
                    ? 'bg-terra'
                    : 'bg-amber-500'
                }`}
                style={{
                  width: `${calculateTimeProgress(detail()?.elapsedMinutes, detail()?.timeLimitMinutes)}%`,
                }}
              />
            </div>
          </div>
        </Show>

        {/* No time limit - show elapsed only */}
        <Show when={!detail()?.timeLimitMinutes && detail()?.startedAt}>
          <div class="flex items-center gap-2 text-xs text-wool-500 mt-3">
            <span>{formatElapsed(detail()?.elapsedMinutes)} elapsed</span>
            <span class="text-wool-600">·</span>
            <span class="flex items-center gap-1 text-wool-600">
              <Icon name="infinity" class="w-3 h-3" /> no limit
            </span>
          </div>
        </Show>

        {/* Waiting Reason Banner */}
        <Show when={detail()?.waitingReason}>
          <div class="mt-3 p-2 bg-golden/10 border border-golden/30 rounded text-sm text-golden">
            <span class="font-medium">Waiting:</span> {detail()?.waitingReason}
          </div>
        </Show>
      </div>

      {/* ========== TAB NAVIGATION ========== */}
      <div class="tabs border-b border-pasture-600">
        <nav role="tablist" aria-orientation="horizontal" class="px-4">
          <TabButton
            active={selection.activeTab() === 'overview'}
            onClick={() => selection.setActiveTab('overview')}
            icon="layout-dashboard"
            label="Overview"
          />
          <TabButton
            active={selection.activeTab() === 'work'}
            onClick={() => selection.setActiveTab('work')}
            icon="list-checks"
            label="Work"
            badge={(tasks()?.length || 0) + (evals()?.length || 0)}
          />
          <TabButton
            active={selection.activeTab() === 'chat'}
            onClick={() => selection.setActiveTab('chat')}
            icon="message-square"
            label="Chat"
            badge={detail()?.unreadCount}
            badgeHighlight={true}
          />
          <TabButton
            active={selection.activeTab() === 'config'}
            onClick={() => selection.setActiveTab('config')}
            icon="settings"
            label="Config"
          />
        </nav>
      </div>

      {/* ========== TAB CONTENT ========== */}
      <div class="flex-1 overflow-auto">
        {/* ==================== OVERVIEW TAB ==================== */}
        <Show when={selection.activeTab() === 'overview'}>
          <div class="p-4 h-full flex flex-col gap-3">
            {/* Stats row - compact inline */}
            <div class="flex items-center gap-4 text-sm flex-shrink-0">
              <div class="flex items-center gap-1.5">
                <Icon name="users" class="w-3.5 h-3.5 text-wool-500" />
                <span class="text-wool-500">Workers</span>
                <span class="font-semibold tabular-nums">
                  <span class="text-amber-400">{detail()?.workersActive}</span>
                  <span class="text-wool-600">/</span>
                  <span class="text-wool-300">{detail()?.workersTotal}</span>
                </span>
              </div>
              <span class="text-wool-700">·</span>
              <div class="flex items-center gap-1.5">
                <Icon name="list-checks" class="w-3.5 h-3.5 text-wool-500" />
                <span class="text-wool-500">Tasks</span>
                <span class="font-semibold tabular-nums">
                  <span class="text-sage">{detail()?.tasksDone}</span>
                  <span class="text-wool-600">/</span>
                  <span class="text-wool-300">{detail()?.tasksTotal}</span>
                </span>
              </div>
              <span class="text-wool-700">·</span>
              <div class="flex items-center gap-1.5">
                <Icon name="clock" class="w-3.5 h-3.5 text-wool-500" />
                <span class="font-semibold text-wool-300 tabular-nums">
                  {Math.round(detail()?.elapsedMinutes || 0)}m
                  <Show when={detail()?.timeLimitMinutes}>
                    <span class="text-wool-600 text-xs">/{detail()?.timeLimitMinutes}m</span>
                  </Show>
                </span>
              </div>
              <Show when={diffStats()}>
                <span class="text-wool-700">·</span>
                <div class="flex items-center gap-1.5">
                  <Icon name="file-diff" class="w-3.5 h-3.5 text-wool-500" />
                  <span class="text-wool-300 tabular-nums">{diffStats()?.filesChanged} files</span>
                  <span class="text-xs tabular-nums">
                    <span class="text-sage">+{diffStats()?.insertions}</span>
                    <span class="text-wool-600">/</span>
                    <span class="text-terra">-{diffStats()?.deletions}</span>
                  </span>
                </div>
              </Show>
            </div>

            {/* Workers horizontal strip */}
            <div class="flex-shrink-0">
              <Show when={workers().length === 0}>
                <div class="flex items-center gap-2 py-2 text-xs text-wool-600">
                  <Icon name="users" class="w-4 h-4" />
                  <span>No workers yet</span>
                </div>
              </Show>
              <Show when={workers().length > 0}>
                <div class="flex gap-2 overflow-x-auto pb-2">
                  <For each={workers()}>
                    {(worker) => (
                      <WorkerCard
                        worker={worker}
                        metricsAvailable={detail()?.metricsAvailable || false}
                        selected={selectedWorkerId() === worker.id}
                        compact={true}
                        onClick={() => {
                          setSelectedWorkerId(worker.id);
                          setShowWorkerModal(true);
                        }}
                        onDoubleClick={() => {
                          openWorkerOutput(worker);
                        }}
                      />
                    )}
                  </For>
                </div>
              </Show>
            </div>

            {/* Activity panel - takes remaining space */}
            <div class="flex-1 flex flex-col min-h-0 bg-pasture-800/50 rounded-lg border border-pasture-600 overflow-hidden">
              <div class="flex items-center gap-2 px-3 py-2 border-b border-pasture-600">
                <Icon name="activity" class="w-4 h-4 text-wool-500" />
                <h3 class="text-xs font-semibold text-wool-400 uppercase tracking-wide">Activity</h3>
              </div>
              <div class="flex-1 overflow-auto">
                <ActivityLog history={history()} workers={workers()} class="h-full" />
              </div>
            </div>
          </div>
        </Show>

        {/* ==================== WORK TAB (Tasks + Evals) ==================== */}
        <Show when={selection.activeTab() === 'work'}>
          <div class="flex h-full">
            {/* Left: Task tree graph view */}
            <div class="w-[360px] border-r border-pasture-600 flex flex-col overflow-hidden">
              <div class="work-graph flex-1 overflow-auto">
                {/* Empty state */}
                <Show when={tasks().length === 0 && evals().length === 0}>
                  <div class="work-empty-state h-full">
                    <div class="work-empty-icon">
                      <Icon name="git-branch" class="w-7 h-7" />
                    </div>
                    <p class="work-empty-text">No work items yet</p>
                    <p class="text-xs text-wool-600">Tasks will appear as the run progresses</p>
                  </div>
                </Show>

                {/* Tasks section - Tree visualization */}
                <Show when={tasks().length > 0}>
                  <div class="work-group-header">
                    <div class="work-group-header-icon">
                      <Icon name="git-branch" class="w-3 h-3" />
                    </div>
                    <span class="work-group-header-text">Tasks</span>
                    <div class="work-group-header-line" />
                    <button
                      class="ml-2 p-1 rounded hover:bg-pasture-700 text-wool-500 hover:text-wool-300"
                      onClick={() => setShowAllTasks(v => !v)}
                      title={showAllTasks() ? "Hide worker-added tasks" : "Show all tasks"}
                    >
                      <Icon name={showAllTasks() ? "layers" : "git-branch"} class="w-3.5 h-3.5" />
                    </button>
                  </div>

                  <TaskTreeView
                    tasks={tasks()}
                    workers={workers()}
                    selectedTaskId={selectedTaskId()}
                    onSelectTask={setSelectedTaskId}
                    showAllTasks={showAllTasks()}
                  />
                </Show>

                {/* Evals section */}
                <Show when={evals().length > 0}>
                  <div class="work-group-header mt-4">
                    <div class="work-group-header-icon" style="background: rgba(125,153,112,0.2)">
                      <Icon name="shield-check" class="w-3 h-3 text-sage" />
                    </div>
                    <span class="work-group-header-text text-sage/60">Evaluations</span>
                    <div class="work-group-header-line" />
                  </div>

                  <For each={evals()}>
                    {(evalItem) => {
                      const isSelected = () => selectedTaskId() === `eval-${evalItem.id}`;
                      const statusClass = () => {
                        switch (evalItem.status) {
                          case 'passed': return 'passed';
                          case 'failed': return 'failed';
                          case 'running': return 'running';
                          default: return 'todo';
                        }
                      };

                      return (
                        <div
                          class={`work-node eval-node status-${statusClass()}`}
                          classList={{ selected: isSelected() }}
                          onClick={() => setSelectedTaskId(`eval-${evalItem.id}`)}
                        >
                          {/* Status indicator */}
                          <div class={`work-node-status ${statusClass()}`} />

                          {/* Content */}
                          <div class="work-node-content">
                            <div class="work-node-name" classList={{ 'text-sage/80': !isSelected() }}>
                              {evalItem.evalName || 'Evaluation'}
                            </div>
                            <div class="work-node-meta">
                              <span class="work-node-type eval">
                                <Icon name="shield-check" class="w-2 h-2" />
                                eval
                              </span>
                              <Show when={evalItem.status === 'running'}>
                                <span class="text-amber-400">running...</span>
                              </Show>
                            </div>
                          </div>
                        </div>
                      );
                    }}
                  </For>
                </Show>
              </div>

              {/* Progress footer */}
              <Show when={taskStats().total > 0}>
                <div class="px-3 py-2.5 border-t border-pasture-600 bg-pasture-800/50">
                  <div class="flex items-center justify-between text-xs text-wool-500 mb-1.5">
                    <span>{taskStats().done} of {taskStats().total} complete</span>
                    <span class="tabular-nums">{taskStats().percent}%</span>
                  </div>
                  <div class="h-1 bg-pasture-600 rounded-full overflow-hidden">
                    <div
                      class="h-full bg-sage rounded-full transition-all duration-500"
                      style={{ width: `${taskStats().percent}%` }}
                    />
                  </div>
                </div>
              </Show>
            </div>

            {/* Right: Details pane */}
            <div class="work-details flex-1 overflow-auto">
              {/* No selection - empty state */}
              <Show when={!selectedTaskId()}>
                <div class="work-empty-state h-full">
                  <div class="work-empty-icon">
                    <Icon name="mouse-pointer-click" class="w-7 h-7" />
                  </div>
                  <p class="work-empty-text">Select a task to view details</p>
                  <p class="text-xs text-wool-600">Click any item in the list</p>
                </div>
              </Show>

              {/* Task details */}
              <Show when={selectedTaskId() && !selectedTaskId()?.startsWith('eval-')}>
                {(() => {
                  const task = () => tasks().find((t) => t.id === selectedTaskId());
                  const isBlocked = () => (task()?.blockedBy || []).length > 0;
                  const claimedWorker = () =>
                    task()?.claimedBy
                      ? workers().find((w) => w.name === task()?.claimedBy)
                      : null;

                  return (
                    <Show when={task()}>
                      <div class="flex flex-col h-full">
                        {/* Header - simplified, no empty box */}
                        <div class="p-4 border-b border-pasture-600">
                          <div class="flex items-center gap-2 mb-2">
                            <StatusBadge status={task()!.status} />
                            <Show when={isBlocked()}>
                              <span class="text-xs text-terra flex items-center gap-1">
                                <Icon name="lock" class="w-3 h-3" />
                                Blocked
                              </span>
                            </Show>
                          </div>
                          <h3 class="text-lg font-semibold text-wool-100" style="font-family: 'ET Book', serif;">
                            {task()!.description}
                          </h3>
                        </div>

                        {/* Actions bar */}
                        <div class="work-actions">
                          <Show when={task()!.status !== 'done'}>
                            <button class="btn btn-success" onClick={() => completeTask(task()!.id)}>
                              <Icon name="check" class="w-4 h-4" />
                              Complete
                            </button>
                          </Show>
                          <Show when={task()!.status === 'done'}>
                            <button class="btn-outline" onClick={() => reopenTask(task()!.id)}>
                              <Icon name="rotate-ccw" class="w-4 h-4" />
                              Reopen
                            </button>
                          </Show>
                          <Show when={task()!.claimedBy}>
                            <button class="btn-outline" onClick={() => unclaimTask(task()!.id)}>
                              <Icon name="user-minus" class="w-4 h-4" />
                              Unclaim
                            </button>
                          </Show>
                          <div class="flex-1" />
                          <button
                            class="btn-ghost text-terra hover:bg-terra/10"
                            onClick={() => deleteTask(task()!.id)}
                            title="Delete task"
                          >
                            <Icon name="trash-2" class="w-4 h-4" />
                          </button>
                        </div>

                        {/* Content */}
                        <div class="flex-1 overflow-auto p-4 space-y-4">
                          {/* Claimed worker card */}
                          <Show when={claimedWorker()}>
                            {(worker) => (
                              <div class="work-worker-card">
                                <div
                                  innerHTML={generateSheepSvg(worker().sheepConfig, 36)}
                                  class="w-9 h-9 shrink-0"
                                />
                                <div class="flex-1 min-w-0">
                                  <p class="text-sm font-medium text-wool-200">{worker().name}</p>
                                  <Show when={task()!.claimedAt}>
                                    <p class="text-xs text-wool-500">
                                      Working for {formatDuration(task()!.claimedAt, null)}
                                    </p>
                                  </Show>
                                </div>
                                <button
                                  class="btn-ghost text-xs"
                                  onClick={() => {
                                    setSelectedWorkerId(worker().id);
                                    setShowWorkerModal(true);
                                  }}
                                >
                                  View
                                </button>
                              </div>
                            )}
                          </Show>

                          {/* Meta info card */}
                          <div class="work-meta-grid">
                            <span class="work-meta-label">Task ID</span>
                            <span class="work-meta-value">{task()!.id}</span>

                            <Show when={task()!.parentId}>
                              <span class="work-meta-label">Parent</span>
                              <button
                                class="work-meta-value text-left hover:text-amber-400 transition-colors"
                                onClick={() => setSelectedTaskId(task()!.parentId!)}
                              >
                                {task()!.parentId}
                              </button>
                            </Show>

                            <Show when={task()!.blockedBy && task()!.blockedBy!.length > 0}>
                              <span class="work-meta-label">Blocked by</span>
                              <div class="flex flex-wrap gap-1">
                                <For each={task()!.blockedBy}>
                                  {(blockerId) => (
                                    <button
                                      class="text-xs px-1.5 py-0.5 rounded bg-terra/10 text-terra hover:bg-terra/20 transition-colors font-mono"
                                      onClick={() => setSelectedTaskId(blockerId)}
                                    >
                                      {blockerId.slice(0, 8)}...
                                    </button>
                                  )}
                                </For>
                              </div>
                            </Show>

                            <Show when={task()!.createdAt}>
                              <span class="work-meta-label">Created</span>
                              <span class="work-meta-value">{formatRelativeTime(task()!.createdAt)}</span>
                            </Show>
                          </div>
                        </div>
                      </div>
                    </Show>
                  );
                })()}
              </Show>

              {/* Eval details */}
              <Show when={selectedTaskId()?.startsWith('eval-')}>
                {(() => {
                  const evalId = () => Number.parseInt(selectedTaskId()!.replace('eval-', ''));
                  const evalItem = () => evals().find((e) => e.id === evalId());

                  const heroClass = () => {
                    switch (evalItem()?.status) {
                      case 'passed': return 'passed';
                      case 'failed': return 'failed';
                      case 'running': return 'running';
                      default: return '';
                    }
                  };

                  return (
                    <Show when={evalItem()}>
                      <div class="flex flex-col h-full">
                        {/* Header - simplified */}
                        <div class="p-4 border-b border-pasture-600">
                          <div class="flex items-center gap-2 mb-2">
                            <span
                              class={`px-2 py-0.5 text-xs rounded font-medium uppercase ${
                                evalItem()!.status === 'passed'
                                  ? 'bg-sage/20 text-sage'
                                  : evalItem()!.status === 'failed'
                                    ? 'bg-terra/20 text-terra'
                                    : 'bg-amber-500/20 text-amber-400'
                              }`}
                            >
                              {evalItem()!.status}
                            </span>
                            <span class="work-node-type eval">
                              <Icon name="shield-check" class="w-2.5 h-2.5" />
                              evaluation
                            </span>
                          </div>
                          <h3 class="text-lg font-semibold text-wool-100" style="font-family: 'ET Book', serif;">
                            {evalItem()!.evalName || 'Evaluation'}
                          </h3>
                          <p class="text-xs text-wool-500 mt-1">
                            {formatRelativeTime(evalItem()!.startedAt)}
                          </p>
                        </div>

                        {/* Content */}
                        <div class="flex-1 overflow-auto p-6">
                          {/* Status hero */}
                          <div class={`work-eval-hero ${heroClass()}`}>
                            <Show when={evalItem()!.status === 'passed'}>
                              <Icon name="check" class="w-10 h-10 text-sage" />
                            </Show>
                            <Show when={evalItem()!.status === 'failed'}>
                              <Icon name="x" class="w-10 h-10 text-terra" />
                            </Show>
                            <Show when={evalItem()!.status === 'running'}>
                              <div class="spinner w-8 h-8" />
                            </Show>
                            <Show
                              when={
                                evalItem()!.status !== 'passed' &&
                                evalItem()!.status !== 'failed' &&
                                evalItem()!.status !== 'running'
                              }
                            >
                              <Icon name="clock" class="w-10 h-10 text-wool-500" />
                            </Show>
                          </div>

                          {/* Feedback card */}
                          <Show when={evalItem()!.feedback}>
                            <div class="work-feedback-card mt-6">
                              <p class="text-xs text-sage/70 mb-1 uppercase tracking-wider font-semibold">
                                Feedback
                              </p>
                              <p class="text-sm text-wool-200" style="font-family: 'ET Book', serif;">
                                {evalItem()!.feedback}
                              </p>
                            </div>
                          </Show>

                          {/* Actions */}
                          <div class="flex items-center gap-2 mt-6">
                            <button
                              class="btn-outline"
                              onClick={() => {
                                const name = runName();
                                if (!name) return;
                                window.dispatchEvent(
                                  new CustomEvent('show-worker-output', {
                                    detail: {
                                      runName: name,
                                      workerName: evalItem()!.evalName || 'eval',
                                    },
                                  })
                                );
                              }}
                            >
                              <Icon name="terminal" class="w-4 h-4" />
                              View Full Output
                            </button>
                          </div>
                        </div>
                      </div>
                    </Show>
                  );
                })()}
              </Show>
            </div>
          </div>
        </Show>

        {/* ==================== CHAT TAB ==================== */}
        <Show when={selection.activeTab() === 'chat'}>
          <div class="flex h-full">
            {/* Thread list sidebar */}
            <div class="w-56 border-r border-pasture-600 flex flex-col bg-pasture-800/50">
              <div class="flex-1 overflow-auto">
                {/* Group chats section */}
                <Show when={groupThreads().length > 0}>
                  <div class="px-3 pt-3 pb-1.5">
                    <span class="text-[10px] font-semibold text-wool-600 uppercase tracking-wider">
                      Channels
                    </span>
                  </div>
                  <div class="px-2">
                    <For each={groupThreads()}>
                      {(thread) => (
                        <button
                          class={`w-full px-2.5 py-2 rounded-md text-left transition-colors ${
                            selection.selectedThread() === thread.name
                              ? 'bg-pasture-600'
                              : 'hover:bg-pasture-700/50'
                          }`}
                          onClick={() => selection.setSelectedThread(thread.name)}
                        >
                          <div class="flex items-center gap-2.5">
                            <div class={`w-7 h-7 rounded-md flex items-center justify-center ${
                              thread.name === 'group' ? 'bg-amber-500/20' : 'bg-pasture-600'
                            }`}>
                              <Show when={thread.name === 'group'}>
                                <svg class="w-4 h-4 text-amber-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0zm6 3a2 2 0 11-4 0 2 2 0 014 0zM7 10a2 2 0 11-4 0 2 2 0 014 0z" />
                                </svg>
                              </Show>
                              <Show when={thread.name !== 'group'}>
                                <svg class="w-4 h-4 text-wool-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 8h10M7 12h4m1 8l-4-4H5a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v8a2 2 0 01-2 2h-3l-4 4z" />
                                </svg>
                              </Show>
                            </div>
                            <div class="flex-1 min-w-0">
                              <div class="flex items-center justify-between">
                                <span class="text-sm font-medium text-wool-200 truncate">
                                  {thread.name}
                                </span>
                                <Show when={thread.unreadCount > 0}>
                                  <span class="ml-2 px-1.5 py-0.5 text-[10px] font-semibold rounded-full bg-amber-500 text-pasture-900 tabular-nums">
                                    {thread.unreadCount}
                                  </span>
                                </Show>
                              </div>
                              <Show when={thread.lastMessage}>
                                <p class="text-xs text-wool-600 truncate mt-0.5">{thread.lastMessage}</p>
                              </Show>
                            </div>
                          </div>
                        </button>
                      )}
                    </For>
                  </div>
                </Show>

                {/* Worker DMs section */}
                <Show when={workerThreads().length > 0}>
                  <div class="px-3 pt-4 pb-1.5">
                    <span class="text-[10px] font-semibold text-wool-600 uppercase tracking-wider">
                      Direct Messages
                    </span>
                  </div>
                  <div class="px-2">
                    <For each={workerThreads()}>
                      {(thread) => {
                        const worker = () => workers().find((w) => w.name === thread.name);
                        return (
                          <button
                            class={`w-full px-2.5 py-2 rounded-md text-left transition-colors ${
                              selection.selectedThread() === thread.name
                                ? 'bg-pasture-600'
                                : 'hover:bg-pasture-700/50'
                            }`}
                            onClick={() => selection.setSelectedThread(thread.name)}
                          >
                            <div class="flex items-center gap-2.5">
                              <Show
                                when={worker()}
                                fallback={
                                  <div class="w-7 h-7 rounded-full bg-pasture-600 flex items-center justify-center">
                                    <svg class="w-4 h-4 text-wool-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z" />
                                    </svg>
                                  </div>
                                }
                              >
                                <div
                                  innerHTML={generateSheepSvg(worker()!.sheepConfig, 28)}
                                  class="w-7 h-7 shrink-0"
                                />
                              </Show>
                              <div class="flex-1 min-w-0">
                                <div class="flex items-center justify-between">
                                  <span class="text-sm font-medium text-wool-200 truncate">
                                    {thread.name}
                                  </span>
                                  <Show when={thread.unreadCount > 0}>
                                    <span class="ml-2 px-1.5 py-0.5 text-[10px] font-semibold rounded-full bg-amber-500 text-pasture-900 tabular-nums">
                                      {thread.unreadCount}
                                    </span>
                                  </Show>
                                </div>
                                <Show when={thread.lastMessage}>
                                  <p class="text-xs text-wool-600 truncate mt-0.5">{thread.lastMessage}</p>
                                </Show>
                              </div>
                            </div>
                          </button>
                        );
                      }}
                    </For>
                  </div>
                </Show>

                {/* Empty state */}
                <Show when={threads().length === 0}>
                  <div class="flex flex-col items-center justify-center h-full p-6 text-center">
                    <div class="w-12 h-12 rounded-full bg-pasture-700 flex items-center justify-center mb-3">
                      <svg class="w-6 h-6 text-wool-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />
                      </svg>
                    </div>
                    <p class="text-sm text-wool-500">No conversations yet</p>
                    <p class="text-xs text-wool-600 mt-1">Messages will appear here</p>
                  </div>
                </Show>
              </div>
            </div>

            {/* Messages area */}
            <div class="flex-1 flex flex-col overflow-hidden bg-pasture-900">
              {/* Thread header */}
              <Show when={selection.selectedThread()}>
                <div class="px-4 py-3 border-b border-pasture-600 bg-pasture-800/50 shrink-0">
                  <div class="flex items-center gap-3">
                    {(() => {
                      const worker = () => workers().find((w) => w.name === selection.selectedThread());
                      return (
                        <Show
                          when={worker()}
                          fallback={
                            <div class="w-8 h-8 rounded-md bg-amber-500/20 flex items-center justify-center">
                              <svg class="w-4 h-4 text-amber-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0zm6 3a2 2 0 11-4 0 2 2 0 014 0zM7 10a2 2 0 11-4 0 2 2 0 014 0z" />
                              </svg>
                            </div>
                          }
                        >
                          <div
                            innerHTML={generateSheepSvg(worker()!.sheepConfig, 32)}
                            class="w-8 h-8 shrink-0"
                          />
                        </Show>
                      );
                    })()}
                    <div>
                      <h3 class="text-sm font-semibold text-wool-100">{selection.selectedThread()}</h3>
                      <p class="text-xs text-wool-500">
                        {messages().length} message{messages().length !== 1 ? 's' : ''}
                      </p>
                    </div>
                  </div>
                </div>
              </Show>

              {/* Messages list */}
              <div class="flex-1 overflow-auto">
                <div class="p-4 space-y-3 min-h-full flex flex-col">
                  {/* Spacer to push messages down */}
                  <div class="flex-1" />

                  <For each={messages()}>
                    {(msg) => {
                      const isHuman = () => msg.sender === 'human';
                      const isSystem = () => msg.sender === 'System' || msg.sender === 'system';
                      const senderWorker = () => workers().find((w) => w.name === msg.sender);

                      return (
                        <div
                          class={`flex gap-3 ${isHuman() ? 'flex-row-reverse' : ''}`}
                          classList={{ 'justify-center': isSystem() }}
                        >
                          {/* Avatar */}
                          <Show when={!isSystem()}>
                            <div class="shrink-0 pt-0.5">
                              <Show
                                when={senderWorker()}
                                fallback={
                                  <div class={`w-8 h-8 rounded-full flex items-center justify-center ${
                                    isHuman() ? 'bg-amber-500/20' : 'bg-pasture-600'
                                  }`}>
                                    <Show when={isHuman()}>
                                      <svg class="w-4 h-4 text-amber-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z" />
                                      </svg>
                                    </Show>
                                    <Show when={!isHuman()}>
                                      <svg class="w-4 h-4 text-wool-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
                                      </svg>
                                    </Show>
                                  </div>
                                }
                              >
                                <div
                                  innerHTML={generateSheepSvg(senderWorker()!.sheepConfig, 32)}
                                  class="w-8 h-8"
                                />
                              </Show>
                            </div>
                          </Show>

                          {/* Message bubble */}
                          <div
                            class={`max-w-[70%] ${
                              isSystem()
                                ? 'px-3 py-1.5 text-xs text-wool-500 bg-pasture-700/50 rounded-full'
                                : isHuman()
                                  ? 'px-4 py-2.5 rounded-2xl rounded-tr-sm bg-amber-500/15 border border-amber-500/20'
                                  : 'px-4 py-2.5 rounded-2xl rounded-tl-sm bg-pasture-700'
                            }`}
                          >
                            <Show when={!isSystem()}>
                              <div class="flex items-center gap-2 mb-1">
                                <span class={`text-xs font-semibold ${isHuman() ? 'text-amber-400' : 'text-wool-300'}`}>
                                  {msg.sender}
                                </span>
                                <span class="text-[10px] text-wool-600">
                                  {formatRelativeTime(msg.timestamp)}
                                </span>
                              </div>
                            </Show>
                            <p class={`whitespace-pre-wrap ${isSystem() ? '' : 'text-sm text-wool-200'}`}>
                              {msg.content}
                            </p>
                          </div>
                        </div>
                      );
                    }}
                  </For>

                  {/* Empty state */}
                  <Show when={messages().length === 0 && selection.selectedThread()}>
                    <div class="flex-1 flex flex-col items-center justify-center text-center py-12">
                      <div class="w-16 h-16 rounded-full bg-pasture-700 flex items-center justify-center mb-4">
                        <svg class="w-8 h-8 text-wool-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />
                        </svg>
                      </div>
                      <p class="text-sm text-wool-400">No messages yet</p>
                      <p class="text-xs text-wool-600 mt-1">Start the conversation below</p>
                    </div>
                  </Show>
                </div>
              </div>

              {/* Message input */}
              <Show when={selection.selectedThread()}>
                <div class="p-4 border-t border-pasture-600 bg-pasture-800/30 shrink-0">
                  <form class="form flex gap-3 items-end" onSubmit={(e) => { e.preventDefault(); sendMessage(); }}>
                    <div class="flex-1">
                      <input
                        type="text"
                        placeholder="Type a message..."
                        value={messageInput()}
                        onInput={(e) => setMessageInput(e.currentTarget.value)}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter' && !e.shiftKey) {
                            e.preventDefault();
                            sendMessage();
                          }
                        }}
                      />
                    </div>
                    <button
                      type="submit"
                      class="btn-icon"
                      disabled={!messageInput().trim() || sendingMessage()}
                    >
                      <Show when={sendingMessage()}>
                        <svg class="w-5 h-5 animate-spin" fill="none" viewBox="0 0 24 24">
                          <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" />
                          <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
                        </svg>
                      </Show>
                      <Show when={!sendingMessage()}>
                        <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 19l9 2-9-18-9 18 9-2zm0 0v-8" />
                        </svg>
                      </Show>
                    </button>
                  </form>
                </div>
              </Show>

              {/* No thread selected */}
              <Show when={!selection.selectedThread()}>
                <div class="flex-1 flex flex-col items-center justify-center text-center p-6">
                  <div class="w-20 h-20 rounded-full bg-pasture-700 flex items-center justify-center mb-4">
                    <svg class="w-10 h-10 text-wool-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />
                    </svg>
                  </div>
                  <p class="text-lg font-medium text-wool-300">Select a conversation</p>
                  <p class="text-sm text-wool-600 mt-1">Choose a channel or worker to start chatting</p>
                </div>
              </Show>
            </div>
          </div>
        </Show>

        {/* ==================== CONFIG TAB ==================== */}
        <Show when={selection.activeTab() === 'config'}>
          <div class="p-4 space-y-4 max-w-2xl">

            {/* Execution Card */}
            <div class="bg-pasture-800 border border-pasture-600 rounded-lg p-4">
              <h3 class="text-sm font-semibold text-wool-200 mb-4 flex items-center gap-2">
                <Icon name="cpu" class="w-4 h-4 text-amber-500/70" />
                Execution
              </h3>

              <div class="space-y-0">
                <ConfigRow label="Runner" value={detail()?.runner || 'Local'} />
                <ConfigRow label="Workers" value={String(detail()?.workerScale || '1')} />
                <ConfigRow label="Agent">
                  <span class="px-2 py-0.5 text-xs rounded bg-amber-500/15 text-amber-400 font-medium">
                    {detail()?.agentType || 'claude'}
                  </span>
                </ConfigRow>
                <ConfigRow label="Time Limit">
                  <Show when={detail()?.timeLimitMinutes} fallback={
                    <span class="text-xs text-wool-500 flex items-center gap-1">
                      <Icon name="infinity" class="w-3 h-3" /> Unlimited
                    </span>
                  }>
                    <span class="text-wool-100 font-medium tabular-nums">
                      {detail()?.timeLimitMinutes} min
                    </span>
                  </Show>
                </ConfigRow>
                <ConfigRow label="Human in Loop">
                  <span class={`px-2 py-0.5 text-xs rounded font-medium ${
                    detail()?.humanInTheLoop
                      ? 'bg-sage/20 text-sage'
                      : 'bg-wool-500/20 text-wool-400'
                  }`}>
                    {detail()?.humanInTheLoop ? 'Enabled' : 'Disabled'}
                  </span>
                </ConfigRow>
              </div>
            </div>

            {/* Repository Card */}
            <div class="bg-pasture-800 border border-pasture-600 rounded-lg p-4">
              <h3 class="text-sm font-semibold text-wool-200 mb-4 flex items-center gap-2">
                <Icon name="git-branch" class="w-4 h-4 text-amber-500/70" />
                Repository
              </h3>

              <div class="space-y-0">
                <ConfigRow label="Workspace">
                  <code class="font-mono text-xs text-wool-300 bg-pasture-700 px-2 py-1 rounded truncate max-w-[240px] inline-block" title={detail()?.projectPath ?? undefined}>
                    {detail()?.projectPath || '(none)'}
                  </code>
                </ConfigRow>
                <ConfigRow label="Branch">
                  <span class="flex items-center gap-1.5">
                    <Icon name="git-branch" class="w-3 h-3 text-wool-500" />
                    <span class="text-wool-100 font-medium">{detail()?.branch || 'main'}</span>
                  </span>
                </ConfigRow>
                <Show when={detail()?.remoteUrl}>
                  <ConfigRow label="Remote">
                    <code class="font-mono text-xs text-wool-300 bg-pasture-700 px-2 py-1 rounded truncate max-w-[240px] inline-block" title={detail()?.remoteUrl ?? undefined}>
                      {detail()?.remoteUrl}
                    </code>
                  </ConfigRow>
                </Show>
              </div>
            </div>

          </div>
        </Show>
      </div>

      {/* ========== MODALS ========== */}

      {/* Worker Detail Modal */}
      <Show when={showWorkerModal() && selectedWorker()}>
        <WorkerDetailModal
          worker={selectedWorker()!}
          metricsAvailable={detail()?.metricsAvailable || false}
          runName={runName()!}
          onClose={() => setShowWorkerModal(false)}
          onAttach={() => {
            openWorkerOutput(selectedWorker()!);
            setShowWorkerModal(false);
          }}
        />
      </Show>

      {/* Task Detail Modal */}
      <Show when={showTaskModal() && selectedTask()}>
        <TaskDetailModal
          task={selectedTask()!}
          allTasks={tasks()}
          workers={workers()}
          onClose={() => setShowTaskModal(false)}
          onComplete={() => {
            completeTask(selectedTask()!.id);
            setShowTaskModal(false);
          }}
          onReopen={() => {
            reopenTask(selectedTask()!.id);
            setShowTaskModal(false);
          }}
          onUnclaim={() => {
            unclaimTask(selectedTask()!.id);
            setShowTaskModal(false);
          }}
          onDelete={() => deleteTask(selectedTask()!.id)}
          onTaskClick={(taskId) => {
            setSelectedTaskId(taskId);
            setShowTaskModal(true);
          }}
        />
      </Show>

      {/* Deliver Modal */}
      <Show when={showDeliverModal()}>
        <div
          class="fixed inset-0 z-50 flex items-center justify-center bg-black/80"
          onClick={(e) => {
            if (e.target === e.currentTarget) setShowDeliverModal(false);
          }}
        >
          <div class="bg-pasture-800 border border-pasture-600 rounded-lg shadow-xl w-full max-w-md">
            <div class="p-4 border-b border-pasture-600">
              <h3 class="text-lg font-medium text-wool-100">Deliver Run</h3>
            </div>
            <div class="p-4 space-y-4">
              <div class="grid gap-2">
                <label for="deliver-branch" class="text-sm text-wool-300">
                  Branch Name (optional)
                </label>
                <input
                  id="deliver-branch"
                  type="text"
                  class="input"
                  placeholder={runName() || 'feature-branch'}
                  value={deliverBranch()}
                  onInput={(e) => setDeliverBranch(e.currentTarget.value)}
                />
                <p class="text-xs text-wool-500">Leave empty to use the run name as the branch</p>
              </div>
            </div>
            <div class="p-4 border-t border-pasture-600 flex justify-end gap-2">
              <button
                class="btn-ghost"
                onClick={() => setShowDeliverModal(false)}
                disabled={delivering()}
              >
                Cancel
              </button>
              <button class="btn" onClick={deliverRun} disabled={delivering()}>
                <Show when={delivering()}>
                  <span class="spinner w-4 h-4" />
                </Show>
                <Show when={!delivering()}>
                  <Icon name="git-pull-request" class="w-4 h-4" />
                </Show>
                Deliver
              </button>
            </div>
          </div>
        </div>
      </Show>
    </div>
  );
};
