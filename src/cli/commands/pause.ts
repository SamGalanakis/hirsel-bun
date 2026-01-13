/**
 * hirsel pause <run> - Pause workers
 */

import { ICONS } from "../index";
import { dim, bold } from "../../shared/theme";
import { existsSync } from "fs";
import { join } from "path";
import { homedir } from "os";
import { Database } from "bun:sqlite";

// Get hirsel root directory
function getHirselRoot(): string {
  return process.env.HIRSEL_ROOT || join(homedir(), ".hirsel");
}

// Main command handler
export default async function pause(args: string[]): Promise<void> {
  if (args.length === 0) {
    console.error(`${ICONS.error} Run name required`);
    console.error("Usage: hirsel pause <run>");
    process.exit(1);
  }

  const runName = args[0];
  const dbPath = join(getHirselRoot(), "runs", runName, "hirsel.db");

  if (!existsSync(dbPath)) {
    console.error(`${ICONS.error} Run '${runName}' not found`);
    process.exit(1);
  }

  const db = new Database(dbPath);
  const state = db.query("SELECT status FROM state LIMIT 1").get() as { status: string } | null;

  if (!state) {
    console.error(`${ICONS.error} Invalid run state`);
    db.close();
    process.exit(1);
  }

  if (state.status === "paused") {
    console.log(dim(`Run '${runName}' is already paused`));
    db.close();
    return;
  }

  if (!["working", "waiting", "eval"].includes(state.status)) {
    console.error(`${ICONS.error} Run '${runName}' is not active (${state.status})`);
    db.close();
    process.exit(1);
  }

  // Update status
  const now = new Date().toISOString();
  db.run("UPDATE state SET status = 'paused', updated_at = ?", [now]);
  db.run("INSERT INTO history (action, detail) VALUES (?, ?)", ["run paused", null]);
  db.close();

  console.log(`${ICONS.done} Paused run: ${bold(runName)}`);
  console.log(dim(`Resume with: hirsel resume ${runName}`));
}
