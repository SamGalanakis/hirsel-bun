/**
 * Worker runner for hirsel.
 *
 * Manages:
 * - Spawning ACP workers in subprocesses
 * - Worker lifecycle (start, pause, resume)
 * - Time limit notifications
 * - Eval handling
 * - Autoscaling
 */

import { spawn } from "bun";
import { existsSync, mkdirSync, appendFileSync, writeFileSync, readFileSync } from "fs";
import { join, dirname } from "path";
import {
  ACPClient,
  createMCPServerConfig,
  type MCPServerConfig,
  type SessionUpdate,
} from "./acp";
import { config, getRunDir, getDbPath, getRunConfig, createConfig, type Config, type RunConfig } from "./config";
import { Files } from "./files";
import { createWorkerClone } from "./git";
import { State } from "./state";
import { getAvailableName } from "./workers";
import { Status, WorkerStatus, TaskStatus } from "../shared/types";

/** Max eval attempts before giving up */
export const MAX_EVAL_ATTEMPTS = 50;

/** Time notification thresholds (accelerating frequency) */
const TIME_NOTIFICATION_THRESHOLDS = [25, 50, 75, 85, 90, 95, 98];

const TIME_NOTIFICATION_MESSAGES: Record<number, string> = {
  25: "Time check: 25% elapsed, 75% remaining",
  50: "Halfway point: 50% of time used",
  75: "75% of time used. Start wrapping up non-essential tasks.",
  85: "85% elapsed. Prioritize completing current work.",
  90: "90% of time elapsed! Focus on essential tasks only.",
  95: "5% time remaining! Finalize immediately.",
  98: "2% remaining - run will auto-complete very soon.",
};

/** Eval result types */
export enum EvalResult {
  PASSED = "passed",
  FAILED_CONTINUE = "failed_continue",
  FAILED_MAX_ATTEMPTS = "failed_max_attempts",
  ERROR = "error",
}

/**
 * Check and send time notifications at threshold crossings.
 */
export function checkAndSendTimeNotifications(
  state: State,
  files: Files,
  isMultiWorker: boolean,
  workerName?: string
): void {
  const timeInfo = state.getTimeInfo();
  if (!timeInfo) return;

  const pctElapsed = timeInfo.elapsedPct ?? 0;
  const runState = state.getRunState();
  const lastNotified = runState.lastTimeNotificationPct ?? 0;

  for (const threshold of TIME_NOTIFICATION_THRESHOLDS) {
    if (threshold > lastNotified && pctElapsed >= threshold) {
      const message =
        TIME_NOTIFICATION_MESSAGES[threshold] ?? `Time: ${threshold}% elapsed`;

      const thread = isMultiWorker ? "group" : workerName ?? "user";
      state.addMessage(thread, "System", message);

      // Append to chat file
      const chatFile = join(files.chatsDir, `${thread}.md`);
      if (existsSync(chatFile)) {
        appendFileSync(chatFile, `\n**System**: ${message}\n`);
      }

      console.log(`Time notification sent: ${threshold}% - ${message}`);
      // Update last notification pct in state
      state.setLastTimeNotificationPct(threshold);
    }
  }
}

/**
 * Handle time limit expiration.
 */
export function handleTimeExpired(
  state: State,
  files: Files,
  isMultiWorker: boolean,
  workerName: string,
  logFile: string
): void {
  if (state.status === Status.TIMED_OUT) {
    console.log(`[${workerName}] Time already expired, skipping handler`);
    return;
  }

  const message = "Time limit reached. Run paused with current progress.";
  const thread = isMultiWorker ? "group" : workerName;
  state.addMessage(thread, "System", message);

  // Set all workers to PAUSED
  for (const w of state.getWorkers()) {
    if (
      [WorkerStatus.WORKING, WorkerStatus.WAITING, WorkerStatus.AWAITING].includes(
        w.status
      )
    ) {
      state.updateWorker(w.name, { status: WorkerStatus.PAUSED });
    }
  }

  state.status = Status.TIMED_OUT;
  console.log(`[${workerName}] Run status set to TIMED_OUT`);

  appendFileSync(logFile, "\n[time limit reached - run timed out]\n");
}

