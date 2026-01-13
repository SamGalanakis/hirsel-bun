/**
 * hirsel diff <run> - Show code changes
 *
 * Shows git diff between the original project HEAD and the work directory staging branch.
 * This gives an accurate view of all changes made by workers during the run.
 */

import { ICONS, dim, bold } from "../../shared/theme";
import { existsSync, readdirSync } from "fs";
import { join } from "path";
import { homedir } from "os";
import { Database } from "bun:sqlite";
import { getDiff, getDiffStat } from "../../core/git";

// Get hirsel root directory
function getHirselRoot(): string {
  return process.env.HIRSEL_ROOT || join(homedir(), ".hirsel");
}

// Find a work directory with a git repository
function findWorkDir(runDir: string): string | null {
  // Prefer staging directory (multi-worker mode)
  const stagingDir = join(runDir, "work", "staging");
  if (existsSync(stagingDir) && existsSync(join(stagingDir, ".git"))) {
    return stagingDir;
  }

  // Check work directory directly (single-worker mode)
  const workDir = join(runDir, "work");
  if (existsSync(workDir) && existsSync(join(workDir, ".git"))) {
    return workDir;
  }

  // Try to find any subdirectory with .git (legacy runs)
  if (existsSync(workDir)) {
    try {
      const subdirs = readdirSync(workDir, { withFileTypes: true })
        .filter((d) => d.isDirectory() && existsSync(join(workDir, d.name, ".git")))
        .map((d) => join(workDir, d.name));
      if (subdirs.length > 0) {
        return subdirs[0];
      }
    } catch {
      // Ignore errors
    }
  }

  return null;
}

// Main command handler
export default async function diff(args: string[]): Promise<void> {
  if (args.length === 0) {
    console.error(`${ICONS.error} Run name required`);
    console.error("Usage: hirsel diff <run>");
    process.exit(1);
  }

  const runName = args[0];
  const runDir = join(getHirselRoot(), "runs", runName);
  const dbPath = join(runDir, "hirsel.db");

  if (!existsSync(dbPath)) {
    console.error(`${ICONS.error} Run '${runName}' not found`);
    process.exit(1);
  }

  // Get project path from database
  const db = new Database(dbPath, { readonly: true });
  const state = db.query("SELECT project_path FROM state LIMIT 1").get() as {
    project_path: string | null;
  } | null;
  db.close();

  if (!state?.project_path) {
    console.error(`${ICONS.error} No project path found for run`);
    process.exit(1);
  }

  const projectPath = state.project_path;
  if (!existsSync(projectPath)) {
    console.error(`${ICONS.error} Project path does not exist: ${projectPath}`);
    process.exit(1);
  }

  // Find work directory
  const workDir = findWorkDir(runDir);
  if (!workDir) {
    console.error(`${ICONS.error} No git repository found in work directory`);
    process.exit(1);
  }

  console.log(bold(`\nChanges in ${runName}\n`));
  console.log(dim(`Comparing: ${projectPath} HEAD...work staging`));
  console.log();

  try {
    // Get diff stat (summary of changes)
    const stat = await getDiffStat(projectPath, workDir);

    if (!stat.trim()) {
      console.log(dim(`${ICONS.idle} No differences between HEAD and work directory`));
      return;
    }

    console.log(dim("Summary:"));
    console.log(stat);
    console.log();

    // Get full diff
    const fullDiff = await getDiff(projectPath, workDir);

    if (fullDiff.trim()) {
      console.log(dim("─".repeat(40)));
      console.log(fullDiff);
    }
  } catch (error) {
    console.error(`${ICONS.error} Failed to get diff: ${(error as Error).message}`);
    process.exit(1);
  }
}
