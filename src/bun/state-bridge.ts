/**
 * State Bridge - Connects SQLite State to RPC handlers
 *
 * Provides data transformation between the State class and RPC types,
 * plus run management operations.
 */

import { existsSync, readdirSync, rmSync } from "fs";
import { join } from "path";
import { State } from "../core/state";
import { config } from "../core/config";
import * as git from "../core/git";
import type { RunSummary, RunDetails, Worker, Task, HistoryEntry, Eval } from "../shared/types";
import type { RunDetail } from "../shared/rpc-types";
import { Status, WorkerStatus, TaskStatus } from "../shared/types";

// =============================================================================
// Run Discovery
// =============================================================================

/** Get all run names from the runs directory */
export function getRunNames(): string[] {
  const runsDir = join(config.root, "runs");
  if (!existsSync(runsDir)) {
    return [];
  }

  return readdirSync(runsDir, { withFileTypes: true })
    .filter((dirent) => dirent.isDirectory())
    .filter((dirent) => existsSync(join(runsDir, dirent.name, "state.db")))
    .map((dirent) => dirent.name);
}

/** Open state database for a run */
export function openState(runName: string): State | null {
  const dbPath = join(config.root, "runs", runName, "state.db");
  if (!existsSync(dbPath)) {
    return null;
  }
  return new State(dbPath);
}

// =============================================================================
// RPC Data Providers
// =============================================================================

/** Get summary for all runs */
export function getRuns(): RunSummary[] {
  const runNames = getRunNames();
  const summaries: RunSummary[] = [];

  for (const name of runNames) {
    const state = openState(name);
    if (!state) continue;

    try {
      const runState = state.getRunState();
      const workers = state.getWorkers();
      const tasks = state.getTasks();

      const tasksDone = tasks.filter((t) => t.status === "done").length;
      const activeWorkers = workers.filter(
        (w) => w.status === "working" || w.status === "waiting"
      ).length;

      // Calculate elapsed time
      let elapsedMinutes = 0;
      if (runState.startedAt) {
        const started = new Date(runState.startedAt);
        elapsedMinutes = Math.floor((Date.now() - started.getTime()) / 60000);
      }

      summaries.push({
        name,
        status: runState.status,
        projectPath: runState.projectPath,
        tasksDone,
        tasksTotal: tasks.length,
        workersActive: activeWorkers,
        workersTotal: workers.length,
        elapsedMinutes,
        timeLimitMinutes: runState.timeLimitMinutes,
        hasUnread: runState.unreadCount > 0,
      });
    } finally {
      state.close();
    }
  }

  // Sort by status (active first) then by name
  const statusOrder: Record<string, number> = {
    working: 0,
    waiting: 1,
    eval: 2,
    paused: 3,
    done: 4,
    delivered: 5,
    idle: 6,
  };

  summaries.sort((a, b) => {
    const aOrder = statusOrder[a.status] ?? 10;
    const bOrder = statusOrder[b.status] ?? 10;
    if (aOrder !== bOrder) return aOrder - bOrder;
    return a.name.localeCompare(b.name);
  });

  return summaries;
}

