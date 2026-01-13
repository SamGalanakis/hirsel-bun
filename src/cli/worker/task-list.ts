/**
 * hirsel-worker task-list - List all tasks
 *
 * Returns the current state of all tasks in JSON format.
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

export default async function taskList(
  args: string[],
  ctx: Context
): Promise<void> {
  const state = new State(getDbPath(ctx.runName));

  try {
    const tasks = state.getTasks();

    console.log(
      JSON.stringify({
        tasks: tasks.map((t) => ({
          id: t.id,
          name: t.name,
          status: t.status,
          claimedBy: t.claimedBy,
          parentId: t.parentId,
          blockedBy: t.blockedBy,
        })),
      })
    );
  } finally {
    state.close();
  }
}
