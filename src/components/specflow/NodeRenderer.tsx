/**
 * Node Renderer - LOAD-aware rendering for board items
 *
 * Renders tasks and evals at different levels of detail based on zoom:
 * - dot: Status dot only (24x24)
 * - compact: Name + status badges (140x48)
 * - full: Complete card with content (280x180)
 */

import type { Component } from 'solid-js';
import { Match, Show, Switch } from 'solid-js';
import type { TaskTree, BoardEval, BoardTaskStatus, BoardEvalStatus } from '../../lib/types';
import { BOARD_TASK_COLORS, BOARD_EVAL_COLORS } from '../../lib/types';
import type { LOADLevel, NodePosition } from './use-tree-layout';

// =============================================================================
// Task Card Components
// =============================================================================

interface TaskCardProps {
  task: TaskTree;
  position: NodePosition;
  load: LOADLevel;
  selected: boolean;
  editing: boolean;
  onClick: () => void;
  onDoubleClick: () => void;
  onContextMenu: (e: MouseEvent) => void;
  onAskGyp: (e: MouseEvent) => void;
  onDragStart?: (e: MouseEvent, nodeId: string) => void;
}

/** Get task status color class */
const getTaskColor = (status: BoardTaskStatus): string => {
  switch (status) {
    case 'todo':
      return 'bg-wool-500';
    case 'doing':
      return 'bg-amber-500';
    case 'done':
      return 'bg-sage';
    case 'blocked':
      return 'bg-terra';
  }
};

/** Dot view - just a status circle */
const TaskDot: Component<{
  task: TaskTree;
  selected: boolean;
  onDragStart?: (e: MouseEvent, nodeId: string) => void;
}> = (props) => {
  const statusColor = () => {
    // Show validation status if validated
    if (props.task.validated) return 'bg-sage';
    return getTaskColor(props.task.status);
  };

  return (
    <div
      class="w-6 h-6 rounded-full flex items-center justify-center transition-all"
      classList={{
        'ring-2 ring-amber-500': props.selected,
      }}
      style={{
        background: 'rgba(39,39,42,0.9)',
        border: '1px solid rgba(63,63,70,0.6)',
        cursor: 'grab',
      }}
      onMouseDown={(e) => {
        if (e.button === 0 && props.onDragStart) {
          e.stopPropagation();
          props.onDragStart(e, props.task.id);
        }
      }}
    >
      <div class={`w-3 h-3 rounded-full ${statusColor()}`} />
    </div>
  );
};