/** Get detailed info for a specific run */
export function getRunDetail(runName: string): RunDetail | null {
  const state = openState(runName);
  if (!state) return null;

  try {
    const runState = state.getRunState();
    const workers = state.getWorkers();
    const tasks = state.getTasks();
    const history = state.getHistory(100);
    const evals = state.getRunningEvals(); // Gets currently running evals

    // Calculate metrics
    const tasksDone = tasks.filter((t) => t.status === "done").length;
    const activeWorkers = workers.filter(
      (w) => w.status === "working" || w.status === "waiting"
    ).length;

    let elapsedMinutes = 0;
    if (runState.startedAt) {
      const started = new Date(runState.startedAt);
      elapsedMinutes = Math.floor((Date.now() - started.getTime()) / 60000);
    }

    // Build worker -> claimed task map
    const workerTasks = new Map<string, string>();
    for (const task of tasks) {
      if (task.claimedBy && task.status === TaskStatus.DOING) {
        workerTasks.set(task.claimedBy, task.id);
      }
    }

    // Transform workers to include UI-friendly properties
    const uiWorkers = workers.map((w, i) => ({
      ...w,
      isLeader: i === 0,
      currentTask: workerTasks.get(w.name) ?? null,
    }));

    // Build task tree from flat list
    const taskMap = new Map<string, Task & { children: Task[] }>();
    const rootTasks: Array<Task & { children: Task[] }> = [];

    // First pass: create all task objects with children array
    for (const task of tasks) {
      taskMap.set(task.id, { ...task, children: [] });
    }

    // Second pass: build tree structure
    for (const task of tasks) {
      const taskWithChildren = taskMap.get(task.id)!;
      if (task.parentId && taskMap.has(task.parentId)) {
        taskMap.get(task.parentId)!.children.push(taskWithChildren);
      } else {
        rootTasks.push(taskWithChildren);
      }
    }

    return {
      name: runName,
      status: runState.status,
      projectPath: runState.projectPath,
      tasksDone,
      tasksTotal: tasks.length,
      workersActive: activeWorkers,
      workersTotal: workers.length,
      elapsedMinutes,
      timeLimitMinutes: runState.timeLimitMinutes,
      hasUnread: runState.unreadCount > 0,
      workers: uiWorkers,
      tasks: rootTasks,
      activity: history,
      evals,
      request: runState.request,
      summary: runState.summary,
    };
  } finally {
    state.close();
  }
}

/** Get messages for a thread */
export function getMessages(
  runName: string,
  thread: string,
  limit?: number
): { id: number; thread: string; sender: string; content: string; waiting: boolean; timestamp: string }[] {
  const state = openState(runName);
  if (!state) return [];

  try {
    const messages = state.getMessages(thread);
    const limited = limit ? messages.slice(-limit) : messages;
    return limited;
  } finally {
    state.close();
  }
}

/** Get activity log for a run */
export function getActivity(runName: string, limit?: number): HistoryEntry[] {
  const state = openState(runName);
  if (!state) return [];

  try {
    return state.getHistory(limit ?? 100);
  } finally {
    state.close();
  }
}

// =============================================================================
// Run Operations
// =============================================================================

/** Pause a run */
export function pauseRun(runName: string): { success: boolean; error?: string } {
  const state = openState(runName);
  if (!state) {
    return { success: false, error: `Run '${runName}' not found` };
  }

  try {
    const currentStatus = state.status;
    if (currentStatus === Status.PAUSED) {
      return { success: false, error: "Run is already paused" };
    }
    if (
      currentStatus !== Status.WORKING &&
      currentStatus !== Status.WAITING &&
      currentStatus !== Status.EVAL
    ) {
      return { success: false, error: `Cannot pause run in '${currentStatus}' status` };
    }

    state.status = Status.PAUSED;

    // Mark all active workers as paused
    const workers = state.getWorkers();
    for (const worker of workers) {
      if (worker.status === WorkerStatus.WORKING || worker.status === WorkerStatus.WAITING) {
        state.setWorkerStatus(worker.name, WorkerStatus.PAUSED);
      }
    }

    return { success: true };
  } finally {
    state.close();
  }
}

/** Resume a run */
export function resumeRun(runName: string, timeLimit?: string): { success: boolean; error?: string } {
  const state = openState(runName);
  if (!state) {
    return { success: false, error: `Run '${runName}' not found` };
  }

  try {
    const currentStatus = state.status;
    if (currentStatus !== Status.PAUSED && currentStatus !== Status.TIMED_OUT) {
      return { success: false, error: `Cannot resume run in '${currentStatus}' status` };
    }

    // If time limit provided, set new time limit
    if (timeLimit) {
      const minutes = parseTimeLimit(timeLimit);
      if (minutes) {
        state.setTimeLimit(minutes);
      }
    }

    state.status = Status.WORKING;

    // Mark paused workers as needing restart
    const workers = state.getWorkers();
    for (const worker of workers) {
      if (worker.status === WorkerStatus.PAUSED) {
        state.setWorkerNeedsRestart(worker.name, true);
      }
    }

    return { success: true };
  } finally {
    state.close();
  }
}

