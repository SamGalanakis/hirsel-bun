/**
 * Node Renderer - LOAD-aware rendering for board items
 *
 * Renders tasks and evals at two levels of detail based on zoom:
 * - compact: Name + status badges (140x48)
 * - full: Complete card with content (280x180)
 *
 * Follows Highland Craft design language - warm, crafted, functional.
 */

import type { Component } from 'solid-js';
import { Match, Show, Switch } from 'solid-js';
import type { TaskTree, BoardEval, BoardTaskStatus, BoardEvalStatus } from '../../lib/types';
import type { LOADLevel, NodePosition } from './use-tree-layout';
import { getCounterScale, MIN_SCREEN_SIZE, BASE_WORLD_SIZE } from './use-tree-layout';

// =============================================================================
// Task Card Components
// =============================================================================

/** Active run info to display as a badge on task cards */
interface ActiveRunInfo {
  name: string;
  status: string;
}

interface TaskCardProps {
  task: TaskTree;
  position: NodePosition;
  load: LOADLevel;
  selected: boolean;
  editing: boolean;
  zoom: number;
  inDispatchScope?: boolean;
  isDispatchRoot?: boolean;
  activeRun?: ActiveRunInfo | null;
  onClick: (e: MouseEvent) => void;
  onDoubleClick: () => void;
  onContextMenu: (e: MouseEvent) => void;
  onAskGyp: (e: MouseEvent) => void;
  onDragStart?: (e: MouseEvent, nodeId: string) => void;
}

/** Get task status color - follows Highland Craft status indicators */
const getTaskStatusColor = (status: BoardTaskStatus): string => {
  switch (status) {
    case 'todo':
      return 'bg-wool-600'; // Idle - muted wool
    case 'doing':
      return 'bg-amber-500'; // Working - shepherd's lantern
    case 'done':
      return 'bg-sage'; // Success - highland sage
    case 'blocked':
      return 'bg-terra'; // Error - Scottish earth
  }
};

/** Get task status badge styling */
const getTaskStatusBadge = (status: BoardTaskStatus): string => {
  switch (status) {
    case 'todo':
      return 'bg-pasture-700 text-wool-500';
    case 'doing':
      return 'bg-amber-500/20 text-amber-400';
    case 'done':
      return 'bg-sage/20 text-sage';
    case 'blocked':
      return 'bg-terra/20 text-terra';
  }
};

/** Get run badge color based on status */
const getRunBadgeColor = (status: string): string => {
  switch (status) {
    case 'working':
    case 'eval':
      return 'bg-amber-500';
    case 'paused':
      return 'bg-wool-500';
    case 'done':
    case 'delivered':
    case 'merged':
      return 'bg-sage';
    case 'failed':
      return 'bg-terra';
    default:
      return 'bg-wool-600';
  }
};

