/**
 * hirsel-worker task-unclaim [id] - Release a claimed task
 *
 * Releases the currently claimed task without completing it.
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

export default async function taskUnclaim(
  args: string[],
  ctx: Context
): Promise<void> {
  const state = new State(getDbPath(ctx.runName));

  try {
    let taskId = args[0];

    // If no task ID provided, find the worker's current task
    if (!taskId) {
      const tasks = state.getTasks();
      const currentTask = tasks.find(
        (t) => t.claimedBy === ctx.workerName && t.status === "doing"
      );

      if (!currentTask) {
        console.log(
          JSON.stringify({ error: "No task currently claimed by this worker" })
        );
        process.exit(1);
      }

      taskId = currentTask.id;
    }

    const task = state.getTask(taskId);
    if (!task) {
      console.log(JSON.stringify({ error: `Task '${taskId}' not found` }));
      process.exit(1);
    }

    // Verify the worker owns this task
    if (task.claimedBy !== ctx.workerName) {
      console.log(
        JSON.stringify({
          error: `Task '${taskId}' is not claimed by you (claimed by: ${task.claimedBy || "none"})`,
        })
      );
      process.exit(1);
    }

    // Unclaim the task
    state.unclaimTask(taskId);

    console.log(
      JSON.stringify({
        success: true,
        task: {
          id: taskId,
          status: "todo",
          claimedBy: null,
        },
      })
    );
  } finally {
    state.close();
  }
}
