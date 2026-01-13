/**
 * hirsel summary <run> - Generate run summary
 *
 * Shows a summary of the run including completed tasks, worker activity, and changes.
 */

import { isJsonOutput, jsonOutput, ICONS } from "../index";
import { dim, bold, colorize, ditherBar } from "../../shared/theme";
import { existsSync, readFileSync } from "fs";
import { join } from "path";
import { homedir } from "os";
import { Database } from "bun:sqlite";

// Get hirsel root directory
function getHirselRoot(): string {
  return process.env.HIRSEL_ROOT || join(homedir(), ".hirsel");
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
export default async function summary(args: string[]): Promise<void> {
  if (args.length === 0) {
    console.error(`${ICONS.error} Run name required`);
    console.error("Usage: hirsel summary <run>");
    process.exit(1);
  }

  const runName = args[0];
  const runDir = join(getHirselRoot(), "runs", runName);
  const dbPath = join(runDir, "hirsel.db");

  if (!existsSync(dbPath)) {
    console.error(`${ICONS.error} Run '${runName}' not found`);
    process.exit(1);
  }

  const db = new Database(dbPath, { readonly: true });

  // Get state
  const state = db.query("SELECT * FROM state LIMIT 1").get() as {
    status: string;
    request: string | null;
    project_path: string | null;
    started_at: string | null;
    summary: string | null;
    time_limit_minutes: number | null;
  } | null;

  // Get tasks
  const tasks = db.query("SELECT id, name, status, claimed_by FROM tasks").all() as Array<{
    id: string;
    name: string;
    status: string;
    claimed_by: string | null;
  }>;

  // Get workers
  const workers = db.query("SELECT name, status FROM workers").all() as Array<{
    name: string;
    status: string;
  }>;

  // Get history
  const history = db.query(`
    SELECT action, detail, timestamp
    FROM history
    ORDER BY timestamp ASC
  `).all() as Array<{ action: string; detail: string | null; timestamp: string }>;

  db.close();

  if (!state) {
    console.error(`${ICONS.error} Invalid run state`);
    process.exit(1);
  }

  // Calculate stats
  const tasksDone = tasks.filter(t => t.status === "done").length;
  const tasksTotal = tasks.length;

  let elapsedMinutes = 0;
  if (state.started_at) {
    const started = new Date(state.started_at);
    const now = new Date();
    elapsedMinutes = Math.floor((now.getTime() - started.getTime()) / 60000);
  }

  // Build summary
  const summaryData = {
    run: runName,
    status: state.status,
    project: state.project_path,
    elapsed: formatElapsed(elapsedMinutes),
    tasks: {
      done: tasksDone,
      total: tasksTotal,
      list: tasks,
    },
    workers: workers.map(w => w.name),
    milestones: history.filter(h =>
      h.action.includes("started") ||
      h.action.includes("completed") ||
      h.action.includes("delivered")
    ),
  };

  if (isJsonOutput()) {
    jsonOutput(summaryData);
    return;
  }

  // Print summary
  console.log();
  console.log(bold(`Summary: ${runName}`));
  console.log();

  // Status
  const statusIcon = ICONS[state.status as keyof typeof ICONS] || ICONS.idle;
  console.log(`Status:  ${statusIcon} ${state.status}`);
  console.log(`Project: ${state.project_path || "unknown"}`);
  console.log(`Elapsed: ${formatElapsed(elapsedMinutes)}`);
  console.log();

  // Tasks
  const progressBar = ditherBar(tasksDone, tasksTotal, 20);
  console.log(bold("Tasks"));
  console.log(`  ${dim(progressBar)} ${tasksDone}/${tasksTotal}`);
  console.log();

  // List completed tasks
  const completedTasks = tasks.filter(t => t.status === "done");
  if (completedTasks.length > 0) {
    console.log(dim("Completed:"));
    for (const task of completedTasks) {
      console.log(dim(`  ${ICONS.done} ${task.id}: ${task.name}`));
    }
    console.log();
  }

  // List remaining tasks
  const remainingTasks = tasks.filter(t => t.status !== "done");
  if (remainingTasks.length > 0) {
    console.log(dim("Remaining:"));
    for (const task of remainingTasks) {
      const icon = task.status === "doing" ? ICONS.taskDoing : ICONS.taskTodo;
      console.log(dim(`  ${icon} ${task.id}: ${task.name}`));
    }
    console.log();
  }

  // Workers
  console.log(bold("Workers"));
  console.log(`  ${workers.map(w => w.name).join(", ")}`);
  console.log();

  // Stored summary if available
  if (state.summary) {
    console.log(bold("AI Summary"));
    console.log(state.summary);
    console.log();
  }
}