/** Delete a run */
export function deleteRun(runName: string): { success: boolean; error?: string } {
  const runDir = join(config.root, "runs", runName);
  if (!existsSync(runDir)) {
    return { success: false, error: `Run '${runName}' not found` };
  }

  try {
    rmSync(runDir, { recursive: true });
    return { success: true };
  } catch (error) {
    return {
      success: false,
      error: `Failed to delete run: ${(error as Error).message}`,
    };
  }
}

/** Send a message to a thread */
export function sendMessage(
  runName: string,
  thread: string,
  message: string
): { success: boolean; error?: string } {
  const state = openState(runName);
  if (!state) {
    return { success: false, error: `Run '${runName}' not found` };
  }

  try {
    state.addMessage(thread, "user", message);
    return { success: true };
  } finally {
    state.close();
  }
}

/** Mark a task as done */
export function markTaskDone(runName: string, taskId: string): { success: boolean; error?: string } {
  const state = openState(runName);
  if (!state) {
    return { success: false, error: `Run '${runName}' not found` };
  }

  try {
    const task = state.getTask(taskId);
    if (!task) {
      return { success: false, error: `Task '${taskId}' not found` };
    }
    if (task.status === "done") {
      return { success: false, error: "Task is already done" };
    }

    state.completeTask(taskId);
    return { success: true };
  } finally {
    state.close();
  }
}

/** Unclaim a task */
export function unclaimTask(runName: string, taskId: string): { success: boolean; error?: string } {
  const state = openState(runName);
  if (!state) {
    return { success: false, error: `Run '${runName}' not found` };
  }

  try {
    const task = state.getTask(taskId);
    if (!task) {
      return { success: false, error: `Task '${taskId}' not found` };
    }
    if (!task.claimedBy) {
      return { success: false, error: "Task is not claimed" };
    }

    state.unclaimTask(taskId);
    return { success: true };
  } finally {
    state.close();
  }
}

/** Add a new task */
export function addTask(
  runName: string,
  taskId: string,
  description: string,
  parentId?: string,
  blockedBy?: string[]
): { success: boolean; error?: string } {
  const state = openState(runName);
  if (!state) {
    return { success: false, error: `Run '${runName}' not found` };
  }

  try {
    // Validate task ID format
    if (!/^[a-z][a-z0-9_]*$/.test(taskId)) {
      return {
        success: false,
        error: "Task ID must be lowercase, start with letter, use underscores only",
      };
    }

    // Check if task already exists
    if (state.getTask(taskId)) {
      return { success: false, error: `Task '${taskId}' already exists` };
    }

    // Validate parent exists
    if (parentId && !state.getTask(parentId)) {
      return { success: false, error: `Parent task '${parentId}' not found` };
    }

    // Validate blockers exist
    if (blockedBy) {
      for (const blockerId of blockedBy) {
        if (!state.getTask(blockerId)) {
          return { success: false, error: `Blocker task '${blockerId}' not found` };
        }
      }
    }

    state.addTask(taskId, description, parentId ?? null, blockedBy ?? null);
    return { success: true };
  } finally {
    state.close();
  }
}

/** Delete a task */
export function deleteTask(runName: string, taskId: string): { success: boolean; error?: string } {
  const state = openState(runName);
  if (!state) {
    return { success: false, error: `Run '${runName}' not found` };
  }

  try {
    const task = state.getTask(taskId);
    if (!task) {
      return { success: false, error: `Task '${taskId}' not found` };
    }
    if (task.claimedBy) {
      return { success: false, error: "Cannot delete claimed task" };
    }

    state.deleteTask(taskId);
    return { success: true };
  } finally {
    state.close();
  }
}