/** Compact view - same size as full, just big centered name */
const TaskCompact: Component<{
  task: TaskTree;
  selected: boolean;
  inDispatchScope?: boolean;
  isDispatchRoot?: boolean;
  activeRun?: ActiveRunInfo | null;
  onClick: (e: MouseEvent) => void;
  onContextMenu: (e: MouseEvent) => void;
  onDragStart?: (e: MouseEvent, nodeId: string) => void;
}> = (props) => {
  const isWorking = () => props.task.status === 'doing';

  // Adaptive font size based on name length
  const fontSize = () => {
    const len = props.task.name.length;
    if (len <= 10) return '32px';
    if (len <= 20) return '26px';
    if (len <= 30) return '22px';
    return '18px';
  };

  // Dispatch scope styling
  const getBorder = () => {
    if (props.isDispatchRoot) return '2px solid rgba(245, 158, 11, 0.7)';
    if (props.inDispatchScope) return '2px dashed rgba(245, 158, 11, 0.5)';
    if (props.selected) return '1px solid rgba(212, 165, 116, 0.4)';
    return '1px solid #3d3a36';
  };

  const getBoxShadow = () => {
    if (props.isDispatchRoot) return '0 0 24px rgba(245, 158, 11, 0.3), 0 4px 16px rgba(0, 0, 0, 0.4)';
    if (props.inDispatchScope) return '0 0 16px rgba(245, 158, 11, 0.15), 0 2px 8px rgba(0, 0, 0, 0.3)';
    if (props.selected) return '0 4px 16px rgba(0, 0, 0, 0.4), 0 0 20px rgba(212, 165, 116, 0.1)';
    return '0 2px 8px rgba(0, 0, 0, 0.3)';
  };

  return (
    <div
      class="w-[280px] h-[120px] rounded-lg select-none flex flex-col items-center justify-center relative"
      classList={{
        'ring-2 ring-amber-500/50': props.selected && !props.inDispatchScope,
      }}
      style={{
        background: 'linear-gradient(180deg, #2a2825 0%, #1f1d1a 100%)',
        border: getBorder(),
        'box-shadow': getBoxShadow(),
        cursor: 'grab',
      }}
      onClick={props.onClick}
      onContextMenu={props.onContextMenu}
      onMouseDown={(e) => {
        if (e.button === 0 && props.onDragStart) {
          e.stopPropagation();
          props.onDragStart(e, props.task.id);
        }
      }}
    >
      {/* Active run badge */}
      <Show when={props.activeRun}>
        <div
          class={`absolute -top-1.5 -right-1.5 w-4 h-4 rounded-full ${getRunBadgeColor(props.activeRun!.status)}`}
          classList={{
            'animate-pulse': props.activeRun!.status === 'working' || props.activeRun!.status === 'eval',
          }}
          style={{
            'box-shadow': '0 2px 6px rgba(0, 0, 0, 0.4)',
          }}
          title={`Run: ${props.activeRun!.name} (${props.activeRun!.status})`}
        />
      </Show>
      {/* Status dot */}
      <span
        class={`w-3 h-3 rounded-full mb-2 ${getTaskStatusColor(props.task.status)}`}
        classList={{
          'pulse-glow': isWorking(),
        }}
      />
      {/* Big centered name - adaptive font size */}
      <span
        class="text-wool-100 font-semibold text-center px-4 leading-tight line-clamp-2"
        style={{ 'font-size': fontSize() }}
      >
        {props.task.name}
      </span>
      {/* Subtle children indicator */}
      <Show when={props.task.children.length > 0}>
        <span class="text-[11px] text-wool-600 mt-2 tabular-nums">
          +{props.task.children.length}
        </span>
      </Show>
    </div>
  );
};