/**
 * Build continuation prompt for fresh context.
 */
export function buildContinuationPrompt(
  workerName: string,
  workerPrompt: string,
  baseContext: string,
  state: State,
  files?: Files
): string {
  const tasks = state.getTasks();
  const myDoneTasks = tasks.filter(
    (t) => t.status === TaskStatus.DONE && t.claimedBy === workerName
  );
  const lastTask = myDoneTasks[myDoneTasks.length - 1];
  const todoTasks = tasks.filter((t) => t.status === TaskStatus.TODO);
  const doingTasks = tasks.filter((t) => t.status === TaskStatus.DOING);

  // Check for eval fix task
  let evalContext = "";
  const evalFixTask = todoTasks.find((t) => t.id.startsWith("fix_eval_"));
  if (evalFixTask && files) {
    const taskDetailPath = join(files.tasksDir, `${evalFixTask.id}.md`);
    if (existsSync(taskDetailPath)) {
      const detail = readFileSync(taskDetailPath, "utf-8");
      evalContext = `
## EVAL FAILED - Action Required

The previous evaluation failed. You need to fix the issues identified below.

**Task to claim:** \`${evalFixTask.id}\`

${detail}

---

`;
    }
  }

  // Time context
  let timeContext = "";
  const timeInfo = state.getTimeInfo();
  const runState = state.getRunState();
  if (timeInfo && runState.timeLimitMinutes) {
    const elapsed = Math.round(timeInfo.elapsedMinutes * 10) / 10;
    const remaining = Math.round((timeInfo.remainingMinutes ?? 0) * 10) / 10;
    const pct = Math.round((timeInfo.elapsedPct ?? 0) * 10) / 10;
    const limit = runState.timeLimitMinutes;
    timeContext = `
## Time Status (Updated)
- **Elapsed:** ${elapsed}/${limit} min (${pct}%)
- **Remaining:** ${remaining} min
`;
    if (pct >= 90) {
      timeContext +=
        "\n**URGENT:** Less than 10% time remaining! Focus on completing essential work only.\n";
    } else if (pct >= 75) {
      timeContext +=
        "\n**Note:** Over 75% of time used. Start wrapping up non-essential tasks.\n";
    }
  }

  return `# Continuation

You are **${workerName}**, continuing work on this project.

## Your Progress
- Tasks you've completed: ${myDoneTasks.length}
- Last task: ${lastTask ? `${lastTask.id} - ${lastTask.name}` : "none"}

## Current State
- TODO tasks: ${todoTasks.length}
- Tasks being worked on: ${doingTasks.length}
${timeContext}${evalContext}
## Before Starting Your Next Task
1. **Read the learnings chat**: \`msg_read('learnings')\` - see discoveries from this project
2. Review task list: \`task_list\` - see what's available
3. Claim a TODO task and continue working

${workerPrompt}
${baseContext}`;
}

/**
 * Get the leader worker name.
 */
function getLeader(state: State): string | null {
  const scopeTask = state.getTask("scope");
  return scopeTask?.claimedBy ?? null;
}

/**
 * Spawn a worker subprocess.
 */
export function spawnWorker(
  runName: string,
  config: Config,
  state: State,
  files: Files,
  workerName: string,
  workDir: string,
  options?: {
    resumeSessionId?: string;
    isLeader?: boolean;
    leaderName?: string;
    teammates?: string[];
  }
): number | null {
  // Check if run is paused
  state.reconnect();
  if (state.status === Status.PAUSED) {
    console.log(`[${workerName}] spawn_worker: run is paused, not spawning`);
    state.updateWorker(workerName, { status: WorkerStatus.PAUSED });
    return null;
  }

  state.updateWorker(workerName, { status: WorkerStatus.WORKING });

  const logFile = files.workerLog(workerName);
  mkdirSync(dirname(logFile), { recursive: true });

  // Build the worker script
  const runDir = getRunDir(runName);
  const script = `
import { runAcpWorker } from "./src/core/worker-runner";
await runAcpWorker({
  runName: ${JSON.stringify(runName)},
  workerName: ${JSON.stringify(workerName)},
  workDir: ${JSON.stringify(workDir)},
  specPath: ${JSON.stringify(files.spec)},
  runDir: ${JSON.stringify(runDir)},
  logFile: ${JSON.stringify(logFile)},
  agentCommand: ${JSON.stringify(config.agentCommand)},
  resumeSessionId: ${JSON.stringify(options?.resumeSessionId ?? null)},
  isLeader: ${options?.isLeader ?? false},
  leaderName: ${JSON.stringify(options?.leaderName ?? null)},
  teammates: ${JSON.stringify(options?.teammates ?? null)},
});
`;

  // Spawn subprocess
  const proc = spawn({
    cmd: ["bun", "-e", script],
    cwd: runDir,
    env: {
      ...process.env,
      HIRSEL_RUN: runName,
      HIRSEL_WORKER: workerName,
      HIRSEL_WORKER_SUBPROCESS: "1",
    },
    stdout: "inherit",
    stderr: "inherit",
  });

  const pid = proc.pid;
  state.updateWorker(workerName, { pid });
  console.log(`Spawned worker ${workerName} (PID ${pid})`);

  return pid;
}