/** Reopen a completed task */
export function reopenTask(runName: string, taskId: string): { success: boolean; error?: string } {
  const state = openState(runName);
  if (!state) {
    return { success: false, error: `Run '${runName}' not found` };
  }

  try {
    const task = state.getTask(taskId);
    if (!task) {
      return { success: false, error: `Task '${taskId}' not found` };
    }
    if (task.status !== "done") {
      return { success: false, error: "Task is not completed" };
    }

    // Use unclaimTask to reset the task to 'todo' status
    state.unclaimTask(taskId);
    return { success: true };
  } finally {
    state.close();
  }
}

/** Create a new run */
export function createRun(
  runName: string,
  specPath: string,
  options: { workers: number | string; timeLimit: string | null; humanInTheLoop: boolean }
): { success: boolean; error?: string } {
  // Run creation is complex and involves:
  // 1. Creating run directory structure
  // 2. Initializing git worktrees
  // 3. Setting up state database
  // 4. Spawning workers
  // This should be delegated to the CLI 'go' command for now
  return {
    success: false,
    error: "Run creation from desktop app not yet implemented. Use 'hirsel go' from CLI."
  };
}

// =============================================================================
// Helpers
// =============================================================================

/** Parse time limit string (e.g., "30m", "1h", "1h30m") to minutes */
function parseTimeLimit(timeLimit: string): number | null {
  const match = timeLimit.match(/^(?:(\d+)h)?(?:(\d+)m)?$/);
  if (!match) return null;

  const hours = parseInt(match[1] ?? "0", 10);
  const minutes = parseInt(match[2] ?? "0", 10);
  return hours * 60 + minutes || null;
}

// =============================================================================
// Git Operations
// =============================================================================

/** Get diff between project and work directory */
export async function getDiff(runName: string): Promise<{ diff: string; stat: string }> {
  const state = openState(runName);
  if (!state) {
    return { diff: "", stat: "" };
  }

  try {
    const runState = state.getRunState();
    if (!runState.projectPath) {
      return { diff: "", stat: "" };
    }

    const workDir = join(config.root, "runs", runName, "work", "staging");
    const [diff, stat] = await Promise.all([
      git.getDiff(runState.projectPath, workDir).catch(() => ""),
      git.getDiffStat(runState.projectPath, workDir).catch(() => ""),
    ]);

    return { diff, stat };
  } finally {
    state.close();
  }
}

/** Deliver run - push staging branch to project as a named branch */
export async function deliverRun(
  runName: string,
  branchName?: string
): Promise<{ success: boolean; branchName?: string; error?: string }> {
  const state = openState(runName);
  if (!state) {
    return { success: false, error: `Run '${runName}' not found` };
  }

  try {
    const runState = state.getRunState();
    if (!runState.projectPath) {
      return { success: false, error: "Run has no project path" };
    }

    // Check for unmerged task branches
    const workDir = join(config.root, "runs", runName, "work", "staging");
    const unmerged = await git.listUnmergedBranches(workDir);
    if (unmerged.length > 0) {
      return {
        success: false,
        error: `Unmerged task branches: ${unmerged.join(", ")}. Complete or unclaim these tasks first.`,
      };
    }

    // Default branch name
    const targetBranch = branchName ?? `hirsel/${runName}`;

    // Check if branch already exists
    const exists = await git.branchExists(runState.projectPath, targetBranch);
    if (exists) {
      return {
        success: false,
        error: `Branch '${targetBranch}' already exists. Use a different branch name.`,
      };
    }

    // Push staging to project as the target branch
    await git.pushStagingAsBranch(workDir, runState.projectPath, targetBranch);

    // Update run status to delivered
    state.status = Status.DELIVERED;

    return { success: true, branchName: targetBranch };
  } catch (error) {
    return {
      success: false,
      error: `Delivery failed: ${(error as Error).message}`,
    };
  } finally {
    state.close();
  }
}