/** Full view - complete task card */
const TaskFull: Component<{
  task: TaskTree;
  selected: boolean;
  editing: boolean;
  inDispatchScope?: boolean;
  isDispatchRoot?: boolean;
  activeRun?: ActiveRunInfo | null;
  onClick: (e: MouseEvent) => void;
  onDoubleClick: () => void;
  onContextMenu: (e: MouseEvent) => void;
  onAskGyp: (e: MouseEvent) => void;
  onDragStart?: (e: MouseEvent, nodeId: string) => void;
}> = (props) => {
  const isWorking = () => props.task.status === 'doing';

  // Dispatch scope styling
  const getBorder = () => {
    if (props.isDispatchRoot) return '2px solid rgba(245, 158, 11, 0.7)';
    if (props.inDispatchScope) return '2px dashed rgba(245, 158, 11, 0.5)';
    if (props.selected) return '1px solid rgba(212, 165, 116, 0.4)';
    return '1px solid #3d3a36';
  };

  const getBoxShadow = () => {
    if (props.isDispatchRoot) return '0 0 24px rgba(245, 158, 11, 0.3), 0 8px 32px rgba(0, 0, 0, 0.5)';
    if (props.inDispatchScope) return '0 0 16px rgba(245, 158, 11, 0.15), 0 4px 16px rgba(0, 0, 0, 0.4)';
    if (props.selected) return '0 8px 32px rgba(0, 0, 0, 0.5), 0 0 24px rgba(212, 165, 116, 0.12)';
    return '0 4px 16px rgba(0, 0, 0, 0.4), inset 0 1px 0 rgba(255, 255, 255, 0.02)';
  };

  return (
    <div
      class="w-[280px] rounded-lg select-none group relative"
      classList={{
        'ring-2 ring-amber-500/50': props.selected && !props.inDispatchScope,
        'gyp-editing-shimmer': props.editing,
      }}
      style={{
        background: 'linear-gradient(180deg, #2a2825 0%, #1f1d1a 100%)',
        border: getBorder(),
        'box-shadow': getBoxShadow(),
        cursor: 'pointer',
      }}
      onClick={props.onClick}
      onDblClick={props.onDoubleClick}
      onContextMenu={props.onContextMenu}
    >
      {/* Active run badge */}
      <Show when={props.activeRun}>
        <div
          class={`absolute -top-1.5 -right-1.5 w-4 h-4 rounded-full ${getRunBadgeColor(props.activeRun!.status)}`}
          classList={{
            'animate-pulse': props.activeRun!.status === 'working' || props.activeRun!.status === 'eval',
          }}
          style={{
            'box-shadow': '0 2px 6px rgba(0, 0, 0, 0.4)',
            'z-index': 10,
          }}
          title={`Run: ${props.activeRun!.name} (${props.activeRun!.status})`}
        />
      </Show>
      {/* Header - draggable */}
      <div
        class="flex items-center justify-between px-3 py-2.5"
        style={{
          'border-bottom': '1px solid rgba(61, 58, 54, 0.6)',
          background: 'linear-gradient(180deg, rgba(255, 255, 255, 0.02) 0%, transparent 100%)',
          'border-radius': '8px 8px 0 0',
          cursor: 'grab',
        }}
        onMouseDown={(e) => {
          if (e.button === 0 && props.onDragStart) {
            e.stopPropagation();
            props.onDragStart(e, props.task.id);
          }
        }}
      >
        <div class="flex items-center gap-2.5 min-w-0">
          <span
            class={`w-2.5 h-2.5 rounded-full shrink-0 ${getTaskStatusColor(props.task.status)}`}
            classList={{
              'pulse-glow': isWorking(),
            }}
          />
          <span class="text-[15px] font-semibold text-wool-100 truncate leading-snug">
            {props.task.name}
          </span>
        </div>
        <div class="flex items-center gap-1.5">
          <Show when={props.task.validated}>
            <span class="text-[10px] px-1.5 py-0.5 rounded-full font-medium bg-sage/15 text-sage border border-sage/20">
              validated
            </span>
          </Show>
          <button
            onClick={(e) => {
              e.stopPropagation();
              props.onAskGyp(e);
            }}
            class="p-1.5 rounded-md text-amber-500/60 hover:text-amber-400 hover:bg-amber-500/10 opacity-0 group-hover:opacity-100 transition-all"
            title="Ask Gyp"
          >
            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M5 3v4M3 5h4M6 17v4m-2-2h4m5-16l2.286 6.857L21 12l-5.714 2.143L13 21l-2.286-6.857L5 12l5.714-2.143L13 3z"
              />
            </svg>
          </button>
        </div>
      </div>

      {/* Content */}
      <div class="px-3 py-2.5">
        <div class="flex items-center justify-between mb-1.5">
          <span class="text-[10px] font-semibold uppercase tracking-wider text-wool-500">
            Task
          </span>
          <span
            class={`text-[10px] px-1.5 py-0.5 rounded font-medium uppercase ${getTaskStatusBadge(props.task.status)}`}
          >
            {props.task.status}
          </span>
        </div>
        <div class="text-[13px] text-wool-300 line-clamp-3 leading-relaxed min-h-[48px]">
          {props.task.content || (
            <span class="text-wool-600 italic">No description yet</span>
          )}
        </div>
      </div>

      {/* Children indicator */}
      <Show when={props.task.children.length > 0}>
        <div
          class="px-3 py-2 flex items-center gap-1.5 text-wool-500"
          style={{
            'border-top': '1px solid rgba(61, 58, 54, 0.4)',
            background: 'rgba(0, 0, 0, 0.15)',
            'border-radius': '0 0 8px 8px',
          }}
        >
          <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M19 9l-7 7-7-7"
            />
          </svg>
          <span class="text-[11px] tabular-nums">
            {props.task.children.length} {props.task.children.length === 1 ? 'subtask' : 'subtasks'}
          </span>
        </div>
      </Show>
    </div>
  );
};