/** Compact view - name + badges */
const TaskCompact: Component<{
  task: TaskTree;
  selected: boolean;
  onClick: () => void;
  onContextMenu: (e: MouseEvent) => void;
  onDragStart?: (e: MouseEvent, nodeId: string) => void;
}> = (props) => {
  return (
    <div
      class="w-[140px] p-2 rounded-lg transition-all select-none"
      classList={{
        'ring-2 ring-amber-500/60 shadow-[0_0_20px_rgba(212,165,116,0.15)]': props.selected,
      }}
      style={{
        background: 'linear-gradient(180deg, rgba(39,39,42,0.95) 0%, rgba(24,24,27,0.95) 100%)',
        border: props.selected
          ? '1px solid rgba(212,165,116,0.4)'
          : '1px solid rgba(63,63,70,0.6)',
        'box-shadow': '0 4px 12px rgba(0,0,0,0.3)',
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
      <div class="text-xs text-wool-200 truncate font-medium mb-1.5">
        {props.task.name}
      </div>
      <div class="flex items-center gap-1">
        <span
          class={`w-2 h-2 rounded-full ${getTaskColor(props.task.status)}`}
          title={`Status: ${props.task.status}`}
        />
        <Show when={props.task.validated}>
          <span class="w-2 h-2 rounded-full bg-sage" title="Validated" />
        </Show>
        <Show when={props.task.children.length > 0}>
          <span class="text-[10px] text-wool-600 ml-auto">
            {props.task.children.length}
          </span>
        </Show>
      </div>
    </div>
  );
};

/** Full view - complete task card */
const TaskFull: Component<{
  task: TaskTree;
  selected: boolean;
  editing: boolean;
  onClick: () => void;
  onDoubleClick: () => void;
  onContextMenu: (e: MouseEvent) => void;
  onAskGyp: (e: MouseEvent) => void;
  onDragStart?: (e: MouseEvent, nodeId: string) => void;
}> = (props) => {
  return (
    <div
      class="w-[280px] rounded-xl cursor-pointer transition-all select-none group"
      classList={{
        'ring-2 ring-amber-500/60 shadow-[0_0_30px_rgba(212,165,116,0.2)]': props.selected,
        'gyp-editing-shimmer': props.editing,
      }}
      style={{
        background: 'linear-gradient(180deg, rgba(39,39,42,0.98) 0%, rgba(24,24,27,0.98) 100%)',
        border: props.selected
          ? '1px solid rgba(212,165,116,0.4)'
          : '1px solid rgba(63,63,70,0.6)',
        'box-shadow': '0 8px 32px rgba(0,0,0,0.5), 0 2px 8px rgba(0,0,0,0.3), inset 0 1px 0 rgba(255,255,255,0.04)',
      }}
      onClick={props.onClick}
      onDblClick={props.onDoubleClick}
      onContextMenu={props.onContextMenu}
    >
      {/* Header - draggable */}
      <div
        class="flex items-center justify-between px-3 py-2.5"
        style={{
          'border-bottom': '1px solid rgba(63,63,70,0.4)',
          background: 'linear-gradient(180deg, rgba(255,255,255,0.03) 0%, transparent 100%)',
          'border-radius': '12px 12px 0 0',
          cursor: 'grab',
        }}
        onMouseDown={(e) => {
          if (e.button === 0 && props.onDragStart) {
            e.stopPropagation();
            props.onDragStart(e, props.task.id);
          }
        }}
      >
        <div class="flex items-center gap-2 min-w-0">
          <span
            class={`w-2.5 h-2.5 rounded-full shrink-0 ${getTaskColor(props.task.status)}`}
            style={{
              'box-shadow': props.task.status === 'doing'
                ? '0 0 8px rgba(251,191,36,0.5)'
                : 'none',
            }}
          />
          <span class="text-sm font-semibold text-wool-100 truncate tracking-tight">
            {props.task.name}
          </span>
        </div>
        <div class="flex items-center gap-1">
          <Show when={props.task.validated}>
            <span class="text-[10px] px-1.5 py-0.5 rounded font-medium uppercase bg-sage/20 text-sage">
              validated
            </span>
          </Show>
          <button
            onClick={(e) => {
              e.stopPropagation();
              props.onAskGyp(e);
            }}
            class="p-1.5 rounded-md transition-all text-amber-400/70 hover:text-amber-400 hover:bg-amber-500/15 opacity-0 group-hover:opacity-100"
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
      <div class="px-3 py-2" style={{ 'border-bottom': '1px solid rgba(63,63,70,0.3)' }}>
        <div class="flex items-center justify-between mb-1">
          <div class="text-[10px] font-bold uppercase tracking-widest text-amber-400/70">
            Task
          </div>
          <span
            class="text-[10px] px-1.5 py-0.5 rounded font-medium uppercase"
            classList={{
              'bg-wool-800 text-wool-500': props.task.status === 'todo',
              'bg-amber-500/20 text-amber-400': props.task.status === 'doing',
              'bg-sage/20 text-sage': props.task.status === 'done',
              'bg-terra/20 text-terra': props.task.status === 'blocked',
            }}
          >
            {props.task.status}
          </span>
        </div>
        <div class="text-xs text-wool-400 line-clamp-3 leading-relaxed min-h-[40px]">
          {props.task.content || <span class="text-wool-600 italic">No content</span>}
        </div>
      </div>

      {/* Children indicator */}
      <Show when={props.task.children.length > 0}>
        <div
          class="px-3 py-1.5 text-[10px] text-wool-600 flex items-center gap-1"
          style={{
            background: 'rgba(0,0,0,0.2)',
            'border-radius': '0 0 12px 12px',
          }}
        >
          <svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M19 9l-7 7-7-7"
            />
          </svg>
          {props.task.children.length} child{props.task.children.length !== 1 ? 'ren' : ''}
        </div>
      </Show>
    </div>
  );
};

/** Main task card renderer with LOAD switching */
export const TaskCard: Component<TaskCardProps> = (props) => {
  return (
    <div
      class="absolute"
      style={{
        left: `${props.position.x}px`,
        top: `${props.position.y}px`,
        transform: 'translate(-50%, -50%)',
        'z-index': props.selected ? 10 : 1,
      }}
    >
      <Switch>
        <Match when={props.load === 'dot'}>
          <div onClick={props.onClick} onContextMenu={props.onContextMenu}>
            <TaskDot
              task={props.task}
              selected={props.selected}
              onDragStart={props.onDragStart}
            />
          </div>
        </Match>
        <Match when={props.load === 'compact'}>
          <TaskCompact
            task={props.task}
            selected={props.selected}
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
// =============================================================================

interface EvalCardProps {
  eval: BoardEval;
  position: NodePosition;
  load: LOADLevel;
  selected: boolean;
  onClick: () => void;
  onDoubleClick: () => void;
  onContextMenu: (e: MouseEvent) => void;
  onDragStart?: (e: MouseEvent, evalId: string) => void;
}

/** Get eval status color class */
const getEvalColor = (status: BoardEvalStatus): string => {
  switch (status) {
    case 'blocked':
      return 'bg-wool-600';
    case 'queued':
      return 'bg-sky-500';
    case 'in_progress':
      return 'bg-amber-500';
    case 'passed':
      return 'bg-sage';
    case 'failed':
      return 'bg-terra';
  }
};

/** Dot view for eval */
const EvalDot: Component<{
  eval: BoardEval;
  selected: boolean;
  onDragStart?: (e: MouseEvent, evalId: string) => void;
}> = (props) => {
  return (
    <div
      class="w-6 h-6 rounded flex items-center justify-center transition-all"
      classList={{
        'ring-2 ring-emerald-500': props.selected,
      }}
      style={{
        background: 'rgba(16,185,129,0.1)',
        border: '1px solid rgba(16,185,129,0.3)',
        cursor: 'grab',
      }}
      onMouseDown={(e) => {
        if (e.button === 0 && props.onDragStart) {
          e.stopPropagation();
          props.onDragStart(e, props.eval.id);
        }
      }}
    >
      <div class={`w-3 h-3 rounded-sm ${getEvalColor(props.eval.status)}`} />
    </div>
  );
};

/** Compact view for eval */
const EvalCompact: Component<{
  eval: BoardEval;
  selected: boolean;
  onClick: () => void;
  onContextMenu: (e: MouseEvent) => void;
  onDragStart?: (e: MouseEvent, evalId: string) => void;
}> = (props) => {
  return (
    <div
      class="w-[140px] p-2 rounded-lg transition-all select-none"
      classList={{
        'ring-2 ring-emerald-500/60 shadow-[0_0_20px_rgba(16,185,129,0.15)]': props.selected,
      }}
      style={{
        background: 'linear-gradient(180deg, rgba(16,185,129,0.08) 0%, rgba(16,185,129,0.04) 100%)',
        border: props.selected
          ? '1px solid rgba(16,185,129,0.4)'
          : '1px solid rgba(16,185,129,0.2)',
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
      <div class="text-xs text-emerald-200 truncate font-medium mb-1.5">
        {props.eval.name}
      </div>
      <div class="flex items-center gap-1">
        <span
          class={`w-2 h-2 rounded-sm ${getEvalColor(props.eval.status)}`}
          title={`Status: ${props.eval.status}`}
        />
        <Show when={props.eval.validates.length > 0}>
          <span class="text-[10px] text-emerald-600 ml-auto">
            {props.eval.validates.length}
          </span>
        </Show>
      </div>
    </div>
  );
};

/** Full view for eval */
const EvalFull: Component<{
  eval: BoardEval;
  selected: boolean;
  onClick: () => void;
  onDoubleClick: () => void;
  onContextMenu: (e: MouseEvent) => void;
  onDragStart?: (e: MouseEvent, evalId: string) => void;
}> = (props) => {
  return (
    <div
      class="w-[260px] rounded-xl cursor-pointer transition-all select-none group"
      classList={{
        'ring-2 ring-emerald-500/60 shadow-[0_0_30px_rgba(16,185,129,0.2)]': props.selected,
      }}
      style={{
        background: 'linear-gradient(180deg, rgba(16,185,129,0.1) 0%, rgba(16,185,129,0.05) 100%)',
        border: props.selected
          ? '1px solid rgba(16,185,129,0.4)'
          : '1px solid rgba(16,185,129,0.2)',
        'box-shadow': '0 8px 32px rgba(0,0,0,0.4)',
      }}
      onClick={props.onClick}
      onDblClick={props.onDoubleClick}
      onContextMenu={props.onContextMenu}
    >
      {/* Header - draggable */}
      <div
        class="flex items-center justify-between px-3 py-2.5"
        style={{
          'border-bottom': '1px solid rgba(16,185,129,0.2)',
          'border-radius': '12px 12px 0 0',
          cursor: 'grab',
        }}
        onMouseDown={(e) => {
          if (e.button === 0 && props.onDragStart) {
            e.stopPropagation();
            props.onDragStart(e, props.eval.id);
          }
        }}
      >
        <div class="flex items-center gap-2 min-w-0">
          <svg class="w-4 h-4 text-emerald-400 shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
          <span class="text-sm font-semibold text-emerald-100 truncate tracking-tight">
            {props.eval.name}
          </span>
        </div>
        <span
          class="text-[10px] px-1.5 py-0.5 rounded font-medium uppercase"
          classList={{
            'bg-wool-700 text-wool-400': props.eval.status === 'blocked',
            'bg-sky-500/20 text-sky-400': props.eval.status === 'queued',
            'bg-amber-500/20 text-amber-400': props.eval.status === 'in_progress',
            'bg-sage/20 text-sage': props.eval.status === 'passed',
            'bg-terra/20 text-terra': props.eval.status === 'failed',
          }}
        >
          {props.eval.status.replace('_', ' ')}
        </span>
      </div>

      {/* Content */}
      <div class="px-3 py-2">
        <div class="text-[10px] font-bold uppercase tracking-widest text-emerald-400/70 mb-1">
          Verification
        </div>
        <div class="text-xs text-emerald-200/80 line-clamp-2 leading-relaxed min-h-[32px]">
          {props.eval.content || <span class="text-emerald-600 italic">No description</span>}
        </div>
      </div>

      {/* Validates indicator */}
      <Show when={props.eval.validates.length > 0}>
        <div
          class="px-3 py-1.5 text-[10px] text-emerald-500 flex items-center gap-1"
          style={{
            'border-top': '1px solid rgba(16,185,129,0.15)',
            'border-radius': '0 0 12px 12px',
          }}
        >
          <svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.828 10.172a4 4 0 00-5.656 0l-4 4a4 4 0 105.656 5.656l1.102-1.101m-.758-4.899a4 4 0 005.656 0l4-4a4 4 0 00-5.656-5.656l-1.1 1.1" />
          </svg>
          validates {props.eval.validates.length} task{props.eval.validates.length !== 1 ? 's' : ''}
        </div>
      </Show>
    </div>
  );
};

/** Main eval card renderer with LOAD switching */
export const EvalCard: Component<EvalCardProps> = (props) => {
  return (
    <div
      class="absolute"
      style={{
        left: `${props.position.x}px`,
        top: `${props.position.y}px`,
        transform: 'translate(-50%, -50%)',
        'z-index': props.selected ? 10 : 1,
      }}
    >
      <Switch>
        <Match when={props.load === 'dot'}>
          <div onClick={props.onClick} onContextMenu={props.onContextMenu}>
            <EvalDot
              eval={props.eval}
              selected={props.selected}
              onDragStart={props.onDragStart}
            />
          </div>
        </Match>
        <Match when={props.load === 'compact'}>
          <EvalCompact
            eval={props.eval}
            selected={props.selected}
            onClick={props.onClick}
            onContextMenu={props.onContextMenu}
            onDragStart={props.onDragStart}
          />
        </Match>
        <Match when={props.load === 'full'}>
          <EvalFull
            eval={props.eval}
            selected={props.selected}
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
