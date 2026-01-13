/**
 * hirsel attach <run> [worker] - Watch worker live
 *
 * Attaches to a worker's session to see live output.
 * If no worker specified, shows a selection menu.
 */

import { ICONS } from "../index";
import { dim, bold, colorize } from "../../shared/theme";
import { existsSync } from "fs";
import { join } from "path";
import { homedir } from "os";
import { Database } from "bun:sqlite";
import { spawn } from "bun";

// Get hirsel root directory
function getHirselRoot(): string {
  return process.env.HIRSEL_ROOT || join(homedir(), ".hirsel");
}

// Get workers for a run
interface Worker {
  name: string;
  status: string;
  session_id: string | null;
  location: string;
}

function getWorkers(db: Database): Worker[] {
  return db.query(`
    SELECT name, status, session_id, location
    FROM workers
    ORDER BY created_at ASC
  `).all() as Worker[];
}

// Simple worker selection
async function selectWorker(workers: Worker[]): Promise<string | null> {
  if (workers.length === 0) {
    return null;
  }

  if (workers.length === 1) {
    return workers[0].name;
  }

  console.log(bold("Select Worker\n"));

  for (let i = 0; i < workers.length; i++) {
    const w = workers[i];
    const marker = w.status === "working" ? colorize(ICONS.working, "yellow") :
                   w.status === "waiting" ? colorize(ICONS.waiting, "brightYellow") :
                   dim(ICONS.idle);
    const location = w.location !== "local" ? dim(` [${w.location}]`) : "";
    console.log(`  ${marker} ${i + 1} ${bold(w.name.padEnd(12))} ${dim(w.status)}${location}`);
  }

  console.log();
  process.stdout.write("> ");

  // Read input
  const reader = Bun.stdin.stream().getReader();
  const decoder = new TextDecoder();

  try {
    const { value, done } = await reader.read();
    if (done || !value) {
      return null;
    }

    const input = decoder.decode(value).trim();

    // Check for number input
    if (/^\d+$/.test(input)) {
      const idx = parseInt(input, 10) - 1;
      if (idx >= 0 && idx < workers.length) {
        return workers[idx].name;
      }
    }

    // Check for name input
    const found = workers.find(w => w.name === input);
    if (found) {
      return found.name;
    }

    return null;
  } finally {
    reader.releaseLock();
  }
}

// Main command handler
export default async function attach(args: string[]): Promise<void> {
  if (args.length === 0) {
    console.error(`${ICONS.error} Run name required`);
    console.error("Usage: hirsel attach <run> [worker]");
    process.exit(1);
  }

  const runName = args[0];
  const workerArg = args[1];
  const dbPath = join(getHirselRoot(), "runs", runName, "hirsel.db");

  if (!existsSync(dbPath)) {
    console.error(`${ICONS.error} Run '${runName}' not found`);
    process.exit(1);
  }

  const db = new Database(dbPath, { readonly: true });
  const workers = getWorkers(db);
  db.close();

  if (workers.length === 0) {
    console.error(`${ICONS.error} No workers in run '${runName}'`);
    process.exit(1);
  }

  // Select worker
  let workerName: string | null = workerArg;

  if (!workerName) {
    workerName = await selectWorker(workers);
    if (!workerName) {
      console.log(dim("No worker selected"));
      return;
    }
  }

  // Find the worker
  const worker = workers.find(w => w.name === workerName);
  if (!worker) {
    console.error(`${ICONS.error} Worker '${workerName}' not found`);
    process.exit(1);
  }

  if (!worker.session_id) {
    console.error(`${ICONS.error} Worker '${workerName}' has no active session`);
    process.exit(1);
  }

  console.log(bold(`\nAttaching to ${workerName}...\n`));
  console.log(dim("Press Ctrl+C to detach"));
  console.log();

  // Attach using tmux (workers run in tmux sessions)
  try {
    const proc = spawn(["tmux", "attach-session", "-t", worker.session_id], {
      stdin: "inherit",
      stdout: "inherit",
      stderr: "inherit",
    });

    await proc.exited;
  } catch (error) {
    console.error(`${ICONS.error} Failed to attach: ${(error as Error).message}`);
    console.log(dim("Note: Workers run in tmux sessions. Make sure tmux is installed."));
    process.exit(1);
  }
}
