#!/usr/bin/env bun
/**
 * hirsel-worker - Worker subprocess commands (for AI agents via MCP)
 *
 * These commands are invoked by AI agents to manage tasks and communicate.
 * They expect HIRSEL_RUN and HIRSEL_WORKER environment variables to be set.
 */

// Worker command registry
const WORKER_COMMANDS = {
  "task-claim": () => import("./task-claim"),
  "task-done": () => import("./task-done"),
  "task-add": () => import("./task-add"),
  "task-await": () => import("./task-await"),
  "task-list": () => import("./task-list"),
  "task-unclaim": () => import("./task-unclaim"),
  msg: () => import("./msg"),
  "msg-read": () => import("./msg-read"),
  "msg-list": () => import("./msg-list"),
  "msg-inbox": () => import("./msg-inbox"),
  "work-done": () => import("./work-done"),
  "time-status": () => import("./time-status"),
} as const;

type WorkerCommandName = keyof typeof WORKER_COMMANDS;

const HELP = `
hirsel-worker - Worker commands for AI agents

Usage: hirsel-worker <command> [args...]

Task Commands:
  task-claim <id>         Claim a task to work on
  task-done [id]          Complete current or specified task
  task-add <id> <name>    Add a follow-up task
  task-await              Wait for available tasks
  task-list               List all tasks
  task-unclaim [id]       Release current task without completing

Message Commands:
  msg <thread> <message>  Send message to a thread
  msg-read [thread]       Read messages from thread(s)
  msg-list                List available threads
  msg-inbox               Check for new messages

Session Commands:
  work-done               Signal all work is complete
  time-status             Get time limit status

Environment Variables:
  HIRSEL_RUN      - Name of the current run (required)
  HIRSEL_WORKER   - Name of this worker (required)

Examples:
  hirsel-worker task-claim implement_feature
  hirsel-worker task-done
  hirsel-worker msg group "Need help with database schema"
  hirsel-worker work-done
`;

async function main(): Promise<void> {
  const args = process.argv.slice(2);

  // Handle help
  if (args.includes("--help") || args.includes("-h") || args.length === 0) {
    console.log(HELP);
    process.exit(0);
  }

  // Verify environment
  const runName = process.env.HIRSEL_RUN;
  const workerName = process.env.HIRSEL_WORKER;

  if (!runName) {
    console.error('{"error": "HIRSEL_RUN environment variable not set"}');
    process.exit(1);
  }

  if (!workerName) {
    console.error('{"error": "HIRSEL_WORKER environment variable not set"}');
    process.exit(1);
  }

  // Get command
  const command = args[0] as WorkerCommandName;
  const commandArgs = args.slice(1);

  if (!(command in WORKER_COMMANDS)) {
    console.error(`{"error": "Unknown command: ${command}"}`);
    process.exit(1);
  }

  try {
    const module = await WORKER_COMMANDS[command]();
    await module.default(commandArgs, { runName, workerName });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    console.error(JSON.stringify({ error: message }));
    process.exit(1);
  }
}

export { main };

if (import.meta.main) {
  main().catch((err) => {
    console.error(JSON.stringify({ error: String(err) }));
    process.exit(1);
  });
}
