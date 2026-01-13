/**
 * hirsel resume <run> - Resume paused/timed-out run
 *
 * Options:
 *   --time-limit      New time limit for the resumed run
 */

import { parseTimeLimit, ICONS } from "../index";
import { dim, bold } from "../../shared/theme";
import { existsSync } from "fs";
import { join } from "path";
import { homedir } from "os";
import { Database } from "bun:sqlite";

// Get hirsel root directory
function getHirselRoot(): string {
  return process.env.HIRSEL_ROOT || join(homedir(), ".hirsel");
}

// Parse command arguments
interface ResumeOptions {
  runName: string;
  timeLimit: string | null;
}

function parseResumeArgs(args: string[]): ResumeOptions {
  const options: ResumeOptions = {
    runName: "",
    timeLimit: null,
  };

  let i = 0;
  while (i < args.length) {
    const arg = args[i];

    if (arg === "--time-limit" || arg === "-t") {
      options.timeLimit = args[++i] || null;
    } else if (!arg.startsWith("-")) {
      if (!options.runName) {
        options.runName = arg;
      }
    }
    i++;
  }

  return options;
}

// Main command handler
export default async function resume(args: string[]): Promise<void> {
  const options = parseResumeArgs(args);

  if (!options.runName) {
    console.error(`${ICONS.error} Run name required`);
    console.error("Usage: hirsel resume <run> [--time-limit <limit>]");
    process.exit(1);
  }

  const runName = options.runName;
  const dbPath = join(getHirselRoot(), "runs", runName, "hirsel.db");

  if (!existsSync(dbPath)) {
    console.error(`${ICONS.error} Run '${runName}' not found`);
    process.exit(1);
  }

  const db = new Database(dbPath);
  const state = db.query("SELECT status, time_limit_minutes FROM state LIMIT 1").get() as {
    status: string;
    time_limit_minutes: number | null;
  } | null;

  if (!state) {
    console.error(`${ICONS.error} Invalid run state`);
    db.close();
    process.exit(1);
  }

  if (!["paused", "timed_out"].includes(state.status)) {
    console.error(`${ICONS.error} Run '${runName}' is not paused or timed out (${state.status})`);
    db.close();
    process.exit(1);
  }

  // Parse new time limit if provided
  let timeLimitMinutes = state.time_limit_minutes;
  if (options.timeLimit) {
    try {
      timeLimitMinutes = parseTimeLimit(options.timeLimit);
    } catch (error) {
      console.error(`${ICONS.error} ${(error as Error).message}`);
      db.close();
      process.exit(1);
    }
  }

  // Update status
  const now = new Date().toISOString();
  db.run(
    "UPDATE state SET status = 'working', time_limit_minutes = ?, started_at = ?, updated_at = ?",
    [timeLimitMinutes, now, now]
  );
  db.run("INSERT INTO history (action, detail) VALUES (?, ?)", [
    "run resumed",
    timeLimitMinutes ? `time limit: ${timeLimitMinutes}m` : null,
  ]);
  db.close();

  console.log(`${ICONS.done} Resumed run: ${bold(runName)}`);
  if (timeLimitMinutes) {
    console.log(dim(`Time limit: ${timeLimitMinutes} minutes`));
  }
}
