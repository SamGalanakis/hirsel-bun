/**
 * hirsel-worker work-done - Signal that all work is complete
 *
 * Called when a worker has finished all its assigned work.
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

export default async function workDone(
  args: string[],
  ctx: Context
): Promise<void> {
  const force = args.includes("--force");
  const state = new State(getDbPath(ctx.runName));

  try {
    // Check if worker has any unclaimed tasks
    const tasks = state.getTasks();
    const unclaimedTasks = tasks.filter(
      (t) => t.status === "todo" && !t.claimedBy
    );

    if (!force && unclaimedTasks.length > 0) {
      console.log(
        JSON.stringify({
          error: `There are ${unclaimedTasks.length} unclaimed tasks remaining`,
          tasks: unclaimedTasks.map((t) => t.id),
        })
      );
      process.exit(1);
    }

    // Check if worker still has a claimed task
    const currentTask = tasks.find(
      (t) => t.claimedBy === ctx.workerName && t.status === "doing"
    );

    if (!force && currentTask) {
      console.log(
        JSON.stringify({
          error: `You still have task '${currentTask.id}' claimed`,
        })
      );
      process.exit(1);
    }

    // Mark worker as done
    state.setWorkerStatus(ctx.workerName, WorkerStatus.DONE);

    // Check if all workers are done
    const workers = state.getWorkers();
    const allDone = workers.every((w) => w.status === WorkerStatus.DONE);

    if (allDone) {
      // All workers done, mark run as done
      state.status = Status.DONE;
    }

    console.log(
      JSON.stringify({
        success: true,
        allWorkersDone: allDone,
      })
    );
  } finally {
    state.close();
  }
}
