/**
 * hirsel go <run> <spec> - Start a new run
 *
 * Creates a new run with the given spec file or content.
 *
 * Options:
 *   --workers N       Fixed N workers
 *   --workers 1-5     Autoscale between 1 and 5
 *   --workers 2+      Autoscale from 2 with no upper limit
 *   --time-limit      Time limit (e.g., 30m, 1h, 1h30m)
 *   --project <path>  Project path (default: current directory)
 *   --sandbox         Run in sandbox mode
 *   --yolo            Disable human-in-the-loop
 *   --template <name> Use a template
 */

import { slugify, parseTimeLimit, ICONS, isJsonOutput, jsonOutput } from "../index";
import { LOGO, dim, bold, ditherBar } from "../../shared/theme";
import { existsSync, mkdirSync, readFileSync, writeFileSync, rmSync } from "fs";
import { join, resolve } from "path";
import { homedir } from "os";
import { Database } from "bun:sqlite";
import { $ } from "bun";
import {
  createWorkspace,
  createWorkerClone,
  getRepoRoot,
} from "../../core/git";
import { getAvailableNames } from "../../core/workers";
import { State } from "../../core/state";
import { Status } from "../../shared/types";

// Get hirsel root directory
function getHirselRoot(): string {
  return process.env.HIRSEL_ROOT || join(homedir(), ".hirsel");
}

// Parse worker scale specification
interface WorkerScale {
  min: number;
  max: number | null;
  autoscale: boolean;
}

function parseWorkerScale(spec: string): WorkerScale {
  const trimmed = spec.trim();

  // Fixed count: "3"
  if (/^\d+$/.test(trimmed)) {
    const count = parseInt(trimmed, 10);
    return { min: count, max: count, autoscale: false };
  }

  // Range: "1-5"
  const rangeMatch = trimmed.match(/^(\d+)-(\d+)$/);
  if (rangeMatch) {
    return {
      min: parseInt(rangeMatch[1], 10),
      max: parseInt(rangeMatch[2], 10),
      autoscale: true,
    };
  }

  // Open-ended: "2+"
  const openMatch = trimmed.match(/^(\d+)\+$/);
  if (openMatch) {
    return {
      min: parseInt(openMatch[1], 10),
      max: null,
      autoscale: true,
    };
  }

  throw new Error(`Invalid workers specification: ${spec}. Use: 3, 1-5, or 2+`);
}

// Parse command arguments
interface GoOptions {
  runName: string;
  spec: string | null;
  workers: string;
  timeLimit: string | null;
  project: string;
  sandbox: boolean;
  yolo: boolean;
  template: string | null;
}

function parseGoArgs(args: string[]): GoOptions {
  const options: GoOptions = {
    runName: "",
    spec: null,
    workers: "1",
    timeLimit: null,
    project: ".",
    sandbox: false,
    yolo: false,
    template: null,
  };

  let i = 0;
  while (i < args.length) {
    const arg = args[i];

    if (arg === "--workers" || arg === "-w") {
      options.workers = args[++i] || "1";
    } else if (arg === "--time-limit" || arg === "-t") {
      options.timeLimit = args[++i] || null;
    } else if (arg === "--project" || arg === "-p") {
      options.project = args[++i] || ".";
    } else if (arg === "--sandbox") {
      options.sandbox = true;
    } else if (arg === "--yolo") {
      options.yolo = true;
    } else if (arg === "--template") {
      options.template = args[++i] || null;
    } else if (!arg.startsWith("-")) {
      if (!options.runName) {
        options.runName = arg;
      } else if (!options.spec) {
        options.spec = arg;
      }
    }
    i++;
  }

  return options;
}

// Resolve content from file path or direct content
function resolveContent(value: string | null): string | null {
  if (!value) return null;

  // Check if it looks like a file path
  if (value.includes("/") || value.endsWith(".md") || value.endsWith(".txt")) {
    if (existsSync(value)) {
      return readFileSync(value, "utf-8");
    }
    throw new Error(`File not found: ${value}`);
  }

  return value;
}

