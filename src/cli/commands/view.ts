/**
 * hirsel view <run> - View run status
 *
 * Shows detailed status of a run including workers, tasks, and activity.
 */

import { isJsonOutput, jsonOutput, ICONS } from "../index";
import { Status, Worker, Task, RunDetails, WorkerStatus, TaskStatus } from "../../shared/types";
import { formatStatus, ditherBar, dim, bold, colorize, STATUS_ICON } from "../../shared/theme";
import { existsSync } from "fs";
import { join } from "path";
import { homedir } from "os";
import { Database } from "bun:sqlite";

// Get hirsel root directory
function getHirselRoot(): string {
  return process.env.HIRSEL_ROOT || join(homedir(), ".hirsel");
}

// Get run details from database
function getRunDetails(runName: string): RunDetails | null {
  const dbPath = join(getHirselRoot(), "runs", runName, "hirsel.db");
  if (!existsSync(dbPath)) {
    return null;
  }

  try {
    const db = new Database(dbPath, { readonly: true });

    // Get state
    const state = db.query("SELECT * FROM state LIMIT 1").get() as {
      status: string;
      request: string | null;
      project_path: string | null;
      time_limit_minutes: number | null;
      started_at: string | null;
      summary: string | null;
    } | null;

    if (!state) {
      db.close();
      return null;
    }

    // Get workers
    const workers = db.query(`
      SELECT id, name, pid, session_id, status, work_dir, waiting_thread, location, last_heartbeat, created_at
      FROM workers
      ORDER BY created_at ASC
    `).all() as Worker[];

    // Get tasks
    const tasks = db.query(`
      SELECT id, name, status, created_at, completed_at, claimed_by, claimed_at, tokens_used, parent_id, blocked_by
      FROM tasks
      ORDER BY created_at ASC
    `).all() as Task[];

    // Get history
    const history = db.query(`
      SELECT id, timestamp, action, detail
      FROM history
      ORDER BY timestamp DESC
      LIMIT 20
    `).all() as Array<{ id: number; timestamp: string; action: string; detail: string | null }>;

    // Get evals
    const evalsRaw = db.query(`
      SELECT id, branch, eval_name, status, feedback, log_file, started_at, finished_at
      FROM evals
      ORDER BY started_at DESC
    `).all() as Array<{
      id: number;
      branch: string;
      eval_name: string | null;
      status: string;
      feedback: string | null;
      log_file: string | null;
      started_at: string;
      finished_at: string | null;
    }>;

    // Map to Eval interface (snake_case -> camelCase)
    const evals = evalsRaw.map(e => ({
      id: e.id,
      branch: e.branch,
      evalName: e.eval_name,
      status: e.status as import("../../shared/types").EvalStatus,
      feedback: e.feedback,
      logFile: e.log_file,
      startedAt: e.started_at,
      finishedAt: e.finished_at,
    }));

    // Calculate elapsed time
    let elapsedMinutes = 0;
    if (state.started_at) {
      const started = new Date(state.started_at);
      const now = new Date();
      elapsedMinutes = Math.floor((now.getTime() - started.getTime()) / 60000);
    }

    db.close();

    const tasksDone = tasks.filter(t => t.status === TaskStatus.DONE).length;
    const workersActive = workers.filter(w => w.status === WorkerStatus.WORKING || w.status === WorkerStatus.WAITING).length;

    return {
      name: runName,
      status: state.status as Status,
      projectPath: state.project_path,
      tasksDone,
      tasksTotal: tasks.length,
      workersActive,
      workersTotal: workers.length,
      elapsedMinutes,
      timeLimitMinutes: state.time_limit_minutes,
      hasUnread: false, // TODO: Implement
      workers,
      tasks,
      history,
      evals,
      request: state.request,
      summary: state.summary,
    };
  } catch (error) {
    console.error(`Error reading run ${runName}:`, error);
    return null;
  }
}

