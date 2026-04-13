import {
  type Component,
  For,
  Show,
  createSignal,
  createEffect,
  on,
} from "solid-js";
import type { Task } from "@/lib/api/types";
import {
  listTasks,
  createTask,
  updateTask,
  deleteTask,
} from "@/lib/api/tasks";
import { cn } from "@/lib/cn";

interface TaskListProps {
  projectId: number;
  refreshNonce?: number;
  onTaskSelect?: (task: Task) => void;
}

const STATUS_ORDER: Record<string, number> = {
  active: 0,
  todo: 1,
  done: 2,
};

const STATUS_DOTS: Record<string, string> = {
  todo: "bg-muted-foreground/30",
  active: "bg-signal-amber",
  done: "bg-signal-green/60",
};

const STATUS_LABELS: Record<string, string> = {
  todo: "todo",
  active: "active",
  done: "done",
};

function nextStatus(current: string): string {
  if (current === "todo") return "active";
  if (current === "active") return "done";
  return "todo";
}

const TaskList: Component<TaskListProps> = (props) => {
  const [tasks, setTasks] = createSignal<Task[]>([]);
  const [newTitle, setNewTitle] = createSignal("");
  const [editingId, setEditingId] = createSignal<string | null>(null);
  const [editTitle, setEditTitle] = createSignal("");
  const [expandedId, setExpandedId] = createSignal<string | null>(null);

  const loadTasks = async () => {
    try {
      const result = await listTasks(props.projectId);
      setTasks(result);
    } catch (e) {
      console.error("Failed to load tasks", e);
    }
  };

  // Load on mount and when nonce changes
  createEffect(
    on(
      () => [props.projectId, props.refreshNonce],
      () => {
        void loadTasks();
      },
    ),
  );

  const handleCreate = async () => {
    const title = newTitle().trim();
    if (!title) return;
    try {
      await createTask(props.projectId, { title });
      setNewTitle("");
      await loadTasks();
    } catch (e) {
      console.error("Failed to create task", e);
    }
  };

  const handleCreateKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter") {
      e.preventDefault();
      void handleCreate();
    }
  };

  const handleStatusToggle = async (task: Task) => {
    const next = nextStatus(task.status);
    try {
      await updateTask(props.projectId, task.id, { status: next });
      await loadTasks();
    } catch (e) {
      console.error("Failed to update task status", e);
    }
  };

  const handleTitleSave = async (taskId: string) => {
    const title = editTitle().trim();
    if (!title) {
      setEditingId(null);
      return;
    }
    try {
      await updateTask(props.projectId, taskId, { title });
      setEditingId(null);
      await loadTasks();
    } catch (e) {
      console.error("Failed to update task title", e);
    }
  };

  const handleDelete = async (taskId: string) => {
    try {
      await deleteTask(props.projectId, taskId);
      await loadTasks();
    } catch (e) {
      console.error("Failed to delete task", e);
    }
  };

  const sortedTasks = () => {
    return [...tasks()].sort((a, b) => {
      const sa = STATUS_ORDER[a.status] ?? 1;
      const sb = STATUS_ORDER[b.status] ?? 1;
      if (sa !== sb) return sa - sb;
      return a.sort_order - b.sort_order;
    });
  };

  const contentPreview = (task: Task): string => {
    if (!task.content) return "";
    const first = task.content.split("\n").find((line) => line.trim().length > 0);
    if (!first) return "";
    const clean = first.replace(/^#+\s*/, "").trim();
    return clean.length > 80 ? clean.slice(0, 80) + "\u2026" : clean;
  };

  return (
    <div class="flex flex-col h-full">
      {/* Quick-add */}
      <div class="px-3 py-2 border-b border-border/30">
        <div class="flex items-center gap-2">
          <span class="text-muted-foreground/30 text-sm">+</span>
          <input
            type="text"
            class="flex-1 bg-transparent text-foreground text-sm placeholder:text-muted-foreground/25 outline-none border-none"
            placeholder="Add a task..."
            value={newTitle()}
            onInput={(e) => setNewTitle(e.currentTarget.value)}
            onKeyDown={handleCreateKeyDown}
          />
        </div>
      </div>

      {/* Task list */}
      <div class="flex-1 overflow-y-auto chassis-scroll">
        <Show
          when={sortedTasks().length > 0}
          fallback={
            <div class="px-4 py-8 text-center text-[11px] text-muted-foreground/30">
              No tasks yet
            </div>
          }
        >
          <For each={sortedTasks()}>
            {(task) => {
              const isDone = () => task.status === "done";
              const isExpanded = () => expandedId() === task.id;
              const isEditing = () => editingId() === task.id;

              return (
                <div
                  class={cn(
                    "group border-b border-border/20 transition-colors",
                    isDone() ? "opacity-50" : "",
                  )}
                >
                  {/* Main row */}
                  <div class="flex items-start gap-2 px-3 py-2">
                    {/* Status dot — click to toggle */}
                    <button
                      class="mt-1.5 shrink-0 cursor-pointer"
                      onClick={() => void handleStatusToggle(task)}
                      title={`Status: ${STATUS_LABELS[task.status] ?? task.status}. Click to cycle.`}
                    >
                      <span
                        class={cn(
                          "block h-2 w-2 rounded-full transition-colors",
                          STATUS_DOTS[task.status] ?? "bg-muted-foreground/30",
                          task.status === "active" ? "animate-pulse-dot" : "",
                        )}
                      />
                    </button>

                    {/* Title */}
                    <div class="flex-1 min-w-0">
                      <Show
                        when={!isEditing()}
                        fallback={
                          <input
                            type="text"
                            class="w-full bg-transparent text-foreground text-sm outline-none border-b border-border/40"
                            value={editTitle()}
                            onInput={(e) =>
                              setEditTitle(e.currentTarget.value)
                            }
                            onKeyDown={(e) => {
                              if (e.key === "Enter")
                                void handleTitleSave(task.id);
                              if (e.key === "Escape") setEditingId(null);
                            }}
                            onBlur={() => void handleTitleSave(task.id)}
                            autofocus
                          />
                        }
                      >
                        <button
                          class={cn(
                            "text-left text-sm w-full truncate",
                            isDone()
                              ? "line-through text-muted-foreground/50"
                              : "text-foreground",
                          )}
                          onClick={() => {
                            if (props.onTaskSelect) {
                              props.onTaskSelect(task);
                            } else {
                              setExpandedId(isExpanded() ? null : task.id);
                            }
                          }}
                          onDblClick={() => {
                            setEditingId(task.id);
                            setEditTitle(task.title);
                          }}
                        >
                          {task.title}
                        </button>
                      </Show>

                      {/* Content preview */}
                      <Show when={!isExpanded() && contentPreview(task)}>
                        <div class="text-[11px] text-muted-foreground/35 truncate mt-0.5">
                          {contentPreview(task)}
                        </div>
                      </Show>
                    </div>

                    {/* Actions (visible on hover) */}
                    <div class="shrink-0 flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
                      <button
                        class="p-1 text-muted-foreground/30 hover:text-destructive transition-colors"
                        onClick={() => void handleDelete(task.id)}
                        title="Delete task"
                      >
                        <svg
                          width="12"
                          height="12"
                          viewBox="0 0 12 12"
                          fill="none"
                        >
                          <path
                            d="M2 2l8 8M10 2l-8 8"
                            stroke="currentColor"
                            stroke-width="1.5"
                          />
                        </svg>
                      </button>
                    </div>
                  </div>

                  {/* Expanded content */}
                  <Show when={isExpanded() && task.content}>
                    <div class="px-7 pb-3">
                      <div
                        class="text-[12px] text-muted-foreground/60 leading-relaxed whitespace-pre-wrap"
                        style={{ "max-height": "200px", overflow: "auto" }}
                      >
                        {task.content}
                      </div>
                    </div>
                  </Show>
                </div>
              );
            }}
          </For>
        </Show>
      </div>

      {/* Footer stats */}
      <div class="px-3 py-1.5 border-t border-border/20 flex items-center justify-between">
        <span class="font-mono text-[9px] text-muted-foreground/30 uppercase tracking-wider">
          {tasks().length} task{tasks().length !== 1 ? "s" : ""}
        </span>
        <span class="font-mono text-[9px] text-muted-foreground/30">
          {tasks().filter((t) => t.status === "done").length} done
        </span>
      </div>
    </div>
  );
};

export default TaskList;
