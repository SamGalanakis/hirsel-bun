/**
 * hirsel deliver <run> [--branch <name>] - Create branch in target repo
 *
 * Pushes the staging branch changes to a named branch in the original project repo.
 * Validates that all work is merged to staging before delivering.
 */

import { ICONS, dim, bold } from "../../shared/theme";
import { existsSync } from "fs";
import { join } from "path";
import { homedir } from "os";
import { Database } from "bun:sqlite";
import { listUnmergedBranches, pushStagingAsBranch, branchExists } from "../../core/git";
import * as readline from "readline";

// Slugify a string for use as run/branch name
function slugify(value: string): string {
  return value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "")
    .substring(0, 50);
}

// Parse command args into positional and flags
function parseArgs(args: string[]): { positional: string[]; flags: Record<string, string | boolean> } {
  const positional: string[] = [];
  const flags: Record<string, string | boolean> = {};

  let i = 0;
  while (i < args.length) {
    const arg = args[i];
    if (arg.startsWith("--")) {
      const key = arg.slice(2);
      const eqIndex = key.indexOf("=");
      if (eqIndex !== -1) {
        flags[key.slice(0, eqIndex)] = key.slice(eqIndex + 1);
      } else if (i + 1 < args.length && !args[i + 1].startsWith("-")) {
        flags[key] = args[i + 1];
        i++;
      } else {
        flags[key] = true;
      }
    } else if (arg.startsWith("-") && arg.length === 2) {
      const key = arg.slice(1);
      if (i + 1 < args.length && !args[i + 1].startsWith("-")) {
        flags[key] = args[i + 1];
        i++;
      } else {
        flags[key] = true;
      }
    } else {
      positional.push(arg);
    }
    i++;
  }

  return { positional, flags };
}

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

  return null;
}

// Simple confirmation prompt
async function confirm(message: string): Promise<boolean> {
  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout,
  });

  return new Promise((resolve) => {
    rl.question(`${message} (y/N) `, (answer) => {
      rl.close();
      resolve(answer.toLowerCase() === "y" || answer.toLowerCase() === "yes");
    });
  });
}

// Main command handler
export default async function deliver(args: string[]): Promise<void> {
  const { positional, flags } = parseArgs(args);

  if (positional.length === 0) {
    console.error(`${ICONS.error} Run name required`);
    console.error("Usage: hirsel deliver <run> [--branch <name>]");
    process.exit(1);
  }

  const runName = positional[0];
  const customBranch = flags.branch as string | undefined;
  const runDir = join(getHirselRoot(), "runs", runName);
  const dbPath = join(runDir, "hirsel.db");

  if (!existsSync(dbPath)) {
    console.error(`${ICONS.error} Run '${runName}' not found`);
    process.exit(1);
  }

  // Get project path and status from database
  const db = new Database(dbPath);
  const state = db.query("SELECT project_path, status FROM state LIMIT 1").get() as {
    project_path: string | null;
    status: string;
  } | null;

  if (!state?.project_path) {
    console.error(`${ICONS.error} No project path found for run`);
    db.close();
    process.exit(1);
  }

  const projectPath = state.project_path;

  // Check that run is not active
  if (["working", "eval", "waiting"].includes(state.status)) {
    console.error(`${ICONS.error} Run '${runName}' is still active (${state.status})`);
    console.error(dim("Pause it first: hirsel pause " + runName));
    db.close();
    process.exit(1);
  }

  // Check project path exists
  if (!existsSync(projectPath)) {
    console.error(`${ICONS.error} Project path does not exist: ${projectPath}`);
    db.close();
    process.exit(1);
  }

  // Find work directory
  const workDir = findWorkDir(runDir);
  if (!workDir) {
    console.error(`${ICONS.error} No git repository found in work directory`);
    db.close();
    process.exit(1);
  }

  // Check for unmerged branches
  try {
    const unmerged = await listUnmergedBranches(workDir);
    if (unmerged.length > 0) {
      const branchList = unmerged.join(", ");
      console.error(`${ICONS.error} Unmerged branches exist: ${branchList}`);
      console.error(dim("All work must be merged to 'staging' before delivering."));
      console.error(dim("Use 'hirsel view' to check worker status."));
      db.close();
      process.exit(1);
    }
  } catch (error) {
    // Warn but continue - might be a different git layout
    console.warn(dim(`Warning: Could not check for unmerged branches: ${(error as Error).message}`));
  }

  // Generate branch name
  const branchName = customBranch || `hirsel/${slugify(runName)}`;

  // Check if branch already exists in project
  try {
    if (await branchExists(projectPath, branchName)) {
      console.warn(`${ICONS.waiting} Branch '${branchName}' already exists in project`);
      const shouldOverwrite = await confirm("Overwrite?");
      if (!shouldOverwrite) {
        console.log(dim("Cancelled."));
        db.close();
        process.exit(0);
      }
    }
  } catch {
    // Ignore - branch doesn't exist or couldn't check
  }

  console.log(bold(`\nDelivering ${runName}\n`));
  console.log(dim(`Branch: ${branchName}`));
  console.log(dim(`Target: ${projectPath}`));
  console.log();

  try {
    // Push staging as new branch
    await pushStagingAsBranch(workDir, projectPath, branchName);

    // Update state
    const now = new Date().toISOString();
    db.run("UPDATE state SET status = 'delivered', updated_at = ?", [now]);
    db.run("INSERT INTO history (action, detail) VALUES (?, ?)", ["run delivered", branchName]);
    db.close();

    console.log(`${ICONS.done} Delivered to branch: ${bold(branchName)}`);
    console.log();
    console.log(dim("To review:"));
    console.log(dim(`  git checkout ${branchName}`));
    console.log();
    console.log(dim("To merge:"));
    console.log(dim(`  git checkout main && git merge ${branchName}`));
    console.log();
    console.log(dim("To cleanup:"));
    console.log(dim(`  hirsel delete ${runName}`));
  } catch (error) {
    db.close();
    console.error(`${ICONS.error} Failed to deliver: ${(error as Error).message}`);
    process.exit(1);
  }
}
