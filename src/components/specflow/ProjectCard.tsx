/**
 * ProjectCard - Renders project nodes with counter-scaling
 *
 * Always renders the full card view, using counter-scaling to maintain
 * readability at any zoom level.
 *
 * Follows Highland Craft design language - warm, crafted, functional.
 */

import type { Component } from 'solid-js';
import { Show } from 'solid-js';
import type { Project } from '../../stores/project-context';
import { getCounterScale, MIN_SCREEN_SIZE, BASE_WORLD_SIZE } from './use-tree-layout';

interface ProjectCardProps {
  project: Project;
  position: { x: number; y: number };
  selected: boolean;
  focused: boolean;
  runCount?: number;
  activeRunCount?: number;
  zoom: number;
  onClick: () => void;
  onDoubleClick: () => void;
  onContextMenu: (e: MouseEvent) => void;
  onSettings?: () => void;
  onDragStart?: (e: MouseEvent, projectId: number) => void;
  style?: Record<string, string | number>;
}

/** Get project status color - follows Highland Craft status indicators */
function getStatusColor(activeRunCount: number): string {
  if (activeRunCount > 0) return 'bg-amber-500'; // Working - shepherd's lantern
  return 'bg-wool-600'; // Idle
}

/** Full view - complete project card */
const ProjectFull: Component<{
  project: Project;
  selected: boolean;
  runCount: number;
  activeRunCount: number;
  onClick: () => void;
  onDoubleClick: () => void;
  onContextMenu: (e: MouseEvent) => void;
  onSettings: () => void;
  onDragStart?: (e: MouseEvent, projectId: number) => void;
}> = (props) => {
  const isWorking = () => props.activeRunCount > 0;

  return (
    <div
      class="w-[200px] rounded-lg select-none group"
      classList={{
        'ring-2 ring-amber-500/50': props.selected,
      }}
      style={{
        background: 'linear-gradient(180deg, #2a2825 0%, #1f1d1a 100%)',
        border: props.selected
          ? '1px solid rgba(212, 165, 116, 0.4)'
          : '1px solid #3d3a36',
        'box-shadow': props.selected
          ? '0 8px 32px rgba(0, 0, 0, 0.5), 0 0 24px rgba(212, 165, 116, 0.12)'
          : '0 4px 16px rgba(0, 0, 0, 0.4), inset 0 1px 0 rgba(255, 255, 255, 0.02)',
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
          'border-bottom': '1px solid rgba(61, 58, 54, 0.6)',
          background: 'linear-gradient(180deg, rgba(255, 255, 255, 0.02) 0%, transparent 100%)',
          'border-radius': '8px 8px 0 0',
          cursor: 'grab',
        }}
        onMouseDown={(e) => {
          if (e.button === 0 && props.onDragStart) {
            e.stopPropagation();
            props.onDragStart(e, props.project.id);
          }
        }}
      >
        <div class="flex items-center gap-2.5 min-w-0">
          <span
            class={`w-2.5 h-2.5 rounded-full shrink-0 ${getStatusColor(props.activeRunCount)}`}
            classList={{
              'pulse-glow': isWorking(),
            }}
          />
          <span class="text-[15px] font-semibold text-wool-100 truncate leading-snug">
            {props.project.name}
          </span>
        </div>
        <button
          onClick={(e) => {
            e.stopPropagation();
            props.onSettings();
          }}
          class="p-1 rounded-md text-wool-500 hover:text-wool-300 hover:bg-pasture-700 opacity-0 group-hover:opacity-100 transition-all"
          title="Project settings"
        >
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
          </svg>
        </button>
      </div>

      {/* Description */}
      <div class="px-3 py-2.5">
        <div class="text-[13px] text-wool-300 line-clamp-2 leading-relaxed min-h-[40px]">
          {props.project.description || (
            <span class="text-wool-600 italic">No description yet</span>
          )}
        </div>
      </div>

      {/* Footer with progress/status */}
      <div
        class="px-3 py-2 flex items-center justify-between"
        style={{
          'border-top': '1px solid rgba(61, 58, 54, 0.4)',
          background: 'rgba(0, 0, 0, 0.15)',
          'border-radius': '0 0 8px 8px',
        }}
      >
        <Show
          when={props.runCount > 0}
          fallback={
            <span class="text-[11px] text-wool-600 italic">No runs yet</span>
          }
        >
          <div class="flex items-center gap-2.5 w-full">
            {/* Progress bar */}
            <div class="flex-1 h-1.5 rounded-full bg-pasture-700 overflow-hidden">
              <div
                class="h-full rounded-full transition-all"
                classList={{
                  'bg-amber-500': props.activeRunCount > 0,
                  'bg-wool-600': props.activeRunCount === 0,
                }}
                style={{
                  width: `${Math.min(100, (props.activeRunCount / props.runCount) * 100)}%`,
                }}
              />
            </div>
            {/* Run count */}
            <span class="text-[11px] text-wool-400 tabular-nums shrink-0">
              <Show
                when={props.activeRunCount > 0}
                fallback={
                  <span class="text-wool-500">{props.runCount} run{props.runCount !== 1 ? 's' : ''}</span>
                }
              >
                <span class="text-amber-400">{props.activeRunCount}</span>
                <span class="text-wool-600"> / {props.runCount}</span>
              </Show>
            </span>
          </div>
        </Show>
      </div>
    </div>
  );
};

/** Main project card renderer with counter-scaling */
export const ProjectCard: Component<ProjectCardProps> = (props) => {
  const counterScale = () => getCounterScale(
    BASE_WORLD_SIZE.project,
    MIN_SCREEN_SIZE.project,
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
        transition: 'opacity 0.3s ease-out',
        ...props.style,
      }}
    >
      <ProjectFull
        project={props.project}
        selected={props.selected}
        runCount={props.runCount || 0}
        activeRunCount={props.activeRunCount || 0}
        onClick={props.onClick}
        onDoubleClick={props.onDoubleClick}
        onContextMenu={props.onContextMenu}
        onSettings={props.onSettings || (() => {})}
        onDragStart={props.onDragStart}
      />
    </div>
  );
};

export default ProjectCard;
