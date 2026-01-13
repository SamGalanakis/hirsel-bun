/**
 * hirsel delete <run> - Remove a run
 */

import { ICONS } from "../index";
import { dim, bold } from "../../shared/theme";
import { existsSync, rmSync } from "fs";
import { join } from "path";
import { homedir } from "os";
import { Database } from "bun:sqlite";

// Get hirsel root directory
function getHirselRoot(): string {
  return process.env.HIRSEL_ROOT || join(homedir(), ".hirsel");
}

// Main command handler
export default async function deleteRun(args: string[]): Promise<void> {
  if (args.length === 0) {
    console.error(`${ICONS.error} Run name required`);
    console.error("Usage: hirsel delete <run>");
    process.exit(1);
  }

  const runName = args[0];
  const runDir = join(getHirselRoot(), "runs", runName);

  if (!existsSync(runDir)) {
    console.error(`${ICONS.error} Run '${runName}' not found`);
    process.exit(1);
  }

  // Check if run is active
  const dbPath = join(runDir, "hirsel.db");
  if (existsSync(dbPath)) {
    const db = new Database(dbPath, { readonly: true });
    const state = db.query("SELECT status FROM state LIMIT 1").get() as { status: string } | null;
    db.close();

    if (state && ["working", "eval", "waiting"].includes(state.status)) {
      console.error(`${ICONS.error} Run '${runName}' is currently active (${state.status})`);
      console.error(dim("Pause it first: hirsel pause " + runName));
      process.exit(1);
    }
  }

  // Delete
  rmSync(runDir, { recursive: true, force: true });
  console.log(`${ICONS.done} Deleted run: ${bold(runName)}`);
}