/**
 * Resume workers that are awaiting tasks.
 */
export function resumeAwaitingWorkers(runName: string): string[] {
  const runDir = getRunDir(runName);
  if (!existsSync(runDir)) return [];

  const state = new State(getDbPath(runName));
  const files = new Files(runDir);

  const claimable = state.getClaimableTasks();
  if (claimable.length === 0) {
    state.close();
    return [];
  }

  const workers = state.getWorkers();
  const awaiting = workers.filter((w) => w.status === WorkerStatus.AWAITING);

  if (awaiting.length === 0) {
    state.close();
    return [];
  }

  const resumed: string[] = [];
  for (const worker of awaiting) {
    if (!worker.sessionId || !worker.workDir) {
      console.warn(`Worker ${worker.name} awaiting but missing session/work_dir`);
      continue;
    }

    console.log(
      `Resuming awaiting worker ${worker.name} - ${claimable.length} claimable tasks`
    );
    const pid = spawnWorker(runName, config, state, files, worker.name, worker.workDir, {
      resumeSessionId: worker.sessionId,
    });
    if (pid) {
      resumed.push(worker.name);
    }
  }

  state.close();
  return resumed;
}

/**
 * Check if we should scale up workers.
 */
export function maybeScaleUp(runName: string): string | null {
  const runConfig = createConfig();
  const runDir = getRunDir(runName);
  if (!existsSync(runDir)) return null;

  const state = new State(getDbPath(runName));
  const files = new Files(runDir);

  // Check autoscaling settings
  const scaleStr = state.getWorkerScale();
  if (!scaleStr) {
    state.close();
    return null;
  }

  // Parse worker scale (simplified)
  const match = scaleStr.match(/^(\d+)-(\d+)$/) || scaleStr.match(/^(\d+)\+$/);
  if (!match) {
    state.close();
    return null;
  }

  const minWorkers = parseInt(match[1], 10);
  const maxWorkers = match[2] ? parseInt(match[2], 10) : Infinity;

  if (state.status === Status.PAUSED) {
    state.close();
    return null;
  }

  const workers = state.getWorkers();
  const currentCount = workers.length;

  if (currentCount >= maxWorkers) {
    state.close();
    return null;
  }

  const activeStatuses = [WorkerStatus.WORKING, WorkerStatus.WAITING];
  const activeWorkers = workers.filter((w) => activeStatuses.includes(w.status));
  const claimable = state.getClaimableTasks();

  if (claimable.length <= activeWorkers.length) {
    state.close();
    return null;
  }

  // Get new worker name
  const existingNames = new Set(workers.map((w) => w.name));
  const newName = getAvailableName(existingNames);

  const projectPath = state.getProjectPath();
  if (!projectPath) {
    state.close();
    return null;
  }

  const stagingDir = join(runDir, "work", "staging");

  // Create worker clone
  let workerDir: string;
  try {
    // Note: createWorkerClone is async but we're in a sync context
    // For now, skip autoscaling in this implementation
    state.close();
    console.log("Autoscaling not fully implemented yet");
    return null;
  } catch (e) {
    state.close();
    return null;
  }
}

