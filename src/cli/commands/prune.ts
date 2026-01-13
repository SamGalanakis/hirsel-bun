/**
 * hirsel prune - Remove all delivered runs
 *
 * Cleans up runs that have been delivered to free disk space.
 */

import { ICONS } from "../index";
import { dim, bold } from "../../shared/theme";
import { existsSync, readdirSync, rmSync } from "fs";
import { join } from "path";
import { homedir } from "os";
import { Database } from "bun:sqlite";

// Get hirsel root directory
function getHirselRoot(): string {
  return process.env.HIRSEL_ROOT || join(homedir(), ".hirsel");
}

// Main command handler
export default async function prune(args: string[]): Promise<void> {
  const runsDir = join(getHirselRoot(), "runs");

  if (!existsSync(runsDir)) {
    console.log(dim("No runs to prune"));
    return;
  }

  const runDirs = readdirSync(runsDir, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => d.name);

  if (runDirs.length === 0) {
    console.log(dim("No runs to prune"));
    return;
  }

  const toDelete: string[] = [];

  for (const runName of runDirs) {
    const dbPath = join(runsDir, runName, "hirsel.db");
    if (!existsSync(dbPath)) continue;

    try {
      const db = new Database(dbPath, { readonly: true });
      const state = db.query("SELECT status FROM state LIMIT 1").get() as { status: string } | null;
      db.close();

      if (state && state.status === "delivered") {
        toDelete.push(runName);
      }
    } catch {
      // Skip runs with invalid databases
    }
  }

  if (toDelete.length === 0) {
    console.log(dim("No delivered runs to prune"));
    return;
  }

  console.log(bold(`\nPruning ${toDelete.length} delivered run(s):\n`));

  for (const runName of toDelete) {
    const runDir = join(runsDir, runName);
    console.log(dim(`  ${runName}`));
    rmSync(runDir, { recursive: true, force: true });
  }

  console.log();
  console.log(`${ICONS.done} Pruned ${toDelete.length} run(s)`);
}