/** Main task card renderer with LOAD switching */
export const TaskCard: Component<TaskCardProps> = (props) => {
  const counterScale = () => getCounterScale(
    BASE_WORLD_SIZE.task,
    MIN_SCREEN_SIZE.task,
    props.zoom
  );

  return (
    <div
      class="absolute"
      style={{
        left: `${props.position.x}px`,
        top: `${props.position.y}px`,
        transform: `translate(-50%, -50%) scale(${counterScale()})`,
        'transform-origin': 'center center',
        'z-index': props.selected ? 10 : 1,
      }}
    >
      <Switch>
        <Match when={props.load === 'compact'}>
          <TaskCompact
            task={props.task}
            selected={props.selected}
            inDispatchScope={props.inDispatchScope}
            isDispatchRoot={props.isDispatchRoot}
            activeRun={props.activeRun}
            onClick={props.onClick}
            onContextMenu={props.onContextMenu}
            onDragStart={props.onDragStart}
          />
        </Match>
        <Match when={props.load === 'full'}>
          <TaskFull
            task={props.task}
            selected={props.selected}
            editing={props.editing}
            inDispatchScope={props.inDispatchScope}
            isDispatchRoot={props.isDispatchRoot}
            activeRun={props.activeRun}
            onClick={props.onClick}
            onDoubleClick={props.onDoubleClick}
            onContextMenu={props.onContextMenu}
            onAskGyp={props.onAskGyp}
            onDragStart={props.onDragStart}
          />
        </Match>
      </Switch>
    </div>
  );
};

// =============================================================================
// Eval Card Components
// Uses sage (Highland sage) as the verification/eval accent color
// =============================================================================

interface EvalCardProps {
  eval: BoardEval;
  position: NodePosition;
  load: LOADLevel;
  selected: boolean;
  zoom: number;
  inDispatchScope?: boolean;
  onClick: (e: MouseEvent) => void;
  onDoubleClick: () => void;
  onContextMenu: (e: MouseEvent) => void;
  onDragStart?: (e: MouseEvent, evalId: string) => void;
}

/** Get eval status color - uses Highland Craft palette */
const getEvalStatusColor = (status: BoardEvalStatus): string => {
  switch (status) {
    case 'blocked':
      return 'bg-wool-600';
    case 'queued':
      return 'bg-golden'; // Wheat fields - waiting
    case 'in_progress':
      return 'bg-amber-500';
    case 'passed':
      return 'bg-sage';
    case 'failed':
      return 'bg-terra';
  }
};

/** Get eval status badge styling */
const getEvalStatusBadge = (status: BoardEvalStatus): string => {
  switch (status) {
    case 'blocked':
      return 'bg-pasture-700 text-wool-500';
    case 'queued':
      return 'bg-golden/20 text-golden';
    case 'in_progress':
      return 'bg-amber-500/20 text-amber-400';
    case 'passed':
      return 'bg-sage/20 text-sage';
    case 'failed':
      return 'bg-terra/20 text-terra';
  }
};

