/**
 * TaskDetailModal - Task detail popup with blockers and actions
 */
import { type Component, For, Show } from 'solid-js';
import type { Task, WorkerDisplay } from '../../lib/types';
import { formatDuration, formatRelativeTime, formatTokens } from '../../lib/utils/formatters';
import { getTaskStatusConfig } from '../../lib/utils/status';
import { BaseModal, Icon, SheepAvatar } from '../shared';

interface TaskDetailModalProps {
  task: Task;
  allTasks: Task[];
  workers: WorkerDisplay[];
  onClose: () => void;
  onComplete: () => void;
  onReopen: () => void;
  onUnclaim: () => void;
  onDelete: () => void;
  onTaskClick: (taskId: string) => void;
}

export const TaskDetailModal: Component<TaskDetailModalProps> = (props) => {
  // Get blocker tasks
  const blockerTasks = () => {
    const blockerIds = props.task.blockedBy || [];
    return blockerIds
      .map((id) => props.allTasks.find((t) => t.id === id))
      .filter((t): t is Task => t != null);
  };

  // Get parent task
  const parentTask = () => {
    if (!props.task.parentId) return null;
    return props.allTasks.find((t) => t.id === props.task.parentId);
  };

  // Get subtasks count
  const subtasksCount = () => {
    return props.allTasks.filter((t) => t.parentId === props.task.id).length;
  };

  // Get assigned worker
  const assignedWorker = () => {
    if (!props.task.claimedBy) return null;
    return props.workers.find((w) => w.name === props.task.claimedBy);
  };

  // Is task blocked
  const isBlocked = () => (props.task.blockedBy || []).length > 0;

  return (
    <BaseModal
      onClose={props.onClose}
      overlayClass="bg-black/80"
      class="bg-pasture-800 border border-pasture-600 w-full max-w-lg"
    >
      {/* Header */}
        <div class="flex items-center justify-between p-4 border-b border-pasture-600">
          <div class="flex items-center gap-3">
            <div class={getTaskStatusConfig(props.task.status).color}>
              <Icon name={getTaskStatusConfig(props.task.status).icon} class="w-6 h-6" />
            </div>
            <div>
              <span class={`px-2 py-0.5 text-xs rounded ${getTaskStatusConfig(props.task.status).badgeClass}`}>
                {getTaskStatusConfig(props.task.status).label}
              </span>
              <Show when={isBlocked()}>
                <span class="ml-2 px-2 py-0.5 text-xs rounded bg-terra/20 text-terra">
                  <Icon name="lock" class="w-3 h-3 inline mr-1" />
                  Blocked
                </span>
              </Show>
            </div>
          </div>
          <button
            class="p-1.5 rounded hover:bg-pasture-700 text-wool-500 hover:text-wool-300"
            onClick={props.onClose}
          >
            <Icon name="x" class="w-5 h-5" />
          </button>
        </div>

        {/* Content */}
        <div class="p-4 space-y-4">
          {/* Description */}
          <div>
            <p class="text-base text-wool-200 leading-relaxed">{props.task.description}</p>
          </div>

          {/* Assigned Worker */}
          <Show when={assignedWorker()}>
            {(worker) => (
              <div class="card p-3">
                <p class="text-xs text-wool-500 mb-2">Assigned Worker</p>
                <div class="flex items-center gap-3">
                  <SheepAvatar config={worker().sheepConfig} size={32} status={worker().status} />
                  <div>
                    <p class="text-sm font-medium text-wool-200">{worker().name}</p>
                    <p class="text-xs text-wool-500">{worker().status}</p>
                  </div>
                </div>
              </div>
            )}
          </Show>

          <Show when={props.task.claimedBy && !assignedWorker()}>
            <div class="card p-3">
              <p class="text-xs text-wool-500 mb-1">Claimed By</p>
              <p class="text-sm text-wool-300">{props.task.claimedBy}</p>
            </div>
          </Show>

          {/* Blocked By */}
          <Show when={blockerTasks().length > 0}>
            <div class="card p-3 border-terra/30">
              <p class="text-xs text-terra mb-2">Blocked By</p>
              <div class="space-y-2">
                <For each={blockerTasks()}>
                  {(blocker) => (
                    <button
                      class="w-full text-left p-2 rounded bg-pasture-700 hover:bg-pasture-600 transition-colors"
                      onClick={() => {
                        props.onClose();
                        props.onTaskClick(blocker.id);
                      }}
                    >
                      <div class="flex items-center gap-2">
                        <Icon
                          name={getTaskStatusConfig(blocker.status).icon}
                          class={`w-4 h-4 ${getTaskStatusConfig(blocker.status).color}`}
                        />
                        <span class="text-sm text-wool-300 flex-1 truncate">
                          {blocker.description}
                        </span>
                        <span
                          class={`px-1.5 py-0.5 text-xs rounded ${getTaskStatusConfig(blocker.status).badgeClass}`}
                        >
                          {getTaskStatusConfig(blocker.status).label}
                        </span>
                      </div>
                    </button>
                  )}
                </For>
              </div>
            </div>
          </Show>

          {/* Parent Task */}
          <Show when={parentTask()}>
            {(parent) => (
              <div class="card p-3">
                <p class="text-xs text-wool-500 mb-2">Parent Task</p>
                <button
                  class="w-full text-left p-2 rounded bg-pasture-700 hover:bg-pasture-600 transition-colors"
                  onClick={() => {
                    props.onClose();
                    props.onTaskClick(parent().id);
                  }}
                >
                  <div class="flex items-center gap-2">
                    <Icon
                      name={getTaskStatusConfig(parent().status).icon}
                      class={`w-4 h-4 ${getTaskStatusConfig(parent().status).color}`}
                    />
                    <span class="text-sm text-wool-300 truncate">{parent().description}</span>
                  </div>
                </button>
              </div>
            )}
          </Show>

          {/* Subtasks count */}
          <Show when={subtasksCount() > 0}>
            <div class="card p-3">
              <p class="text-xs text-wool-500 mb-1">Subtasks</p>
              <p class="text-sm text-wool-200">{subtasksCount()} subtask(s)</p>
            </div>
          </Show>

          {/* Stats */}
          <div class="card p-3">
            <p class="text-xs text-wool-500 mb-2">Details</p>
            <div class="grid grid-cols-2 gap-4 text-sm">
              <Show when={props.task.tokensUsed}>
                <div>
                  <p class="text-xs text-wool-600">Tokens Used</p>
                  <p class="text-wool-300">{formatTokens(props.task.tokensUsed)}</p>
                </div>
              </Show>
              <div>
                <p class="text-xs text-wool-600">Created</p>
                <p class="text-wool-300">{formatRelativeTime(props.task.createdAt)}</p>
              </div>
              <Show when={props.task.claimedAt}>
                <div>
                  <p class="text-xs text-wool-600">Claimed</p>
                  <p class="text-wool-300">{formatRelativeTime(props.task.claimedAt)}</p>
                </div>
              </Show>
              <Show when={props.task.completedAt}>
                <div>
                  <p class="text-xs text-wool-600">Completed</p>
                  <p class="text-wool-300">{formatRelativeTime(props.task.completedAt)}</p>
                </div>
              </Show>
              <Show when={props.task.claimedAt && props.task.completedAt}>
                <div>
                  <p class="text-xs text-wool-600">Duration</p>
                  <p class="text-wool-300">
                    {formatDuration(props.task.claimedAt, props.task.completedAt)}
                  </p>
                </div>
              </Show>
            </div>
          </div>
        </div>

        {/* Footer */}
        <div class="p-4 border-t border-pasture-600 flex justify-between">
          <div class="flex gap-2">
            <button
              class="btn-ghost text-terra hover:bg-terra/10"
              onClick={props.onDelete}
            >
              <Icon name="trash-2" class="w-4 h-4" />
              Delete
            </button>
          </div>
          <div class="flex gap-2">
            <Show when={props.task.claimedBy}>
              <button class="btn-outline" onClick={props.onUnclaim}>
                <Icon name="user-minus" class="w-4 h-4" />
                Unclaim
              </button>
            </Show>
            <Show when={props.task.status === 'done'}>
              <button class="btn-outline" onClick={props.onReopen}>
                <Icon name="rotate-ccw" class="w-4 h-4" />
                Reopen
              </button>
            </Show>
            <Show when={props.task.status !== 'done'}>
              <button class="btn" onClick={props.onComplete}>
                <Icon name="check" class="w-4 h-4" />
                Mark Done
              </button>
            </Show>
          </div>
        </div>
    </BaseModal>
  );
};
