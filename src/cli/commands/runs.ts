/**
 * hirsel runs - List all runs
 *
 * Lists all runs with their status, worker count, task progress, and elapsed time.
 */

import { isJsonOutput, jsonOutput } from "../index";
import { Status, type RunSummary } from "../../shared/types";
import { ICONS, formatStatus, ditherBar, dim, bold, colorize } from "../../shared/theme";
import { config } from "../../core/config";
import { State } from "../../core/state";
import { existsSync, readdirSync } from "fs";
import { join } from "path";

// Get hirsel root directory
function getHirselRoot(): string {
  return config.root;
}

// Get all run directories
function getRunDirectories(): string[] {
  const runsDir = join(getHirselRoot(), "runs");
  if (!existsSync(runsDir)) {
    return [];
  }

  return readdirSync(runsDir, { withFileTypes: true })
    .filter((dirent) => dirent.isDirectory())
    .map((dirent) => dirent.name);
}

// Get run summary from database
function getRunSummary(runName: string): RunSummary | null {
  const dbPath = join(getHirselRoot(), "runs", runName, "hirsel.db");
  if (!existsSync(dbPath)) {
    return null;
  }

  try {
    const stateDb = new State(dbPath);
    const runState = stateDb.getRunState();
    const tasks = stateDb.getTasks();
    const workers = stateDb.getWorkers();

    const tasksDone = tasks.filter((t) => t.status === "done").length;
    const workersActive = workers.filter(
      (w) => w.status === "working" || w.status === "waiting"
    ).length;

    // Calculate elapsed time
    let elapsedMinutes = 0;
    if (runState.startedAt) {
      const started = new Date(runState.startedAt);
      const now = new Date();
      elapsedMinutes = Math.floor((now.getTime() - started.getTime()) / 60000);
    }

    stateDb.close();

    return {
      name: runName,
      status: runState.status,
      projectPath: runState.projectPath,
      tasksDone,
      tasksTotal: tasks.length,
      workersActive,
      workersTotal: workers.length,
      elapsedMinutes,
      timeLimitMinutes: runState.timeLimitMinutes,
      hasUnread: runState.unreadCount > 0,
    };
  } catch (error) {
    console.error(`Error reading run ${runName}:`, error);
    return null;
  }
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
export default async function runs(args: string[]): Promise<void> {
  const runNames = getRunDirectories();

  if (runNames.length === 0) {
    if (isJsonOutput()) {
      jsonOutput({ runs: [] });
    } else {
      console.log(dim("No runs found"));
      console.log(dim("Start a new run with: hirsel go <name> <spec>"));
    }
    return;
  }

  // Get summaries for all runs
  const summaries: RunSummary[] = [];
  for (const name of runNames) {
    const summary = getRunSummary(name);
    if (summary) {
      summaries.push(summary);
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

  if (isJsonOutput()) {
    jsonOutput({ runs: summaries });
    return;
  }

  // Print header
  console.log(bold("\nRuns\n"));

  // Print each run
  for (const run of summaries) {
    const statusIcon = ICONS[run.status as keyof typeof ICONS] || ICONS.idle;
    const progressBar = ditherBar(run.tasksDone, run.tasksTotal, 10);
    const progress = `${run.tasksDone}/${run.tasksTotal}`;
    const elapsed = formatElapsed(run.elapsedMinutes);
    const workers = `${run.workersActive}/${run.workersTotal}w`;

    // Status-based coloring
    let statusColor: "yellow" | "green" | "brightYellow" | "brightBlack" = "brightBlack";
    if (run.status === "working" || run.status === "eval") statusColor = "yellow";
    else if (run.status === "done" || run.status === "delivered") statusColor = "green";
    else if (run.status === "waiting" || run.status === "paused") statusColor = "brightYellow";

    const statusStr = colorize(`${statusIcon} ${run.status}`, statusColor);
    const nameStr = bold(run.name.padEnd(20));
    const progressStr = `${dim(progressBar)} ${progress}`;
    const workersStr = dim(workers);
    const elapsedStr = dim(elapsed);

    console.log(`  ${statusStr.padEnd(20)} ${nameStr} ${progressStr} ${workersStr} ${elapsedStr}`);
  }

  console.log();
  console.log(dim("View a run: hirsel view <name>"));
}
