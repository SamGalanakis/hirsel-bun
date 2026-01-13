/**
 * hirsel-worker task-claim <id> - Claim a task to work on
 *
 * Only one task can be claimed at a time per worker.
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

export default async function taskClaim(
  args: string[],
  ctx: Context
): Promise<void> {
  if (args.length === 0) {
    console.log(JSON.stringify({ error: "Task ID required" }));
    process.exit(1);
  }

  const taskId = args[0];
  const state = new State(getDbPath(ctx.runName));

  try {
    // Check if worker already has a claimed task
    const tasks = state.getTasks();
    const currentTask = tasks.find(
      (t) => t.claimedBy === ctx.workerName && t.status === "doing"
    );

    if (currentTask) {
      console.log(
        JSON.stringify({
          error: `Already have claimed task: ${currentTask.id}. Finish or unclaim it first.`,
        })
      );
      process.exit(1);
    }

    // Try to claim the task
    const success = state.claimTask(taskId, ctx.workerName);

    if (!success) {
      const task = state.getTask(taskId);
      if (!task) {
        console.log(JSON.stringify({ error: `Task '${taskId}' not found` }));
      } else if (task.claimedBy) {
        console.log(
          JSON.stringify({
            error: `Task '${taskId}' already claimed by ${task.claimedBy}`,
          })
        );
      } else if (task.status !== "todo") {
        console.log(
          JSON.stringify({
            error: `Task '${taskId}' is not available (status: ${task.status})`,
          })
        );
      } else if (task.blockedBy) {
        console.log(
          JSON.stringify({
            error: `Task '${taskId}' is blocked by: ${task.blockedBy}`,
          })
        );
      } else {
        console.log(
          JSON.stringify({ error: `Cannot claim task '${taskId}'` })
        );
      }
      process.exit(1);
    }

    const task = state.getTask(taskId);
    console.log(
      JSON.stringify({
        success: true,
        task: {
          id: task?.id,
          name: task?.name,
          status: task?.status,
          claimedBy: task?.claimedBy,
        },
      })
    );
  } finally {
    state.close();
  }
}