/** Compact view for eval - same size as full, just big centered name */
const EvalCompact: Component<{
  eval: BoardEval;
  selected: boolean;
  inDispatchScope?: boolean;
  onClick: (e: MouseEvent) => void;
  onContextMenu: (e: MouseEvent) => void;
  onDragStart?: (e: MouseEvent, evalId: string) => void;
}> = (props) => {
  const isWorking = () => props.eval.status === 'in_progress';

  // Adaptive font size based on name length
  const fontSize = () => {
    const len = props.eval.name.length;
    if (len <= 10) return '28px';
    if (len <= 20) return '24px';
    if (len <= 30) return '20px';
    return '16px';
  };

  // Dispatch scope styling for evals (dashed amber, they're always propagated not roots)
  const getBorder = () => {
    if (props.inDispatchScope) return '2px dashed rgba(245, 158, 11, 0.5)';
    if (props.selected) return '1px solid rgba(125, 153, 112, 0.4)';
    return '1px solid rgba(125, 153, 112, 0.2)';
  };

  const getBoxShadow = () => {
    if (props.inDispatchScope) return '0 0 16px rgba(245, 158, 11, 0.15), 0 2px 8px rgba(0, 0, 0, 0.3)';
    if (props.selected) return '0 4px 16px rgba(0, 0, 0, 0.4), 0 0 16px rgba(125, 153, 112, 0.1)';
    return '0 2px 8px rgba(0, 0, 0, 0.3)';
  };

  return (
    <div
      class="w-[260px] h-[100px] rounded-lg select-none flex flex-col items-center justify-center"
      classList={{
        'ring-2 ring-sage/50': props.selected && !props.inDispatchScope,
      }}
      style={{
        background: 'linear-gradient(180deg, rgba(125, 153, 112, 0.08) 0%, rgba(31, 29, 26, 0.95) 100%)',
        border: getBorder(),
        'box-shadow': getBoxShadow(),
        cursor: 'grab',
      }}
      onClick={props.onClick}
      onContextMenu={props.onContextMenu}
      onMouseDown={(e) => {
        if (e.button === 0 && props.onDragStart) {
          e.stopPropagation();
          props.onDragStart(e, props.eval.id);
        }
      }}
    >
      {/* Status indicator with check icon */}
      <div class="flex items-center gap-2 mb-2">
        <svg class="w-5 h-5 text-sage" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
        <span
          class={`w-2.5 h-2.5 rounded-sm ${getEvalStatusColor(props.eval.status)}`}
          classList={{
            'pulse-glow-sage': isWorking(),
          }}
        />
      </div>
      {/* Big centered name - adaptive font size */}
      <span
        class="text-sage-light font-semibold text-center px-4 leading-tight line-clamp-2"
        style={{ 'font-size': fontSize() }}
      >
        {props.eval.name}
      </span>
      {/* Subtle validates indicator */}
      <Show when={props.eval.validates.length > 0}>
        <span class="text-[11px] text-sage/50 mt-2 tabular-nums">
          {props.eval.validates.length} tasks
        </span>
      </Show>
    </div>
  );
};

