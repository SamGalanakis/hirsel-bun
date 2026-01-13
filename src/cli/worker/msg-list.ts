/**
 * hirsel-worker msg-list - List available message threads
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

export default async function msgList(
  args: string[],
  ctx: Context
): Promise<void> {
  const state = new State(getDbPath(ctx.runName));

  try {
    const threads = state.getThreadNames();

    // Get unread count for each thread
    const threadsWithCounts = threads.map((thread) => {
      const unread = state.getUnreadMessages(ctx.workerName, thread);
      return {
        name: thread,
        messageCount: unread.length,
      };
    });

    console.log(JSON.stringify({ threads: threadsWithCounts }));
  } finally {
    state.close();
  }
}
