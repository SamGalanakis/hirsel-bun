/**
 * hirsel-worker msg-inbox - Check for new messages since session started
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

export default async function msgInbox(
  args: string[],
  ctx: Context
): Promise<void> {
  const state = new State(getDbPath(ctx.runName));

  try {
    const threads = state.getThreadNames();
    const inbox: Array<{
      thread: string;
      count: number;
    }> = [];

    let totalUnread = 0;

    for (const thread of threads) {
      const unread = state.getUnreadMessages(ctx.workerName, thread);
      if (unread.length > 0) {
        inbox.push({
          thread,
          count: unread.length,
        });
        totalUnread += unread.length;
      }
    }

    if (totalUnread === 0) {
      console.log(
        JSON.stringify({
          inbox: [],
          message: "No new messages since session started",
        })
      );
    } else {
      console.log(JSON.stringify({ inbox }));
    }
  } finally {
    state.close();
  }
}
