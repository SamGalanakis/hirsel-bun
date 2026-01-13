/**
 * hirsel-worker msg <thread> <message> - Send a message
 *
 * Sends a message to the specified thread.
 * Common threads: user, group, learnings, <worker-name>
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

export default async function msg(
  args: string[],
  ctx: Context
): Promise<void> {
  if (args.length < 2) {
    console.log(
      JSON.stringify({ error: "Usage: msg <thread> <message>" })
    );
    process.exit(1);
  }

  const thread = args[0];
  const message = args.slice(1).join(" ");

  if (!message.trim()) {
    console.log(JSON.stringify({ error: "Message cannot be empty" }));
    process.exit(1);
  }

  const state = new State(getDbPath(ctx.runName));

  try {
    const messageId = state.addMessage(thread, ctx.workerName, message);

    console.log(
      JSON.stringify({
        success: true,
        thread,
        messageId,
      })
    );
  } finally {
    state.close();
  }
}