/** Full view for eval */
const EvalFull: Component<{
  eval: BoardEval;
  selected: boolean;
  inDispatchScope?: boolean;
  onClick: (e: MouseEvent) => void;
  onDoubleClick: () => void;
  onContextMenu: (e: MouseEvent) => void;
  onDragStart?: (e: MouseEvent, evalId: string) => void;
}> = (props) => {
  const isWorking = () => props.eval.status === 'in_progress';

  // Dispatch scope styling for evals
  const getBorder = () => {
    if (props.inDispatchScope) return '2px dashed rgba(245, 158, 11, 0.5)';
    if (props.selected) return '1px solid rgba(125, 153, 112, 0.4)';
    return '1px solid rgba(125, 153, 112, 0.2)';
  };

  const getBoxShadow = () => {
    if (props.inDispatchScope) return '0 0 16px rgba(245, 158, 11, 0.15), 0 4px 16px rgba(0, 0, 0, 0.4)';
    if (props.selected) return '0 8px 32px rgba(0, 0, 0, 0.5), 0 0 20px rgba(125, 153, 112, 0.1)';
    return '0 4px 16px rgba(0, 0, 0, 0.4)';
  };

  return (
    <div
      class="w-[260px] rounded-lg select-none group"
      classList={{
        'ring-2 ring-sage/50': props.selected && !props.inDispatchScope,
      }}
      style={{
        background: 'linear-gradient(180deg, rgba(125, 153, 112, 0.1) 0%, rgba(31, 29, 26, 0.98) 100%)',
        border: getBorder(),
        'box-shadow': getBoxShadow(),
        cursor: 'pointer',
      }}
      onClick={props.onClick}
      onDblClick={props.onDoubleClick}
      onContextMenu={props.onContextMenu}
    >
      {/* Header - draggable */}
      <div
        class="flex items-center justify-between px-3 py-2.5"
        style={{
          'border-bottom': '1px solid rgba(125, 153, 112, 0.2)',
          background: 'linear-gradient(180deg, rgba(125, 153, 112, 0.05) 0%, transparent 100%)',
          'border-radius': '8px 8px 0 0',
          cursor: 'grab',
        }}
        onMouseDown={(e) => {
          if (e.button === 0 && props.onDragStart) {
            e.stopPropagation();
            props.onDragStart(e, props.eval.id);
          }
        }}
      >
        <div class="flex items-center gap-2.5 min-w-0">
          <svg class="w-4 h-4 text-sage shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
          <span class="text-[15px] font-semibold text-sage-light truncate leading-snug">
            {props.eval.name}
          </span>
        </div>
        <span
          class={`text-[10px] px-1.5 py-0.5 rounded font-medium uppercase ${getEvalStatusBadge(props.eval.status)}`}
          classList={{
            'pulse-glow-sage': isWorking(),
          }}
        >
          {props.eval.status.replace('_', ' ')}
        </span>
      </div>

      {/* Content */}
      <div class="px-3 py-2.5">
        <span class="text-[10px] font-semibold uppercase tracking-wider text-sage/60 block mb-1.5">
          Verification
        </span>
        <div class="text-[13px] text-wool-300 line-clamp-2 leading-relaxed min-h-[40px]">
          {props.eval.content || (
            <span class="text-wool-600 italic">No criteria defined</span>
          )}
        </div>
      </div>

      {/* Validates indicator */}
      <Show when={props.eval.validates.length > 0}>
        <div
          class="px-3 py-2 flex items-center gap-1.5 text-sage/70"
          style={{
            'border-top': '1px solid rgba(125, 153, 112, 0.15)',
            background: 'rgba(125, 153, 112, 0.03)',
            'border-radius': '0 0 8px 8px',
          }}
        >
          <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.828 10.172a4 4 0 00-5.656 0l-4 4a4 4 0 105.656 5.656l1.102-1.101m-.758-4.899a4 4 0 005.656 0l4-4a4 4 0 00-5.656-5.656l-1.1 1.1" />
          </svg>
          <span class="text-[11px] tabular-nums">
            validates {props.eval.validates.length} {props.eval.validates.length === 1 ? 'task' : 'tasks'}
          </span>
        </div>
      </Show>
    </div>
  );
};

/** Main eval card renderer with LOAD switching */
export const EvalCard: Component<EvalCardProps> = (props) => {
  const counterScale = () => getCounterScale(
    BASE_WORLD_SIZE.task,
    MIN_SCREEN_SIZE.task,
    props.zoom
  );

  return (
    <div
      class="absolute"
      style={{
        left: `${props.position.x}px`,
        top: `${props.position.y}px`,
        transform: `translate(-50%, -50%) scale(${counterScale()})`,
        'transform-origin': 'center center',
        'z-index': props.selected ? 10 : 1,
      }}
    >
      <Switch>
        <Match when={props.load === 'compact'}>
          <EvalCompact
            eval={props.eval}
            selected={props.selected}
            inDispatchScope={props.inDispatchScope}
            onClick={props.onClick}
            onContextMenu={props.onContextMenu}
            onDragStart={props.onDragStart}
          />
        </Match>
        <Match when={props.load === 'full'}>
          <EvalFull
            eval={props.eval}
            selected={props.selected}
            inDispatchScope={props.inDispatchScope}
            onClick={props.onClick}
            onDoubleClick={props.onDoubleClick}
            onContextMenu={props.onContextMenu}
            onDragStart={props.onDragStart}
          />
        </Match>
      </Switch>
    </div>
  );
};
