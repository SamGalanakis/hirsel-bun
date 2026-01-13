/**
 * hirsel-worker time-status - Get time limit status
 *
 * Returns information about elapsed time and remaining time for the run.
 */

import { join } from "path";
import { homedir } from "os";
import { State } from "../../core/state";

interface Context {
  runName: string;
  workerName: string;
}

function getDbPath(runName: string): string {
  const root = process.env.HIRSEL_ROOT || join(homedir(), ".hirsel");
  return join(root, "runs", runName, "hirsel.db");
}

export default async function timeStatus(
  args: string[],
  ctx: Context
): Promise<void> {
  const state = new State(getDbPath(ctx.runName));

  try {
    const timeInfo = state.getTimeInfo();

    if (!timeInfo) {
      console.log(
        JSON.stringify({
          message: "No time limit set for this run.",
        })
      );
      return;
    }

    // Get current task info
    const tasks = state.getTasks();
    const currentTask = tasks.find(
      (t) => t.claimedBy === ctx.workerName && t.status === "doing"
    );

    let taskElapsedMinutes = 0;
    if (currentTask?.claimedAt) {
      const claimedAt = new Date(currentTask.claimedAt);
      taskElapsedMinutes = (Date.now() - claimedAt.getTime()) / 60000;
    }

    console.log(
      JSON.stringify({
        elapsedMinutes: Math.round(timeInfo.elapsedMinutes * 10) / 10,
        remainingMinutes: timeInfo.remainingMinutes
          ? Math.round(timeInfo.remainingMinutes * 10) / 10
          : null,
        elapsedPct: timeInfo.elapsedPct
          ? Math.round(timeInfo.elapsedPct)
          : null,
        remainingPct: timeInfo.remainingPct
          ? Math.round(timeInfo.remainingPct)
          : null,
        isExpired: timeInfo.isExpired,
        currentTaskId: currentTask?.id || null,
        currentTaskName: currentTask?.name || null,
        taskElapsedMinutes: Math.round(taskElapsedMinutes * 10) / 10,
        taskMessage: currentTask
          ? `Current task '${currentTask.name}': ${Math.round(taskElapsedMinutes)} min elapsed`
          : null,
      })
    );
  } finally {
    state.close();
  }
}