// Format worker line
function formatWorker(worker: Worker, isLeader: boolean): string {
  const statusIcon = ICONS[worker.status as keyof typeof ICONS] || ICONS.idle;
  const leaderMark = isLeader ? colorize(ICONS.leader, "yellow") + " " : "  ";

  let statusColor: "yellow" | "green" | "brightYellow" | "brightBlack" = "brightBlack";
  if (worker.status === "working") statusColor = "yellow";
  else if (worker.status === "done") statusColor = "green";
  else if (worker.status === "waiting") statusColor = "brightYellow";

  const statusStr = colorize(statusIcon, statusColor);
  const name = bold(worker.name);
  const statusText = dim(worker.status);
  const location = worker.location !== "local" ? dim(` [${worker.location}]`) : "";

  return `  ${leaderMark}${statusStr} ${name} ${statusText}${location}`;
}

// Format task line
function formatTask(task: Task): string {
  const statusIcon = STATUS_ICON[task.status] || ICONS.taskTodo;

  let statusColor: "yellow" | "green" | "brightBlack" = "brightBlack";
  if (task.status === "doing") statusColor = "yellow";
  else if (task.status === "done") statusColor = "green";

  const statusStr = colorize(statusIcon, statusColor);
  const name = task.status === "done" ? dim(task.name) : task.name;
  const claimedBy = task.claimedBy ? dim(` (${task.claimedBy})`) : "";

  return `  ${statusStr} ${task.id} ${name}${claimedBy}`;
}

// Format elapsed time
function formatElapsed(minutes: number): string {
  if (minutes < 60) {
    return `${minutes}m`;
  }
  const hours = Math.floor(minutes / 60);
  const mins = minutes % 60;
  return mins > 0 ? `${hours}h${mins}m` : `${hours}h`;
}

// Main command handler
export default async function view(args: string[]): Promise<void> {
  if (args.length === 0) {
    console.error(`${ICONS.error} Run name required`);
    console.error("Usage: hirsel view <run>");
    process.exit(1);
  }

  const runName = args[0];
  const details = getRunDetails(runName);

  if (!details) {
    console.error(`${ICONS.error} Run '${runName}' not found`);
    process.exit(1);
  }

  if (isJsonOutput()) {
    jsonOutput(details);
    return;
  }

  // Header
  const statusIcon = STATUS_ICON[details.status] || ICONS.idle;
  console.log();
  console.log(bold(details.name) + "  " + formatStatus(details.status));
  console.log(dim(`project: ${details.projectPath || "unknown"}`));
  console.log();

  // Workers section
  console.log(bold("Workers"));
  if (details.workers.length === 0) {
    console.log(dim("  No workers"));
  } else {
    // First worker is typically the leader
    const leader = details.workers[0]?.name;
    for (const worker of details.workers) {
      console.log(formatWorker(worker, worker.name === leader));
    }
  }
  console.log();

  // Tasks section
  const progressBar = ditherBar(details.tasksDone, details.tasksTotal, 16);
  console.log(bold("Tasks") + dim(` ${progressBar} ${details.tasksDone}/${details.tasksTotal}`));
  if (details.tasks.length === 0) {
    console.log(dim("  No tasks"));
  } else {
    for (const task of details.tasks.slice(0, 15)) {
      console.log(formatTask(task));
    }
    if (details.tasks.length > 15) {
      console.log(dim(`  ... and ${details.tasks.length - 15} more`));
    }
  }
  console.log();

  // Time info
  const elapsed = formatElapsed(details.elapsedMinutes);
  let timeInfo = `Elapsed: ${elapsed}`;
  if (details.timeLimitMinutes) {
    const remaining = details.timeLimitMinutes - details.elapsedMinutes;
    if (remaining > 0) {
      timeInfo += dim(` / ${formatElapsed(remaining)} remaining`);
    } else {
      timeInfo += colorize(` (time limit exceeded)`, "red");
    }
  }
  console.log(dim(timeInfo));

  // Activity section (recent history)
  if (details.history.length > 0) {
    console.log();
    console.log(bold("Recent Activity"));
    for (const entry of details.history.slice(0, 5)) {
      const time = entry.timestamp.substring(11, 16);
      const detail = entry.detail ? ` ${entry.detail}` : "";
      console.log(dim(`  ${time}`) + ` ${entry.action}${detail}`);
    }
  }

  console.log();
}
