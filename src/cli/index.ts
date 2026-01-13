#!/usr/bin/env bun
/**
 * Hirsel CLI Entry Point
 *
 * Command router with argument parsing, help, and version.
 * Dispatches to individual command handlers.
 */

import { config, AGENT_PRESETS } from "../core/config";
import runs from "./commands/runs";
import attach from "./commands/attach";
import pauseCommand from "./commands/pause";
import resumeCommand from "./commands/resume";
import deleteCommand from "./commands/delete";
import pruneCommand from "./commands/prune";
import msg from "./commands/msg";
import configCommand from "./commands/config";
import templates from "./commands/templates";
import completions from "./commands/completions";
import spec from "./commands/spec";
import improve from "./commands/improve";
import man from "./commands/man";
import summary from "./commands/summary";
import tasks, {
  taskAddCommand,
  taskDeleteCommand,
  taskDoneCommand,
  taskReopenCommand,
  taskUnclaimCommand,
} from "./commands/tasks";
import go from "./commands/go";
import view from "./commands/view";
import log from "./commands/log";
import diff from "./commands/diff";
import deliver from "./commands/deliver";

// Re-export utilities for command modules
export { ICONS } from "../shared/theme";

/**
 * Slugify a string for use as a run name
 */
export function slugify(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "")
    .slice(0, 50);
}

/**
 * Parse time limit string (e.g., "30m", "1h", "1h30m")
 */
export function parseTimeLimit(spec: string): number {
  spec = spec.trim().toLowerCase();

  // Just minutes: "30"
  if (/^\d+$/.test(spec)) {
    return parseInt(spec, 10);
  }

  // Minutes: "30m"
  const minMatch = spec.match(/^(\d+)m$/);
  if (minMatch) {
    return parseInt(minMatch[1], 10);
  }

  // Hours: "1h"
  const hourMatch = spec.match(/^(\d+)h$/);
  if (hourMatch) {
    return parseInt(hourMatch[1], 10) * 60;
  }

  // Hours and minutes: "1h30m"
  const hhmMatch = spec.match(/^(\d+)h(\d+)m$/);
  if (hhmMatch) {
    return parseInt(hhmMatch[1], 10) * 60 + parseInt(hhmMatch[2], 10);
  }

  throw new Error(`Invalid time limit: ${spec}. Use: 30, 30m, 1h, or 1h30m`);
}

/**
 * Validate task ID format
 */
export function validateTaskId(id: string): boolean {
  return /^[a-z][a-z0-9_]*$/.test(id);
}

// JSON output tracking
let _jsonOutput = false;

export function setJsonOutput(value: boolean): void {
  _jsonOutput = value;
}

export function isJsonOutput(): boolean {
  return _jsonOutput;
}

export function jsonOutput(data: unknown): void {
  console.log(JSON.stringify(data, null, 2));
}

// =============================================================================
// Version
// =============================================================================

const VERSION = "0.1.0";

// =============================================================================
// Command Interface
// =============================================================================

export interface CommandContext {
  args: string[];
  flags: Record<string, string | boolean>;
}

export type CommandHandler = (ctx: CommandContext) => Promise<void>;

// =============================================================================
// Command Registry
// =============================================================================

interface CommandDef {
  name: string;
  description: string;
  usage?: string;
  handler: CommandHandler;
}

const commands: Map<string, CommandDef> = new Map();

export function registerCommand(def: CommandDef): void {
  commands.set(def.name, def);
}

// =============================================================================
// Argument Parsing
// =============================================================================

interface ParsedArgs {
  command: string;
  args: string[];
  flags: Record<string, string | boolean>;
}

function parseArgs(argv: string[]): ParsedArgs {
  const args: string[] = [];
  const flags: Record<string, string | boolean> = {};
  let command = "";

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
    } else {
      args.push(arg);
    }

    i++;
  }

  return { command, args, flags };
}

// =============================================================================
// Help Text
// =============================================================================

function showHelp(): void {
  console.log(`
hirsel - AI Agent Orchestration

Usage: hirsel [command] [options]

Commands:
  go <run> <spec>      Start a new run
  view <run>           View run status
  log <run>            View activity log
  attach <run>         Watch worker live
  msg <run> <msg>      Send message to run
  diff <run>           Show code changes
  deliver <run>        Create branch in target repo
  pause <run>          Pause workers
  resume <run>         Resume paused/timed-out run
  delete <run>         Remove a run
  prune                Remove all delivered runs
  runs                 List all runs

  tasks <run>          List tasks
  task-add <run> <id> <desc>    Add task
  task-delete <run> <id>        Delete task
  task-done <run> <id>          Mark task done
  task-reopen <run> <id>        Reopen task
  task-unclaim <run> <id>       Unclaim task

  config               Interactive agent selection
  completions          Shell completion scripts
  man                  Show manual

Options:
  --help, -h           Show this help
  --version, -v        Show version
  --json               Output as JSON (where applicable)

Examples:
  hirsel go myproject spec.md --workers 2
  hirsel view myproject
  hirsel attach myproject
  hirsel deliver myproject --branch feature/my-changes
`);
}

function showVersion(): void {
  console.log(`hirsel ${VERSION}`);
}

// =============================================================================
// Command Registration
// =============================================================================

// Register core commands
registerCommand({
  name: "go",
  description: "Start a new run",
  usage: "go <run> <spec> [--workers N] [--time-limit 30m]",
  handler: async (ctx) => go(ctx.args),
});

