/**
 * hirsel-worker msg-read [thread] - Read messages from a thread
 *
 * Reads unread messages from the specified thread (or all threads).
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

export default async function msgRead(
  args: string[],
  ctx: Context
): Promise<void> {
  const thread = args[0];
  const state = new State(getDbPath(ctx.runName));

  try {
    if (thread) {
      // Read from specific thread
      const messages = state.getUnreadMessages(ctx.workerName, thread);

      if (messages.length > 0) {
        // Mark as read
        const lastId = messages[messages.length - 1].id;
        state.markMessagesRead(ctx.workerName, thread, lastId);
      }

      console.log(
        JSON.stringify({
          messages: messages.map((m) => ({
            thread: m.thread,
            sender: m.sender,
            content: m.content,
            timestamp: m.timestamp,
          })),
        })
      );
    } else {
      // Read from all threads
      const threads = state.getThreadNames();
      const allMessages: Array<{
        thread: string;
        sender: string;
        content: string;
        timestamp: string;
      }> = [];

      for (const t of threads) {
        const messages = state.getUnreadMessages(ctx.workerName, t);
        if (messages.length > 0) {
          const lastId = messages[messages.length - 1].id;
          state.markMessagesRead(ctx.workerName, t, lastId);

          for (const m of messages) {
            allMessages.push({
              thread: m.thread,
              sender: m.sender,
              content: m.content,
              timestamp: m.timestamp,
            });
          }
        }
      }

      console.log(JSON.stringify({ messages: allMessages }));
    }
  } finally {
    state.close();
  }
}