// Spawn a worker in tmux
async function spawnWorkerInTmux(
  runName: string,
  workerName: string,
  workDir: string,
  isLeader: boolean
): Promise<number | null> {
  const sessionName = `hirsel-${runName}-${workerName}`;

  // Build environment for the worker
  const env = [
    `HIRSEL_RUN=${runName}`,
    `HIRSEL_WORKER=${workerName}`,
  ].join(" ");

  // Build the command - use hirsel-worker CLI
  const workerCmd = isLeader
    ? `${env} claude --dangerously-skip-permissions`
    : `${env} claude --dangerously-skip-permissions`;

  try {
    // Create tmux session
    await $`tmux new-session -d -s ${sessionName} -c ${workDir}`.quiet();

    // Send the worker command to the tmux session
    await $`tmux send-keys -t ${sessionName} ${workerCmd} Enter`.quiet();

    // Get the PID of the worker process
    const pidResult =
      await $`tmux list-panes -t ${sessionName} -F "#{pane_pid}"`.quiet();
    const pid = parseInt(pidResult.stdout.toString().trim(), 10);

    return isNaN(pid) ? null : pid;
  } catch (error) {
    console.error(
      dim(`Failed to spawn worker ${workerName}: ${(error as Error).message}`)
    );
    return null;
  }
}

