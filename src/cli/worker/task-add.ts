/**
 * hirsel-worker task-add <id> <name> - Add a new task
 *
 * Adds a follow-up task. Optionally specify parent and blocked_by.
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

function validateTaskId(taskId: string): boolean {
  return /^[a-z][a-z0-9_]*$/.test(taskId);
}

export default async function taskAdd(
  args: string[],
  ctx: Context
): Promise<void> {
  if (args.length < 2) {
    console.log(
      JSON.stringify({ error: "Usage: task-add <id> <name> [--parent <id>] [--blocked-by <ids>]" })
    );
    process.exit(1);
  }

  const taskId = args[0];
  let taskName = args[1];
  let parentId: string | null = null;
  let blockedBy: string[] | null = null;

  // Parse optional arguments
  for (let i = 2; i < args.length; i++) {
    if (args[i] === "--parent" && args[i + 1]) {
      parentId = args[i + 1];
      i++;
    } else if (args[i] === "--blocked-by" && args[i + 1]) {
      blockedBy = args[i + 1].split(",").map((s) => s.trim());
      i++;
    } else {
      // Additional words become part of the name
      taskName += " " + args[i];
    }
  }

  // Validate task ID format
  if (!validateTaskId(taskId)) {
    console.log(
      JSON.stringify({
        error: `Invalid task ID: ${taskId}. Must be lowercase, start with letter, use underscores.`,
      })
    );
    process.exit(1);
  }

  const state = new State(getDbPath(ctx.runName));

  try {
    // Check if task already exists
    const existingTask = state.getTask(taskId);
    if (existingTask) {
      console.log(JSON.stringify({ error: `Task '${taskId}' already exists` }));
      process.exit(1);
    }

    // Validate parent exists if specified
    if (parentId) {
      const parent = state.getTask(parentId);
      if (!parent) {
        console.log(
          JSON.stringify({ error: `Parent task '${parentId}' not found` })
        );
        process.exit(1);
      }
    }

    // Validate blocked_by tasks exist
    if (blockedBy) {
      for (const blockerId of blockedBy) {
        const blocker = state.getTask(blockerId);
        if (!blocker) {
          console.log(
            JSON.stringify({ error: `Blocked-by task '${blockerId}' not found` })
          );
          process.exit(1);
        }
      }
    }

    // Add the task
    state.addTask(taskId, taskName, parentId, blockedBy);

    console.log(
      JSON.stringify({
        success: true,
        task: {
          id: taskId,
          name: taskName,
          status: "todo",
          parentId,
          blockedBy: blockedBy?.join(",") || null,
        },
      })
    );
  } finally {
    state.close();
  }
}