/**
 * Send a user message and resume waiting workers.
 */
export function sendUserMessage(
  runName: string,
  thread: string,
  message: string,
  state: State,
  files: Files
): string[] {
  const chatPath = files.chatFile(thread);

  state.addMessage(thread, "user", message, false);
  if (existsSync(chatPath)) {
    appendFileSync(chatPath, `\n**user**: ${message}\n`);
  }

  const runConfig = createConfig();
  const resumed: string[] = [];

  for (const worker of state.getWorkers()) {
    if (worker.status === WorkerStatus.WAITING && worker.waitingThread === thread) {
      if (!worker.sessionId || !worker.workDir) continue;

      state.updateWorker(worker.name, { waitingThread: null });
      state.setWaitingReason(null);

      console.log(`Resuming waiting worker ${worker.name} on thread ${thread}`);
      const pid = spawnWorker(runName, runConfig, state, files, worker.name, worker.workDir, {
        resumeSessionId: worker.sessionId,
      });
      if (pid) {
        state.updateWorker(worker.name, { pid });
        resumed.push(worker.name);
      }
    }
  }

  return resumed;
}

/**
 * Send a system message to a worker.
 */
export function sendSystemMessage(
  workerName: string,
  message: string,
  state: State,
  files: Files
): void {
  const chatPath = files.chatFile(workerName);
  mkdirSync(dirname(chatPath), { recursive: true });

  state.addMessage(workerName, "System", message, false);
  if (existsSync(chatPath)) {
    appendFileSync(chatPath, `\n**System**: ${message}\n`);
  } else {
    writeFileSync(chatPath, `# Chat: ${workerName}\n\n**System**: ${message}\n`);
  }
}

/**
 * Main ACP worker runner.
 * This runs in the worker subprocess.
 */
