#!/usr/bin/env bun
/**
 * Hirsel Worker CLI Entry Point
 *
 * Commands for AI agents running inside hirsel workers.
 * Used via MCP or direct CLI invocation.
 */

// =============================================================================
// Environment
// =============================================================================

function requireEnv(name: string): string {
  const value = process.env[name];
  if (!value) {
    console.error(`Error: ${name} environment variable is required`);
    console.error("Worker commands must be run inside a hirsel worker context.");
    process.exit(1);
  }
  return value;
}

function getRunName(): string {
  return requireEnv("HIRSEL_RUN");
}

function getWorkerName(): string {
  return requireEnv("HIRSEL_WORKER");
}

// =============================================================================
// Command Interface
// =============================================================================

interface WorkerCommandContext {
  runName: string;
  workerName: string;
  args: string[];
  flags: Record<string, string | boolean>;
}

type WorkerCommandHandler = (ctx: WorkerCommandContext) => Promise<void>;

// =============================================================================
// Argument Parsing
// =============================================================================

interface ParsedArgs {
  command: string;
  subcommand?: string;
  args: string[];
  flags: Record<string, string | boolean>;
}

function parseArgs(argv: string[]): ParsedArgs {
  const args: string[] = [];
  const flags: Record<string, string | boolean> = {};
  let command = "";
  let subcommand: string | undefined;

  let i = 0;
  while (i < argv.length) {
    const arg = argv[i];

    if (arg.startsWith("--")) {
      const key = arg.slice(2);
      const eqIndex = key.indexOf("=");
      if (eqIndex !== -1) {
        flags[key.slice(0, eqIndex)] = key.slice(eqIndex + 1);
      } else if (i + 1 < argv.length && !argv[i + 1].startsWith("-")) {
        flags[key] = argv[i + 1];
        i++;
      } else {
        flags[key] = true;
      }
    } else if (arg.startsWith("-") && arg.length === 2) {
      const key = arg.slice(1);
      if (i + 1 < argv.length && !argv[i + 1].startsWith("-")) {
        flags[key] = argv[i + 1];
        i++;
      } else {
        flags[key] = true;
      }
    } else if (!command) {
      command = arg;
    } else if (!subcommand && command === "task") {
      subcommand = arg;
    } else if (!subcommand && command === "msg") {
      subcommand = arg;
    } else {
      args.push(arg);
    }

    i++;
  }

  return { command, subcommand, args, flags };
}

// =============================================================================
// Help Text
// =============================================================================

function showHelp(): void {
  console.log(`
hirsel-worker - Worker Commands for AI Agents

Usage: hirsel-worker <command> [options]

Task Commands:
  task list                    List all tasks
  task claim <id>              Claim a task to work on
  task done [id]               Mark claimed task as done
  task add <id> <desc>         Add a follow-up task
  task unclaim [id]            Release claimed task
  task await                   Wait for available tasks

Message Commands:
  msg send <thread> <message>  Send message to thread
  msg read [thread]            Read messages from thread
  msg list                     List message threads
  msg inbox                    Check for new messages

Other Commands:
  work-done                    Signal all work complete
  time-status                  Check time remaining

Options:
  --help, -h                   Show this help
  --wait                       Wait for reply (msg send)
  --blocked-by <ids>           Task dependencies (task add)
  --parent <id>                Parent task (task add)

Environment Variables:
  HIRSEL_RUN                   Run name (required)
  HIRSEL_WORKER                Worker name (required)

Examples:
  hirsel-worker task list
  hirsel-worker task claim implement_feature
  hirsel-worker task done
  hirsel-worker msg send user "Need clarification on API design"
`);
}

// =============================================================================
// Placeholder Handlers
// =============================================================================

async function notImplemented(name: string): Promise<void> {
  console.log(`Worker command '${name}' is not yet implemented.`);
}

// =============================================================================
// Task Commands
// =============================================================================

async function taskList(ctx: WorkerCommandContext): Promise<void> {
  await notImplemented("task list");
}

