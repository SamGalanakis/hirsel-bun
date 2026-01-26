/**
 * ProjectCard - LOAD-aware rendering for project nodes
 *
 * Renders projects at different levels of detail based on zoom:
 * - dot: Status dot only (24x24)
 * - compact: Name + status (140x60)
 * - full: Complete card with description (200x120)
 */

import type { Component } from 'solid-js';
import { Match, Show, Switch } from 'solid-js';
import type { Project } from '../../stores/project-context';

type ProjectLOADLevel = 'dot' | 'compact' | 'full';

interface ProjectCardProps {
  project: Project;
  position: { x: number; y: number };
  load: ProjectLOADLevel;
  selected: boolean;
  focused: boolean;
  runCount?: number;
  activeRunCount?: number;
  onClick: () => void;
  onDoubleClick: () => void;
  onContextMenu: (e: MouseEvent) => void;
  onDragStart?: (e: MouseEvent, projectId: number) => void;
}

/** Get project status color based on activity */
function getStatusColor(activeRunCount: number): string {
  if (activeRunCount > 0) return 'bg-amber-500';
  return 'bg-wool-700';
}

/** Dot view - just a status circle */
const ProjectDot: Component<{
  project: Project;
  selected: boolean;
  activeRunCount: number;
  onDragStart?: (e: MouseEvent, projectId: number) => void;
}> = (props) => {
  const statusColor = () => getStatusColor(props.activeRunCount);
  const isWorking = () => props.activeRunCount > 0;

  return (
    <div
      class="w-6 h-6 rounded-full flex items-center justify-center transition-all"
      classList={{
        'ring-2 ring-amber-500': props.selected,
        'pulse-glow': isWorking(),
      }}
      style={{
        background: 'rgba(36, 36, 36, 0.9)',
        border: '2px solid rgba(51, 51, 51, 1)',
        cursor: 'grab',
      }}
      onMouseDown={(e) => {
        if (e.button === 0 && props.onDragStart) {
          e.stopPropagation();
          props.onDragStart(e, props.project.id);
        }
      }}
    >
      <div
        class={`w-3 h-3 rounded-full ${statusColor()}`}
        style={{
          'box-shadow': isWorking() ? '0 0 8px rgba(212, 165, 116, 0.5)' : 'none',
        }}
      />
    </div>
  );
};

/** Compact view - name + status */
const ProjectCompact: Component<{
  project: Project;
  selected: boolean;
  runCount: number;
  activeRunCount: number;
  onClick: () => void;
  onContextMenu: (e: MouseEvent) => void;
  onDragStart?: (e: MouseEvent, projectId: number) => void;
}> = (props) => {
  return (
    <div
      class="w-[140px] p-2 rounded-lg transition-all select-none"
      classList={{
        'ring-2 ring-amber-500/60 shadow-[0_0_20px_rgba(212,165,116,0.15)]': props.selected,
      }}
      style={{
        background: 'linear-gradient(180deg, rgba(36, 36, 36, 0.98) 0%, rgba(26, 26, 26, 0.98) 100%)',
        border: props.selected
          ? '1px solid rgba(212, 165, 116, 0.4)'
          : '1px solid rgba(51, 51, 51, 0.8)',
        cursor: 'grab',
      }}
      onClick={props.onClick}
      onContextMenu={props.onContextMenu}
      onMouseDown={(e) => {
        if (e.button === 0 && props.onDragStart) {
          e.stopPropagation();
          props.onDragStart(e, props.project.id);
        }
      }}
    >
      <div class="flex items-center gap-2 mb-1">
        <span
          class={`w-2 h-2 rounded-full ${getStatusColor(props.activeRunCount)}`}
          classList={{
            'pulse-glow': props.activeRunCount > 0,
          }}
        />
        <span class="text-xs text-wool-100 truncate font-semibold">
          {props.project.name}
        </span>
      </div>
      <Show when={props.runCount > 0}>
        <div class="text-[11px] text-wool-400 pl-4">
          {props.runCount} run{props.runCount !== 1 ? 's' : ''}
        </div>
      </Show>
    </div>
  );
};

