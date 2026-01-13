/**
 * hirsel-worker task-await - Wait for available tasks
 *
 * Blocks until a claimable task becomes available.
 * Used by workers when they have no work to do.
 */

import { join } from "path";
import { homedir } from "os";
import { State } from "../../core/state";
import { Status, WorkerStatus } from "../../shared/types";

interface Context {
  runName: string;
  workerName: string;
}

function getDbPath(runName: string): string {
  const root = process.env.HIRSEL_ROOT || join(homedir(), ".hirsel");
  return join(root, "runs", runName, "hirsel.db");
}

const POLL_INTERVAL_MS = 2000; // Check every 2 seconds
const MAX_WAIT_MS = 300000; // Maximum 5 minutes

export default async function taskAwait(
  args: string[],
  ctx: Context
): Promise<void> {
  const startTime = Date.now();

  // Mark worker as awaiting
  const state = new State(getDbPath(ctx.runName));
  state.setWorkerStatus(ctx.workerName, WorkerStatus.AWAITING);
  state.close();

  while (Date.now() - startTime < MAX_WAIT_MS) {
    const checkState = new State(getDbPath(ctx.runName));

    try {
      // Check run status
      const runStatus = checkState.status;
      if (
        runStatus === Status.DONE ||
        runStatus === Status.DELIVERED ||
        runStatus === Status.PAUSED
      ) {
        console.log(
          JSON.stringify({
            status: "run_finished",
            runStatus,
            message: "Run is no longer active",
          })
        );
        return;
      }

      // Get claimable tasks
      const claimable = checkState.getClaimableTasks();

      if (claimable.length > 0) {
        console.log(
          JSON.stringify({
            status: "tasks_available",
            tasks: claimable.map((t) => ({
              id: t.id,
              name: t.name,
            })),
          })
        );
        return;
      }

      // Check if all tasks are done
      const tasks = checkState.getTasks();
      const allDone = tasks.length > 0 && tasks.every((t) => t.status === "done");

      if (allDone) {
        console.log(
          JSON.stringify({
            status: "all_done",
            message: "All tasks are completed",
          })
        );
        return;
      }
    } finally {
      checkState.close();
    }

    // Wait before next check
    await new Promise((resolve) => setTimeout(resolve, POLL_INTERVAL_MS));
  }

  // Timeout
  console.log(
    JSON.stringify({
      status: "timeout",
      message: "Timed out waiting for tasks",
    })
  );
}
