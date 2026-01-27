/**
 * Node Context Menu - Right-click menu for board items
 *
 * Highland Craft styled context menu with warm tones
 * and clear visual hierarchy.
 */

import type { Component } from 'solid-js';
import { For, Show } from 'solid-js';
import type { TaskTree, BoardTaskStatus } from '../../lib/types';

interface ContextMenuProps {
  x: number;
  y: number;
  node: TaskTree | null;
  onClose: () => void;
  // Task actions
  onAddChild: () => void;
  onAddSibling: () => void;
  onAddEval: () => void;
  onEdit: () => void;
  onAskGyp: () => void;
  onSetTaskStatus: (status: BoardTaskStatus) => void;
  onDelete: () => void;
  // Dispatch actions
  onDispatch?: () => void;
  onViewRuns?: () => void;
  hasRuns?: boolean;
  // Canvas actions (when node is null)
  onAddRootNode: () => void;
  onFitAll: () => void;
}

interface MenuItem {
  label: string;
  icon: string;
  action: () => void;
  danger?: boolean;
  disabled?: boolean;
  accent?: boolean;
}

const STATUS_CONFIG: { status: BoardTaskStatus; label: string; color: string; bg: string }[] = [
  { status: 'todo', label: 'To Do', color: 'bg-wool-500', bg: 'hover:bg-wool-500/10' },
  { status: 'doing', label: 'Doing', color: 'bg-amber-500', bg: 'hover:bg-amber-500/10' },
  { status: 'done', label: 'Done', color: 'bg-sage', bg: 'hover:bg-sage/10' },
  { status: 'blocked', label: 'Blocked', color: 'bg-terra', bg: 'hover:bg-terra/10' },
];

const MenuButton: Component<{
  item: MenuItem;
  onClick: () => void;
}> = (props) => (
  <button
    onClick={props.onClick}
    disabled={props.item.disabled}
    class="w-full px-3 py-2 text-left text-[13px] flex items-center gap-2.5 rounded-md mx-1 transition-all"
    classList={{
      'text-wool-200 hover:bg-pasture-700/50 hover:text-wool-100': !props.item.danger && !props.item.disabled && !props.item.accent,
      'text-amber-400 hover:bg-amber-500/10': props.item.accent,
      'text-terra hover:bg-terra/10': props.item.danger,
      'text-wool-600 cursor-not-allowed opacity-50': props.item.disabled,
    }}
    style={{ width: 'calc(100% - 8px)' }}
  >
    <i data-lucide={props.item.icon} class="w-4 h-4 opacity-70" />
    <span class="font-medium">{props.item.label}</span>
  </button>
);

const MenuDivider: Component = () => (
  <div
    class="my-1.5 mx-3"
    style={{ 'border-top': '1px solid rgba(61, 58, 54, 0.6)' }}
  />
);

const MenuSection: Component<{ label: string }> = (props) => (
  <div class="px-4 pt-2 pb-1">
    <span class="text-[10px] font-semibold text-wool-600 uppercase tracking-widest">
      {props.label}
    </span>
  </div>
);

export const NodeContextMenu: Component<ContextMenuProps> = (props) => {
  const handleClick = (action: () => void) => {
    action();
    props.onClose();
  };

  return (
    <div
      class="fixed rounded-xl py-2 min-w-[200px]"
      style={{
        'z-index': 1000,
        left: `${props.x}px`,
        top: `${props.y}px`,
        background: 'linear-gradient(180deg, #2a2825 0%, #1f1d1a 100%)',
        border: '1px solid #3d3a36',
        'box-shadow': '0 16px 48px rgba(0,0,0,0.6), 0 4px 12px rgba(0,0,0,0.3), inset 0 1px 0 rgba(255,255,255,0.03)',
      }}
      onClick={(e) => e.stopPropagation()}
    >
      <Show
        when={props.node}
        fallback={
          // Canvas context menu
          <>
            <MenuButton
              item={{ label: 'Add Task', icon: 'plus', action: props.onAddRootNode }}
              onClick={() => handleClick(props.onAddRootNode)}
            />
            <MenuButton
              item={{ label: 'Add Eval', icon: 'check-circle', action: props.onAddEval }}
              onClick={() => handleClick(props.onAddEval)}
            />
            <MenuDivider />
            <MenuButton
              item={{ label: 'Fit All', icon: 'maximize-2', action: props.onFitAll }}
              onClick={() => handleClick(props.onFitAll)}
            />
          </>
        }
      >
        {/* Task context menu */}
        <MenuSection label="Create" />
        <MenuButton
          item={{ label: 'Add Child', icon: 'corner-down-right', action: props.onAddChild }}
          onClick={() => handleClick(props.onAddChild)}
        />
        <MenuButton
          item={{ label: 'Add Sibling', icon: 'git-branch', action: props.onAddSibling }}
          onClick={() => handleClick(props.onAddSibling)}
        />
        <MenuButton
          item={{ label: 'Add Eval', icon: 'shield-check', action: props.onAddEval }}
          onClick={() => handleClick(props.onAddEval)}
        />

        <MenuDivider />

        <MenuButton
          item={{ label: 'Edit', icon: 'pencil', action: props.onEdit }}
          onClick={() => handleClick(props.onEdit)}
        />
        <MenuButton
          item={{ label: 'Ask Gyp', icon: 'sparkles', action: props.onAskGyp, accent: true }}
          onClick={() => handleClick(props.onAskGyp)}
        />

        {/* Dispatch section */}
        <Show when={props.onDispatch}>
          <MenuDivider />
          <MenuButton
            item={{ label: 'Dispatch Branch', icon: 'rocket', action: props.onDispatch! }}
            onClick={() => handleClick(props.onDispatch!)}
          />
          <Show when={props.hasRuns && props.onViewRuns}>
            <MenuButton
              item={{ label: 'View Runs', icon: 'list', action: props.onViewRuns! }}
              onClick={() => handleClick(props.onViewRuns!)}
            />
          </Show>
        </Show>

        <MenuDivider />

        {/* Status picker - compact dots with tooltips */}
        <div class="px-3 py-2 flex items-center gap-2">
          <span class="text-[10px] font-semibold text-wool-600 uppercase tracking-widest mr-1">
            Status
          </span>
          <div class="flex gap-2">
            <For each={STATUS_CONFIG}>
              {(cfg) => (
                <button
                  onClick={() => handleClick(() => props.onSetTaskStatus(cfg.status))}
                  class="p-1.5 rounded-md transition-all hover:bg-pasture-700/50"
                  classList={{
                    'bg-pasture-600/60 ring-1 ring-wool-500/40': props.node?.status === cfg.status,
                  }}
                  title={cfg.label}
                >
                  <span class={`block w-3 h-3 rounded-full ${cfg.color}`} />
                </button>
              )}
            </For>
          </div>
        </div>

        <MenuDivider />

        {/* Danger zone */}
        <MenuButton
          item={{ label: 'Delete', icon: 'trash-2', action: props.onDelete, danger: true }}
          onClick={() => handleClick(props.onDelete)}
        />
      </Show>
    </div>
  );
};