// Main command handler
export default async function go(args: string[]): Promise<void> {
  const options = parseGoArgs(args);

  if (!options.runName) {
    console.error(`${ICONS.error} Run name required`);
    console.error("Usage: hirsel go <run> <spec> [options]");
    process.exit(1);
  }

  // Slugify run name
  const runName = slugify(options.runName);
  if (runName.length > 50) {
    console.error(`${ICONS.error} Run name too long (max 50 chars)`);
    process.exit(1);
  }

  // Parse worker scale
  let scale: WorkerScale;
  try {
    scale = parseWorkerScale(options.workers);
  } catch (error) {
    console.error(`${ICONS.error} ${(error as Error).message}`);
    process.exit(1);
  }

  // Resolve project path
  let projectPath: string;
  try {
    projectPath = await getRepoRoot(resolve(options.project));
  } catch (error) {
    console.error(`${ICONS.error} ${(error as Error).message}`);
    process.exit(1);
  }

  // Setup run directory
  const root = getHirselRoot();
  const runDir = join(root, "runs", runName);

  // Check for existing run
  if (existsSync(runDir)) {
    const dbPath = join(runDir, "hirsel.db");
    if (existsSync(dbPath)) {
      const db = new Database(dbPath, { readonly: true });
      const row = db.query("SELECT status FROM state WHERE id = 1").get() as {
        status: string;
      } | null;
      db.close();

      if (row && ["working", "eval", "waiting"].includes(row.status)) {
        console.error(
          `${ICONS.error} Run '${runName}' is currently active (${row.status})`
        );
        console.error(dim(`View: hirsel view ${runName}`));
        console.error(dim(`Pause: hirsel pause ${runName}`));
        process.exit(1);
      }
    }

    // Clean up old run
    console.log(dim("Removing old run..."));
    rmSync(runDir, { recursive: true, force: true });
  }

  // Resolve spec content
  let specContent: string | null = null;
  try {
    specContent = resolveContent(options.spec);
  } catch (error) {
    console.error(`${ICONS.error} ${(error as Error).message}`);
    process.exit(1);
  }

  if (!specContent && !options.template) {
    console.error(
      `${ICONS.error} Spec required (provide file path or --template)`
    );
    process.exit(1);
  }

  // Create run directory structure
  mkdirSync(runDir, { recursive: true });
  mkdirSync(join(runDir, "work"), { recursive: true });
  mkdirSync(join(runDir, "chats"), { recursive: true });
  mkdirSync(join(runDir, "tasks"), { recursive: true });

  // Write spec
  if (specContent) {
    writeFileSync(join(runDir, "spec.md"), specContent);
  }

  // Parse time limit
  let timeLimitMinutes: number | null = null;
  if (options.timeLimit) {
    try {
      timeLimitMinutes = parseTimeLimit(options.timeLimit);
    } catch (error) {
      console.error(`${ICONS.error} ${(error as Error).message}`);
      process.exit(1);
    }
  }

  // Initialize state using State class (creates DB with proper schema)
  const dbPath = join(runDir, "hirsel.db");
  const runState = new State(dbPath);

  // Set run state
  runState.status = Status.WORKING;
  runState.setProjectPath(projectPath);
  runState.setRequest(specContent);
  runState.setHumanInTheLoop(!options.yolo);
  if (timeLimitMinutes) {
    runState.setTimeLimit(timeLimitMinutes);
  }

  // Generate worker names using core/workers module
  const workerNames = getAvailableNames(scale.min);

  // Create initial "scope" task
  runState.addTask("scope", "Read spec, create exploration tasks");

  // Add workers to database
  for (const name of workerNames) {
    runState.addWorker(name, join(runDir, "work", name));
  }

  // Pre-claim scope task for first worker
  if (workerNames.length > 0) {
    runState.claimTask("scope", workerNames[0]);
  }

  runState.close();

  // Create work directory with staging branch
  const workDir = join(runDir, "work");
  const isMultiWorker = workerNames.length > 1 || scale.autoscale;
  const leader = isMultiWorker ? workerNames[0] : null;

  try {
    await createWorkspace(projectPath, workDir);
    console.log(dim("Created staging workspace"));

    // Create per-worker clones for multi-worker runs
    if (isMultiWorker) {
      for (const name of workerNames) {
        await createWorkerClone(workDir, name);
      }
      console.log(dim(`Created ${workerNames.length} worker clone(s)`));
    }
  } catch (error) {
    console.log(dim(`Git workspace setup: ${(error as Error).message}`));
    // Continue anyway - workspace may already exist
  }

  // Create chat files
  const chatsDir = join(runDir, "chats");

  // User chat (always created)
  writeFileSync(
    join(chatsDir, "user.md"),
    `# User Chat\n\nThis thread is for communication between you and the workers.\n\n---\n\n`
  );

  // Group chat for multi-worker runs
  if (isMultiWorker) {
    const workerList = workerNames.join(", ");
    writeFileSync(
      join(chatsDir, "group.md"),
      `# Group Chat\n\nWorkers: ${workerList}\nLeader: ${leader}\n\n---\n\n`
    );
  }

  // Learnings thread (workers share discoveries)
  writeFileSync(
    join(chatsDir, "learnings.md"),
    `# Learnings\n\nShared discoveries and patterns across workers.\n\n---\n\n`
  );

  // Individual worker chats
  for (const name of workerNames) {
    writeFileSync(
      join(chatsDir, `${name}.md`),
      `# ${name}\n\nPrivate thread for ${name}.\n\n---\n\n`
    );
  }

  // Create tasks.md file
  writeFileSync(
    join(runDir, "tasks.md"),
    `# Tasks

| ID | Status | Worker | Name |
|----|--------|--------|------|
| scope | DOING | ${workerNames[0] || ""} | Read spec, create exploration tasks |
`
  );

  // Create task detail file
  writeFileSync(join(runDir, "tasks", "scope.md"), "");

  // Output
  if (isJsonOutput()) {
    jsonOutput({
      run: runName,
      project: projectPath,
      workers: workerNames,
      status: "working",
    });
    return;
  }

  // Print banner
  console.log(dim(LOGO));

  const progressBar = ditherBar(0, 1, 16);
  console.log(bold(runName));
  console.log();
  console.log(dim("project  ") + projectPath);
  console.log(dim("tasks    ") + dim(progressBar) + " 0/1");
  console.log();
  console.log(dim("monitor  ") + `hirsel view ${runName}`);
  console.log(dim("live     ") + `hirsel ${runName}`);
  console.log();
  console.log(dim("workers  ") + workerNames.join(", "));
  if (isMultiWorker && leader) {
    console.log(dim("leader   ") + leader);
  }
  console.log();

  // Spawn workers in tmux
  const spawnWorkers = process.env.HIRSEL_NO_SPAWN !== "1";
  if (spawnWorkers) {
    console.log(dim("Spawning workers..."));
    const state = new State(dbPath);
    for (let i = 0; i < workerNames.length; i++) {
      const name = workerNames[i];
      const isLeaderWorker = i === 0;
      const workerWorkDir = isMultiWorker
        ? join(workDir, name)
        : join(workDir, "staging");

      const pid = await spawnWorkerInTmux(
        runName,
        name,
        workerWorkDir,
        isLeaderWorker
      );
      if (pid) {
        state.setWorkerPid(name, pid);
        console.log(dim(`  ${name} started (pid ${pid})`));
      } else {
        console.log(dim(`  ${name} failed to start`));
      }
    }
    state.close();
  } else {
    console.log(dim("Start workers with:"));
    for (const name of workerNames) {
      console.log(dim(`  hirsel attach ${runName} ${name}`));
    }
  }
}