async function taskClaim(ctx: WorkerCommandContext): Promise<void> {
  const taskId = ctx.args[0];
  if (!taskId) {
    console.error("Usage: hirsel-worker task claim <id>");
    process.exit(1);
  }
  await notImplemented("task claim");
}

async function taskDone(ctx: WorkerCommandContext): Promise<void> {
  await notImplemented("task done");
}

async function taskAdd(ctx: WorkerCommandContext): Promise<void> {
  const taskId = ctx.args[0];
  const description = ctx.args.slice(1).join(" ");
  if (!taskId || !description) {
    console.error("Usage: hirsel-worker task add <id> <description>");
    process.exit(1);
  }
  await notImplemented("task add");
}

async function taskUnclaim(ctx: WorkerCommandContext): Promise<void> {
  await notImplemented("task unclaim");
}

async function taskAwait(ctx: WorkerCommandContext): Promise<void> {
  await notImplemented("task await");
}

// =============================================================================
// Message Commands
// =============================================================================

async function msgSend(ctx: WorkerCommandContext): Promise<void> {
  const thread = ctx.args[0];
  const message = ctx.args.slice(1).join(" ");
  if (!thread || !message) {
    console.error("Usage: hirsel-worker msg send <thread> <message>");
    process.exit(1);
  }
  await notImplemented("msg send");
}

async function msgRead(ctx: WorkerCommandContext): Promise<void> {
  await notImplemented("msg read");
}

async function msgList(ctx: WorkerCommandContext): Promise<void> {
  await notImplemented("msg list");
}

async function msgInbox(ctx: WorkerCommandContext): Promise<void> {
  await notImplemented("msg inbox");
}

// =============================================================================
// Other Commands
// =============================================================================

async function workDone(ctx: WorkerCommandContext): Promise<void> {
  await notImplemented("work-done");
}

async function timeStatus(ctx: WorkerCommandContext): Promise<void> {
  await notImplemented("time-status");
}

// =============================================================================
// Main Entry Point
// =============================================================================

async function main(): Promise<void> {
  const argv = process.argv.slice(2);

  if (argv.length === 0 || argv.includes("--help") || argv.includes("-h")) {
    showHelp();
    return;
  }

  const { command, subcommand, args, flags } = parseArgs(argv);

  // Get context (will exit if env vars not set)
  const runName = getRunName();
  const workerName = getWorkerName();
  const ctx: WorkerCommandContext = { runName, workerName, args, flags };

  // Route commands
  switch (command) {
    case "task":
      switch (subcommand) {
        case "list":
          await taskList(ctx);
          break;
        case "claim":
          await taskClaim(ctx);
          break;
        case "done":
          await taskDone(ctx);
          break;
        case "add":
          await taskAdd(ctx);
          break;
        case "unclaim":
          await taskUnclaim(ctx);
          break;
        case "await":
          await taskAwait(ctx);
          break;
        default:
          console.error(`Unknown task subcommand: ${subcommand}`);
          console.error("Run 'hirsel-worker --help' for usage.");
          process.exit(1);
      }
      break;

    case "msg":
      switch (subcommand) {
        case "send":
          await msgSend(ctx);
          break;
        case "read":
          await msgRead(ctx);
          break;
        case "list":
          await msgList(ctx);
          break;
        case "inbox":
          await msgInbox(ctx);
          break;
        default:
          console.error(`Unknown msg subcommand: ${subcommand}`);
          console.error("Run 'hirsel-worker --help' for usage.");
          process.exit(1);
      }
      break;

    case "work-done":
      await workDone(ctx);
      break;

    case "time-status":
      await timeStatus(ctx);
      break;

    default:
      console.error(`Unknown command: ${command}`);
      console.error("Run 'hirsel-worker --help' for usage.");
      process.exit(1);
  }
}

// Run if executed directly
main().catch((error) => {
  console.error(error);
  process.exit(1);
});
