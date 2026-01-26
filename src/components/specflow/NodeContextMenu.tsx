/**
 * Node Context Menu - Right-click menu for board items
 *
 * Provides actions for tasks:
 * - Adding child/sibling tasks
 * - Adding an eval that validates this task
 * - Editing properties
 * - Changing task status
 * - Asking Gyp
 * - Deleting
 */

import type { Component } from 'solid-js';
import { For } from 'solid-js';
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
}

interface MenuSeparator {
  separator: true;
  showStatus?: boolean; // Marker to show status submenu here
}

type MenuItemOrSeparator = MenuItem | MenuSeparator;

const isSeparator = (item: MenuItemOrSeparator): item is MenuSeparator => {
  return 'separator' in item;
};

const TaskStatusIcon: Component<{ status: BoardTaskStatus }> = (props) => {
  const colors: Record<BoardTaskStatus, string> = {
    todo: 'bg-wool-500',
    doing: 'bg-amber-500',
    done: 'bg-sage',
    blocked: 'bg-terra',
  };

  return <span class={`w-2 h-2 rounded-full ${colors[props.status]}`} />;
};

export const NodeContextMenu: Component<ContextMenuProps> = (props) => {
  const handleClick = (action: () => void) => {
    action();
    props.onClose();
  };

  // Build menu items based on context
  const menuItems = (): MenuItemOrSeparator[] => {
    if (!props.node) {
      // Canvas context menu
      return [
        { label: 'Add Task', icon: 'plus', action: props.onAddRootNode },
        { label: 'Add Eval', icon: 'check-circle', action: props.onAddEval },
        { separator: true },
        { label: 'Fit All', icon: 'maximize-2', action: props.onFitAll },
      ];
    }

    // Task context menu
    const items: MenuItemOrSeparator[] = [
      { label: 'Add Child', icon: 'corner-down-right', action: props.onAddChild },
      { label: 'Add Sibling', icon: 'plus-circle', action: props.onAddSibling },
      { label: 'Add Eval', icon: 'check-circle', action: props.onAddEval },
      { separator: true },
      { label: 'Edit', icon: 'pencil', action: props.onEdit },
      { label: 'Ask Gyp', icon: 'sparkles', action: props.onAskGyp },
    ];

    // Add dispatch actions if handler is provided
    if (props.onDispatch) {
      items.push({ separator: true });
      items.push({
        label: 'Dispatch this branch',
        icon: 'rocket',
        action: props.onDispatch,
      });
      if (props.hasRuns && props.onViewRuns) {
        items.push({
          label: 'View dispatched runs',
          icon: 'list',
          action: props.onViewRuns,
        });
      }
    }

    // Status separator with marker for status submenu
    items.push({ separator: true, showStatus: true });
    items.push({ separator: true });
    items.push({ label: 'Delete', icon: 'trash-2', action: props.onDelete, danger: true });

    return items;
  };

  return (
    <div
      class="fixed rounded-lg py-1 min-w-[180px]"
      style={{
        'z-index': 1000,
        left: `${props.x}px`,
        top: `${props.y}px`,
        background: 'rgba(26,26,26,0.98)',
        border: '1px solid rgba(255,255,255,0.1)',
        'box-shadow': '0 12px 40px rgba(0,0,0,0.5), 0 0 0 1px rgba(0,0,0,0.2)',
        'backdrop-filter': 'blur(12px)',
      }}
      onClick={(e) => e.stopPropagation()}
    >
      {/* Regular menu items */}
      <For each={menuItems()}>
        {(item, index) => {
          if (isSeparator(item)) {
            // Check if we need to render status submenu
            if (props.node && item.showStatus) {
              return (
                <>
                  <div
                    class="my-1"
                    style={{ 'border-top': '1px solid rgba(255,255,255,0.08)' }}
                  />
                  {/* Task status submenu */}
                  <div class="px-2 py-1 mb-1">
                    <div class="text-[10px] font-semibold text-wool-600 uppercase tracking-wider px-1 mb-1">
                      Status
                    </div>
                    <div class="flex gap-1">
                      <For each={['todo', 'doing', 'done', 'blocked'] as BoardTaskStatus[]}>
                        {(status) => (
                          <button
                            onClick={() => handleClick(() => props.onSetTaskStatus(status))}
                            class="flex-1 flex items-center justify-center gap-1 py-1.5 rounded text-[10px] hover:bg-white/5 transition-colors"
                            classList={{
                              'bg-white/10': props.node?.status === status,
                            }}
                            title={status}
                          >
                            <TaskStatusIcon status={status} />
                          </button>
                        )}
                      </For>
                    </div>
                  </div>
                </>
              );
            }
            return (
              <div
                class="my-1"
                style={{ 'border-top': '1px solid rgba(255,255,255,0.08)' }}
              />
            );
          }

          return (
            <button
              onClick={() => handleClick(item.action)}
              disabled={item.disabled}
              class="w-full px-3 py-1.5 text-left text-sm flex items-center gap-2 transition-colors"
              classList={{
                'text-wool-200 hover:bg-white/5': !item.danger && !item.disabled,
                'text-terra hover:bg-terra/10': item.danger,
                'text-wool-600 cursor-not-allowed': item.disabled,
              }}
            >
              <i data-lucide={item.icon} class="w-3.5 h-3.5" />
              {item.label}
            </button>
          );
        }}
      </For>
    </div>
  );
};