/** Full view - complete project card */
const ProjectFull: Component<{
  project: Project;
  selected: boolean;
  runCount: number;
  activeRunCount: number;
  onClick: () => void;
  onDoubleClick: () => void;
  onContextMenu: (e: MouseEvent) => void;
  onDragStart?: (e: MouseEvent, projectId: number) => void;
}> = (props) => {
  return (
    <div
      class="w-[200px] rounded-lg cursor-pointer transition-all select-none group hover:-translate-y-0.5"
      classList={{
        'ring-2 ring-amber-500/60 shadow-[0_0_30px_rgba(212,165,116,0.2)]': props.selected,
      }}
      style={{
        background: 'linear-gradient(180deg, rgba(36, 36, 36, 0.98) 0%, rgba(26, 26, 26, 0.98) 100%)',
        border: props.selected
          ? '1px solid rgba(212, 165, 116, 0.4)'
          : '1px solid rgba(51, 51, 51, 0.8)',
        'box-shadow': '0 4px 16px rgba(0, 0, 0, 0.4)',
      }}
      onClick={props.onClick}
      onDblClick={props.onDoubleClick}
      onContextMenu={props.onContextMenu}
    >
      {/* Header - draggable */}
      <div
        class="flex items-center justify-between px-3 py-2.5"
        style={{
          'border-bottom': '1px solid rgba(51, 51, 51, 0.5)',
          cursor: 'grab',
        }}
        onMouseDown={(e) => {
          if (e.button === 0 && props.onDragStart) {
            e.stopPropagation();
            props.onDragStart(e, props.project.id);
          }
        }}
      >
        <div class="flex items-center gap-2 min-w-0">
          <span
            class={`w-2.5 h-2.5 rounded-full shrink-0 ${getStatusColor(props.activeRunCount)}`}
            classList={{
              'pulse-glow': props.activeRunCount > 0,
            }}
          />
          <span class="text-sm font-semibold text-wool-100 truncate">
            {props.project.name}
          </span>
        </div>
        <button
          onClick={(e) => {
            e.stopPropagation();
            // Open settings
          }}
          class="p-1 rounded opacity-0 group-hover:opacity-100 text-wool-400 hover:text-wool-200 hover:bg-pasture-700 transition-all"
        >
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
          </svg>
        </button>
      </div>

      {/* Description */}
      <div class="px-3 py-2">
        <div class="text-xs text-wool-300 line-clamp-2 leading-relaxed min-h-[32px]">
          {props.project.description || (
            <span class="text-wool-500 italic">No description</span>
          )}
        </div>
      </div>

      {/* Footer with progress/status */}
      <div
        class="px-3 py-2 flex items-center justify-between"
        style={{
          'border-top': '1px solid rgba(51, 51, 51, 0.4)',
          background: 'rgba(0, 0, 0, 0.15)',
          'border-radius': '0 0 8px 8px',
        }}
      >
        <Show when={props.runCount > 0}>
          <div class="flex items-center gap-2">
            <div class="w-16 h-1.5 rounded-full bg-pasture-600 overflow-hidden">
              <div
                class="h-full bg-amber-500 rounded-full transition-all"
                style={{ width: `${Math.min(100, (props.activeRunCount / props.runCount) * 100)}%` }}
              />
            </div>
            <span class="text-[11px] text-wool-400">
              {props.activeRunCount} active
            </span>
          </div>
        </Show>
        <Show when={props.runCount === 0}>
          <span class="text-[11px] text-wool-500">No runs</span>
        </Show>
      </div>
    </div>
  );
};

/** Main project card renderer with LOAD switching */
export const ProjectCard: Component<ProjectCardProps> = (props) => {
  return (
    <div
      class="absolute transition-opacity duration-200"
      classList={{
        'opacity-30': props.focused && !props.selected,
      }}
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
            <ProjectDot
              project={props.project}
              selected={props.selected}
              activeRunCount={props.activeRunCount || 0}
              onDragStart={props.onDragStart}
            />
          </div>
        </Match>
        <Match when={props.load === 'compact'}>
          <ProjectCompact
            project={props.project}
            selected={props.selected}
            runCount={props.runCount || 0}
            activeRunCount={props.activeRunCount || 0}
            onClick={props.onClick}
            onContextMenu={props.onContextMenu}
            onDragStart={props.onDragStart}
          />
        </Match>
        <Match when={props.load === 'full'}>
          <ProjectFull
            project={props.project}
            selected={props.selected}
            runCount={props.runCount || 0}
            activeRunCount={props.activeRunCount || 0}
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

export default ProjectCard;