registerCommand({
  name: "view",
  description: "View run status",
  usage: "view <run>",
  handler: async (ctx) => {
    if (ctx.flags.json) setJsonOutput(true);
    await view(ctx.args);
  },
});

registerCommand({
  name: "log",
  description: "View activity log",
  usage: "log <run> [-f]",
  handler: async (ctx) => {
    if (ctx.flags.json) setJsonOutput(true);
    await log(ctx.args);
  },
});

registerCommand({
  name: "diff",
  description: "Show code changes",
  usage: "diff <run>",
  handler: async (ctx) => diff(ctx.args),
});

registerCommand({
  name: "deliver",
  description: "Create branch in target repo",
  usage: "deliver <run> [--branch name]",
  handler: async (ctx) => deliver(ctx.args),
});

// Register implemented commands
registerCommand({
  name: "runs",
  description: "List all runs",
  usage: "runs [--json]",
  handler: async (ctx) => {
    if (ctx.flags.json) setJsonOutput(true);
    await runs(ctx.args);
  },
});

registerCommand({
  name: "attach",
  description: "Watch worker live",
  usage: "attach <run> [worker]",
  handler: async (ctx) => attach(ctx.args),
});

// Task management commands
registerCommand({
  name: "tasks",
  description: "List tasks",
  usage: "tasks <run>",
  handler: async (ctx) => tasks(ctx.args),
});

registerCommand({
  name: "task-add",
  description: "Add task",
  usage: "task-add <run> <id> <desc>",
  handler: async (ctx) => taskAddCommand(ctx.args),
});

registerCommand({
  name: "task-delete",
  description: "Delete task",
  usage: "task-delete <run> <id>",
  handler: async (ctx) => taskDeleteCommand(ctx.args),
});

registerCommand({
  name: "task-done",
  description: "Mark task done",
  usage: "task-done <run> <id>",
  handler: async (ctx) => taskDoneCommand(ctx.args),
});

registerCommand({
  name: "task-reopen",
  description: "Reopen task",
  usage: "task-reopen <run> <id>",
  handler: async (ctx) => taskReopenCommand(ctx.args),
});

registerCommand({
  name: "task-unclaim",
  description: "Unclaim task",
  usage: "task-unclaim <run> <id>",
  handler: async (ctx) => taskUnclaimCommand(ctx.args),
});

// Run control commands
registerCommand({
  name: "pause",
  description: "Pause workers",
  usage: "pause <run>",
  handler: async (ctx) => pauseCommand(ctx.args),
});

registerCommand({
  name: "resume",
  description: "Resume run",
  usage: "resume <run> [--time-limit 30m]",
  handler: async (ctx) => resumeCommand(ctx.args),
});

registerCommand({
  name: "delete",
  description: "Remove a run",
  usage: "delete <run>",
  handler: async (ctx) => deleteCommand(ctx.args),
});

registerCommand({
  name: "prune",
  description: "Remove all delivered runs",
  usage: "prune",
  handler: async (ctx) => pruneCommand(ctx.args),
});

// Messaging command
registerCommand({
  name: "msg",
  description: "Send message to run",
  usage: "msg <run> <message>",
  handler: async (ctx) => msg(ctx.args),
});

// Configuration
registerCommand({
  name: "config",
  description: "Configure agent",
  usage: "config [agent]",
  handler: async (ctx) => configCommand(ctx.args),
});

// Misc commands
registerCommand({
  name: "templates",
  description: "Manage templates",
  usage: "templates [--json]",
  handler: async (ctx) => {
    if (ctx.flags.json) setJsonOutput(true);
    await templates(ctx.args);
  },
});

registerCommand({
  name: "completions",
  description: "Shell completions",
  usage: "completions [bash|zsh]",
  handler: async (ctx) => completions(ctx.args),
});

registerCommand({
  name: "spec",
  description: "Spec management",
  usage: "spec <run>",
  handler: async (ctx) => spec(ctx.args),
});

registerCommand({
  name: "improve",
  description: "Update project memory",
  usage: "improve <run>",
  handler: async (ctx) => improve(ctx.args),
});

registerCommand({
  name: "man",
  description: "Show manual",
  usage: "man",
  handler: async (ctx) => man(ctx.args),
});

registerCommand({
  name: "summary",
  description: "Generate run summary",
  usage: "summary <run> [--json]",
  handler: async (ctx) => {
    if (ctx.flags.json) setJsonOutput(true);
    await summary(ctx.args);
  },
});

// =============================================================================
// Main Entry Point
// =============================================================================

async function main(): Promise<void> {
  // Skip 'bun' and script path
  const argv = process.argv.slice(2);

  // Handle no args - launch desktop app or show help
  if (argv.length === 0) {
    // TODO: Launch Electrobun desktop app
    showHelp();
    return;
  }

  const { command, args, flags } = parseArgs(argv);

  // Handle global flags
  if (flags.help || flags.h || command === "help") {
    showHelp();
    return;
  }

  if (flags.version || flags.v) {
    showVersion();
    return;
  }

  // Find and execute command
  const cmdDef = commands.get(command);
  if (!cmdDef) {
    console.error(`Unknown command: ${command}`);
    console.error("Run 'hirsel --help' for usage.");
    process.exit(1);
  }

  try {
    await cmdDef.handler({ args, flags });
  } catch (error) {
    if (error instanceof Error) {
      console.error(`Error: ${error.message}`);
    } else {
      console.error("An unexpected error occurred");
    }
    process.exit(1);
  }
}

// Run if executed directly
main().catch((error) => {
  console.error(error);
  process.exit(1);
});