export async function runAcpWorker(options: {
  runName: string;
  workerName: string;
  workDir: string;
  specPath: string;
  runDir: string;
  logFile: string;
  agentCommand: string[];
  resumeSessionId?: string | null;
  isLeader?: boolean;
  leaderName?: string | null;
  teammates?: string[] | null;
}): Promise<void> {
  const {
    runName,
    workerName,
    workDir,
    logFile,
    agentCommand,
    resumeSessionId,
    isLeader,
    leaderName,
    teammates,
  } = options;

  console.log(`[${workerName}] Starting ACP worker for run=${runName}`);

  const runConfig = getRunConfig(runName);
  const state = new State(runConfig.dbPath);
  const files = new Files(runConfig.runDir);

  // Create log file
  mkdirSync(dirname(logFile), { recursive: true });
  writeFileSync(logFile, `[worker: ${workerName}]\n`);
  if (isLeader) {
    appendFileSync(logFile, "Starting as leader...\n\n");
  } else {
    appendFileSync(logFile, "Starting, waiting for tasks...\n\n");
  }

  const mcpServer = createMCPServerConfig("hirsel", "hirsel-mcp", [], {
    HIRSEL_RUN: runName,
    HIRSEL_WORKER: workerName,
  });

  const isMultiWorker = teammates != null && teammates.length > 0;

  // Build base context
  let baseContext = `
## Your Context

- **Run name:** ${runName}
- **Worker name:** ${workerName}
- **Run directory:** ${runConfig.runDir}
- **Work directory:** ${workDir} (git worktree - this is where you write code)

## Available MCP Tools

You have access to the \`hirsel\` MCP server with these tools:
- \`task_list\` - List all tasks
- \`task_add(task_id, name)\` - Add a new task
- \`task_claim(task_id)\` - Claim a task to work on
- \`task_done(task_id?)\` - Mark a task as done
- \`task_unclaim(task_id?)\` - Release a task
- \`task_await\` - Block until tasks are available
- \`work_done\` - Signal all work is complete
- \`msg_send(thread, message, wait?)\` - Send a message
- \`msg_read(thread?)\` - Check for new messages
- \`msg_list\` - List available threads
- \`time_status\` - Check time limit status
`;

  // Add time info if set
  const timeInfo2 = state.getTimeInfo();
  const runState2 = state.getRunState();
  if (timeInfo2 && runState2.timeLimitMinutes) {
    const elapsed = Math.round(timeInfo2.elapsedMinutes * 10) / 10;
    const remaining = Math.round((timeInfo2.remainingMinutes ?? 0) * 10) / 10;
    const pct = Math.round((timeInfo2.elapsedPct ?? 0) * 10) / 10;
    baseContext += `
## Time Limit

- **Total time:** ${runState2.timeLimitMinutes} minutes
- **Elapsed:** ${elapsed} minutes (${pct}%)
- **Remaining:** ${remaining} minutes

Work efficiently. Use \`time_status\` tool to check current time.
`;
  }

  // Role-specific instructions
  if (teammates && teammates.length > 0) {
    const teammatesStr = teammates.join(", ");
    if (isLeader) {
      baseContext += `
## Your Role: LEADER

You are the team leader. Your teammates are: ${teammatesStr}

### Leader Workflow:
1. Read spec.md to understand what needs to be built
2. Create exploration tasks (independent, no blocking)
3. Complete "scope" task to unlock work for teammates
4. Claim and complete exploration tasks
5. Create implementation tasks with full knowledge
6. Work on implementation with your team
`;
    } else {
      baseContext += `
## Your Role: TEAM MEMBER

Leader: **${leaderName}**. Teammates: ${teammatesStr}

### Your Workflow:
1. Read spec.md to understand the project
2. Check task_list or task_await for tasks
3. Claim and work on tasks
4. Document findings in learnings chat
5. Call work_done when all tasks complete
`;
    }
  } else {
    baseContext += `
## Getting Started

1. Use \`task_list\` to see available tasks
2. Use \`task_claim\` to claim the first TODO task
3. Work on the task in your work directory
4. Use \`task_done\` when done
5. Repeat until all tasks are done
6. Call \`work_done\` to finish
`;
  }

  // Track tools and shutdown state
  const activeTool: Record<string, string> = {};
  const shutdownState = { waiting: false, evalPending: false };
  let currentClient: ACPClient | null = null;

  const onUpdate = (update: SessionUpdate) => {
    switch (update.updateType) {
      case "agent_message_chunk": {
        const text = (update.content.content as { text?: string })?.text ?? "";
        if (text) {
          appendFileSync(logFile, text);
        }
        break;
      }
      case "tool_call_start": {
        const meta = (update.content._meta as { claudeCode?: { toolName?: string } })
          ?.claudeCode;
        const toolName = meta?.toolName ?? (update.content.title as string) ?? "unknown";
        const toolId = update.content.toolCallId as string;
        const rawInput = update.content.rawInput as Record<string, unknown>;
        const isMcpTool = toolName.startsWith("mcp__");

        if (toolId && activeTool[toolId]) return;

        let detail = "";
        if (toolName === "Read" && rawInput.file_path) {
          detail = rawInput.file_path as string;
        } else if (toolName === "Bash" && rawInput.command) {
          const cmd = rawInput.command as string;
          detail = cmd.length > 60 ? cmd.slice(0, 60) + "..." : cmd;
        }

        if (!isMcpTool && !detail) return;
        if (!toolId) return;

        activeTool[toolId] = toolName;
        if (isMcpTool) {
          appendFileSync(logFile, `\n[tool: ${toolName}]${JSON.stringify(rawInput)}\n`);
        } else {
          appendFileSync(logFile, `\n[tool: ${toolName}] ${detail}\n`);
        }
        break;
      }
      case "tool_call_progress": {
        const toolId = update.content.toolCallId as string;
        const status = update.content.status as string;

        if (status === "completed" || status === "failed") {
          const toolName = toolId ? activeTool[toolId] : "";
          delete activeTool[toolId];

          if (toolName) {
            appendFileSync(logFile, "[/tool]\n");
          }

          // Handle special tools
          if (toolName === "mcp__hirsel__msg_send") {
            const worker = state.getWorker(workerName);
            if (worker?.status === WorkerStatus.WAITING) {
              shutdownState.waiting = true;
              appendFileSync(logFile, "\n[waiting for reply...]\n");
              currentClient?.kill();
            }
          } else if (toolName === "mcp__hirsel__task_await") {
            const worker = state.getWorker(workerName);
            if (worker?.status === WorkerStatus.AWAITING) {
              shutdownState.waiting = true;
              appendFileSync(logFile, "\n[awaiting tasks from leader...]\n");
              currentClient?.kill();
            }
          } else if (toolName === "mcp__hirsel__work_done") {
            state.reconnect();
            if (state.status === Status.EVAL) {
              shutdownState.evalPending = true;
              appendFileSync(logFile, "\n[work complete - running eval...]\n");
              currentClient?.kill();
            }
          }
        }
        break;
      }
    }
  };

  let isContinuation = false;
  let isFirstIteration = true;

  while (true) {
    // Check if run is paused
    state.reconnect();
    if (state.status === Status.PAUSED) {
      console.log(`[${workerName}] Run is paused, exiting worker loop`);
      state.updateWorker(workerName, { status: WorkerStatus.PAUSED });
      appendFileSync(logFile, "\n[run paused]\n");
      break;
    }

    // Time notifications
    checkAndSendTimeNotifications(state, files, isMultiWorker, workerName);

    // Time expiration check
    if (state.isTimeExpired()) {
      handleTimeExpired(state, files, isMultiWorker, workerName, logFile);
      break;
    }

    // Reset shutdown state
    shutdownState.waiting = false;
    shutdownState.evalPending = false;

    // Create ACP client
    const client = new ACPClient({
      command: agentCommand,
      cwd: workDir,
      mcpServers: [mcpServer],
      onUpdate,
    });
    currentClient = client;

    try {
      await client.start();
      console.log(`[${workerName}] ACP client started`);

      if (isFirstIteration) {
        appendFileSync(logFile, "Connected to agent.\n\n");
      }

      // Create session
      const sessionId = await client.newSession({
        cwd: workDir,
        mcpServers: [mcpServer],
      });
      console.log(`[${workerName}] Created session ${sessionId}`);

      state.updateWorker(workerName, { sessionId, needsRestart: false });
      await client.setMode(sessionId, "bypassPermissions");

      // Build prompt
      let fullPrompt: string;
      if (isFirstIteration && resumeSessionId) {
        fullPrompt =
          "You have received a reply. Use `msg_read` to see the message, then continue your work.";
      } else if (isContinuation) {
        fullPrompt = buildContinuationPrompt(
          workerName,
          "",
          baseContext,
          state,
          files
        );
        appendFileSync(logFile, "\n[fresh context - starting new session]\n\n");
      } else {
        fullPrompt =
          baseContext +
          "\n\nBegin by using `task_list` to see available tasks, then claim and work on them.";
      }

      isFirstIteration = false;

      // Send prompt (blocks until agent finishes)
      await client.prompt({ sessionId, text: fullPrompt });

      // Check exit reason
      state.reconnect();
      const worker = state.getWorker(workerName);

      if (shutdownState.waiting) {
        console.log(`[${workerName}] Worker paused, waiting for reply/tasks`);
        break;
      }

      if (state.status === Status.EVAL) {
        console.log(`[${workerName}] Triggering eval`);
        // TODO: Implement eval handling
        break;
      }

      if (worker?.needsRestart) {
        const currentIter = state.incrementIteration();
        const maxIter = state.getMaxIterations();

        if (maxIter && currentIter >= maxIter) {
          console.log(`[${workerName}] Max iterations (${maxIter}) reached`);
          state.status = Status.RUNAWAY;
          state.updateWorker(workerName, { status: WorkerStatus.PAUSED });
          appendFileSync(
            logFile,
            `\n[RUNAWAY] Max iterations (${maxIter}) reached.\n`
          );
          break;
        }

        console.log(
          `[${workerName}] Task completed, restarting with fresh context`
        );
        isContinuation = true;
        await client.stop();
        continue;
      }

      console.log(`[${workerName}] Agent session ended`);
      break;
    } catch (e) {
      if (shutdownState.evalPending || shutdownState.waiting) {
        break;
      }
      console.error(`[${workerName}] Worker error:`, e);
      appendFileSync(logFile, `\n[ERROR] Worker crashed: ${e}\n`);
      throw e;
    } finally {
      await client.stop();
    }
  }

  state.close();
}
