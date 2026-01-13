/**
 * Evaluation system for hirsel.
 *
 * Manages:
 * - Running evals via ACP client
 * - Eval MCP server for pass/fail verdict tools
 * - Branch validation (must be on staging)
 * - Eval result collection and logging
 */

import { existsSync, mkdirSync, readFileSync, writeFileSync, appendFileSync, unlinkSync } from "fs";
import { join, resolve } from "path";
import { EvalStatus } from "../shared/types";
import type { Config } from "./config";
import type { Files } from "./files";
import type { State } from "./state";
import { ACPClient, createMCPServerConfig, type MCPServerConfig, type SessionUpdate } from "./acp";
import { getCurrentBranch } from "./git";
import { getAvailableName } from "./workers";
import type { Eval } from "../shared/types";

export { runEval, runEvalMcpServer, waitForEval };

// Default eval timeout in seconds
const DEFAULT_EVAL_TIMEOUT = 600; // 10 minutes

// Eval names use a different prefix to distinguish from workers
const EVAL_NAME_PREFIX = "eval_";

// Tool definitions for the eval MCP server
const EVAL_PASS_TOOL = {
  name: "eval_pass",
  description: "Call this when all evaluation criteria pass. No parameters needed.",
  inputSchema: {
    type: "object",
    properties: {},
  },
};

const EVAL_FAIL_TOOL = {
  name: "eval_fail",
  description: "Call this when evaluation fails. You must provide feedback explaining what failed and how to fix it.",
  inputSchema: {
    type: "object",
    properties: {
      feedback: {
        type: "string",
        description: "What failed and how to fix it",
      },
    },
    required: ["feedback"],
  },
};

// Conservative timeouts
const EVAL_POLL_INTERVAL = 2000; // ms between checking if eval is done
const MAX_VERDICT_RETRIES = 2; // max follow-up prompts if agent doesn't submit verdict

const VERDICT_REMINDER_PROMPT = `You have not yet submitted your evaluation verdict.

You MUST call one of these MCP tools to complete your evaluation:

- \`mcp__eval__eval_pass\` - if all checks passed
- \`mcp__eval__eval_fail\` - if any check failed (include feedback parameter)

Please call the appropriate tool NOW to submit your verdict.`;

/** Get the eval prompt template */
function getEvalPrompt(): string {
  // The prompt is bundled with the package
  const promptPath = join(__dirname, "..", "prompts", "eval.md");
  if (!existsSync(promptPath)) {
    // Fallback: check relative to cwd
    const altPath = join(process.cwd(), "src", "prompts", "eval.md");
    if (existsSync(altPath)) {
      return readFileSync(altPath, "utf-8");
    }
    throw new Error(`Eval prompt not found at ${promptPath}`);
  }
  return readFileSync(promptPath, "utf-8");
}

/** Eval result from the MCP server */
interface EvalResult {
  success: boolean;
  feedback: string;
}

/**
 * MCP server for eval tools.
 * Provides eval_pass and eval_fail tools that write results to a file.
 */
class EvalMCPServer {
  private resultFile: string;
  submitted = false;

  constructor(resultFile: string) {
    this.resultFile = resultFile;
    console.debug(`EvalMCPServer initialized with result_file=${resultFile}`);
  }

  handleRequest(request: {
    method?: string;
    params?: Record<string, unknown>;
    id?: string | number | null;
  }): Record<string, unknown> | null {
    const method = request.method;
    const params = (request.params || {}) as Record<string, unknown>;
    const requestId = request.id;

    console.debug(`EvalMCPServer handling method=${method}`);

    if (requestId === undefined || requestId === null) {
      return null;
    }

    try {
      let result: Record<string, unknown>;

      if (method === "initialize") {
        result = {
          protocolVersion: "2024-11-05",
          capabilities: { tools: {} },
          serverInfo: { name: "hirsel-eval", version: "1.0.0" },
        };
      } else if (method === "tools/list") {
        result = { tools: [EVAL_PASS_TOOL, EVAL_FAIL_TOOL] };
      } else if (method === "tools/call") {
        result = this.handleToolsCall(params);
      } else {
        console.warn(`EvalMCPServer: unknown method ${method}`);
        return {
          jsonrpc: "2.0",
          id: requestId,
          error: { code: -32601, message: `Method not found: ${method}` },
        };
      }

      return {
        jsonrpc: "2.0",
        id: requestId,
        result,
      };
    } catch (e) {
      console.error(`EvalMCPServer: error handling ${method}: ${e}`);
      return {
        jsonrpc: "2.0",
        id: requestId,
        error: { code: -32000, message: String(e) },
      };
    }
  }

  private handleToolsCall(params: Record<string, unknown>): Record<string, unknown> {
    const toolName = params.name as string;
    const args = (params.arguments || {}) as Record<string, unknown>;

    switch (toolName) {
      case "eval_pass": {
        console.log("EvalMCPServer: eval_pass called");
        const result: EvalResult = { success: true, feedback: "All checks passed." };
        writeFileSync(this.resultFile, JSON.stringify(result));
        this.submitted = true;
        return {
          content: [{ type: "text", text: "Eval result submitted: PASSED" }],
        };
      }

      case "eval_fail": {
        const feedback = (args.feedback as string) || "No feedback provided";
        console.log("EvalMCPServer: eval_fail called");
        console.debug(`EvalMCPServer: feedback=${feedback.slice(0, 200)}...`);
        const result: EvalResult = { success: false, feedback };
        writeFileSync(this.resultFile, JSON.stringify(result));
        this.submitted = true;
        return {
          content: [{ type: "text", text: "Eval result submitted: FAILED" }],
        };
      }

      default:
        console.warn(`EvalMCPServer: unknown tool ${toolName}`);
        return {
          content: [{ type: "text", text: `Unknown tool: ${toolName}` }],
          isError: true,
        };
    }
  }
}

/**
 * Run the eval MCP server (stdio mode).
 * This is called as a subprocess by the ACP client.
 */
function runEvalMcpServer(): void {
  const resultFile = process.env.HIRSEL_EVAL_RESULT_FILE;
  if (!resultFile) {
    const errorResponse = {
      jsonrpc: "2.0",
      id: null,
      error: { code: -32000, message: "HIRSEL_EVAL_RESULT_FILE must be set" },
    };
    console.log(JSON.stringify(errorResponse));
    console.error("HIRSEL_EVAL_RESULT_FILE not set, exiting");
    process.exit(1);
  }

  const server = new EvalMCPServer(resultFile);
  console.log(`Eval MCP server started, result_file=${resultFile}`);

  // Read JSON-RPC requests from stdin
  const readline = require("readline");
  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout,
    terminal: false,
  });

  rl.on("line", (line: string) => {
    line = line.trim();
    if (!line) return;

    try {
      const request = JSON.parse(line);
      const response = server.handleRequest(request);
      if (response) {
        console.log(JSON.stringify(response));
      }

      // Exit after successful submission
      if (server.submitted) {
        console.log("Eval MCP server: result submitted, exiting");
        process.exit(0);
      }
    } catch (e) {
      if (e instanceof SyntaxError) {
        console.error(`Eval MCP server: JSON parse error: ${e}`);
        console.log(
          JSON.stringify({
            jsonrpc: "2.0",
            id: null,
            error: { code: -32700, message: `Parse error: ${e}` },
          })
        );
      } else {
        throw e;
      }
    }
  });
}

/** Validate that we're on the staging branch */
function validateStagingBranch(files: Files): { valid: boolean; error: string } {
  // For single-worker mode, git repo is in work/staging/
  // For multi-worker mode, each worker has work/<worker>/
  // Try staging first (single-worker), then fall back to work dir
  const stagingDir = join(files.work, "staging");
  let workDir: string;

  if (existsSync(stagingDir) && existsSync(join(stagingDir, ".git"))) {
    workDir = stagingDir;
  } else {
    workDir = files.work;
  }

  if (!existsSync(workDir)) {
    return { valid: false, error: `Work directory does not exist: ${workDir}` };
  }

  let currentBranch: string;
  try {
    // getCurrentBranch is async, but we need sync here
    // Use Bun's synchronous shell as a fallback
    const result = Bun.spawnSync(["git", "-C", workDir, "rev-parse", "--abbrev-ref", "HEAD"]);
    currentBranch = new TextDecoder().decode(result.stdout).trim();
  } catch (e) {
    console.error(`Failed to get current branch: ${e}`);
    return { valid: false, error: `Failed to determine current branch: ${e}` };
  }

  if (currentBranch !== "staging") {
    const errorMsg =
      `Eval can only run on the 'staging' branch, but currently on '${currentBranch}'.\n\n` +
      `To fix this:\n` +
      `1. Merge your work to staging: git checkout staging && git merge ${currentBranch}\n` +
      `2. Push to origin: git push origin staging\n` +
      `3. Then call work_done again\n\n` +
      `Eval runs on staging to ensure all workers' code is integrated before verification.`;
    return { valid: false, error: errorMsg };
  }

  return { valid: true, error: "" };
}

/** Wait for a running eval to complete */
function waitForEval(state: State, timeout = 1800000): Eval | null {
  const startTime = Date.now();

  while (true) {
    // Refresh state to get fresh data
    const runningEvals = state.getRunningEvals();
    const runningEval = runningEvals.length > 0 ? runningEvals[0] : null;

    if (runningEval === null) {
      // No running eval - it may have just completed
      return null;
    }

    const elapsed = Date.now() - startTime;
    if (elapsed > timeout) {
      console.warn(`waitForEval: timed out after ${Math.round(elapsed / 1000)}s waiting for eval ${runningEval.id}`);
      return null;
    }

    console.debug(`waitForEval: eval ${runningEval.id} still running, waiting...`);

    // Synchronous sleep
    Bun.sleepSync(EVAL_POLL_INTERVAL);
  }
}

/** Generate a unique eval name */
function generateEvalName(): string {
  // Use a timestamp-based name for simplicity
  const timestamp = Date.now().toString(36);
  return `${EVAL_NAME_PREFIX}${timestamp}`;
}

/** Run eval via ACP client */
async function runEvalAcp(options: {
  evalName: string;
  evalId: number;
  fullPrompt: string;
  resultFile: string;
  evalLogFile: string;
  cwd: string;
  branch: string;
  timeout: number;
  workerName: string | null;
  state: State;
  agentCommand: string[];
}): Promise<EvalResult> {
  const {
    evalName,
    evalId,
    fullPrompt,
    resultFile,
    evalLogFile,
    cwd,
    branch,
    timeout,
    workerName,
    state,
    agentCommand,
  } = options;

  const logPrefix = `[${workerName || "unknown"}]`;

  // Create MCP server config for eval
  // Use short name "eval" so tools are mcp__eval__eval_pass/eval_fail
  const mcpServer = createMCPServerConfig(
    "eval",
    "hirsel-eval-mcp",
    [],
    { HIRSEL_EVAL_RESULT_FILE: resultFile }
  );

  // Write log file header
  const now = new Date().toISOString();
  writeFileSync(evalLogFile, `# Eval ${evalName} (id=${evalId})\n# Started: ${now}\n# Branch: ${branch}\n\n`);

  // Track active tools for logging
  const activeTools: Map<string, string> = new Map();

  const onUpdate = (update: SessionUpdate): void => {
    const updateType = update.updateType;
    const content = update.content || {};

    switch (updateType) {
      case "agent_message_chunk": {
        const text = (content.content as Record<string, unknown>)?.text as string;
        if (text) {
          appendFileSync(evalLogFile, text);
        }
        break;
      }

      case "tool_call_start": {
        const meta = ((content._meta as Record<string, unknown>)?.claudeCode as Record<string, unknown>) || {};
        const toolName = (meta.toolName as string) || (content.title as string) || "unknown";
        const toolId = content.toolCallId as string;
        const rawInput = (content.rawInput as Record<string, unknown>) || {};
        const isMcpTool = toolName.startsWith("mcp__");

        if (toolId && activeTools.has(toolId)) {
          return;
        }

        let detail = "";
        if (toolName === "Read" && rawInput.file_path) {
          detail = rawInput.file_path as string;
        } else if (toolName === "Bash" && rawInput.command) {
          const cmd = rawInput.command as string;
          detail = cmd.length > 60 ? cmd.slice(0, 60) + "..." : cmd;
        }

        if (!isMcpTool && !detail) {
          return;
        }

        // Only write tool to log if we have a tool_id to track it
        if (!toolId) {
          return;
        }

        activeTools.set(toolId, toolName);

        console.debug(`${logPrefix} Eval tool call: ${toolName}`);
        if (isMcpTool) {
          appendFileSync(evalLogFile, `\n[tool: ${toolName}]${JSON.stringify(rawInput)}\n`);
        } else {
          appendFileSync(evalLogFile, `\n[tool: ${toolName}] ${detail}\n`);
        }
        break;
      }

      case "tool_call_update":
      case "tool_call_progress": {
        const toolId = content.toolCallId as string;
        const status = content.status as string;

        if (status === "completed" || status === "failed") {
          const toolName = toolId ? activeTools.get(toolId) : undefined;
          if (toolName) {
            activeTools.delete(toolId);
            appendFileSync(evalLogFile, "[/tool]\n");
          }
        }
        break;
      }
    }
  };

  // Create ACP client
  const client = new ACPClient({
    command: agentCommand,
    cwd,
    mcpServers: [mcpServer],
    onUpdate,
  });

  try {
    console.log(`${logPrefix} runEvalAcp: starting ACP client`);
    await client.start();

    appendFileSync(evalLogFile, "Connected to agent.\n\n");

    // Create session
    const sessionId = await client.newSession({
      cwd,
      mcpServers: [mcpServer],
    });
    console.log(`${logPrefix} runEvalAcp: created session ${sessionId}`);

    // Set bypassPermissions mode
    await client.setMode(sessionId, "bypassPermissions");

    // Send initial prompt
    let currentPrompt = fullPrompt;
    let attempt = 0;

    while (attempt <= MAX_VERDICT_RETRIES) {
      try {
        // Create a promise that times out
        const promptPromise = client.prompt({ sessionId, text: currentPrompt });
        const timeoutPromise = new Promise<never>((_, reject) => {
          setTimeout(() => reject(new Error("TIMEOUT")), timeout);
        });

        await Promise.race([promptPromise, timeoutPromise]);
        console.log(`${logPrefix} runEvalAcp: prompt completed (attempt ${attempt})`);
      } catch (e) {
        if (e instanceof Error && e.message === "TIMEOUT") {
          const timeoutMins = Math.round(timeout / 60000);
          console.warn(`${logPrefix} runEvalAcp: timeout after ${timeout}ms`);
          appendFileSync(evalLogFile, `\n[TIMEOUT after ${timeoutMins} minutes]\n`);
          return {
            success: false,
            feedback: `Eval timed out after ${timeoutMins} minutes`,
          };
        }
        throw e;
      }

      // Check if verdict was submitted
      if (existsSync(resultFile)) {
        break;
      }

      // No verdict - send reminder if we have retries left
      attempt++;
      if (attempt <= MAX_VERDICT_RETRIES) {
        console.warn(`${logPrefix} runEvalAcp: no verdict submitted, sending reminder (attempt ${attempt})`);
        appendFileSync(evalLogFile, "\n[SYSTEM: No verdict submitted, sending reminder...]\n\n");
        currentPrompt = VERDICT_REMINDER_PROMPT;
      } else {
        console.error(`${logPrefix} runEvalAcp: no verdict after ${MAX_VERDICT_RETRIES} reminders`);
        appendFileSync(
          evalLogFile,
          `\n[SYSTEM: Eval failed to submit verdict after ${MAX_VERDICT_RETRIES} reminders]\n`
        );
      }
    }
  } catch (e) {
    console.error(`${logPrefix} runEvalAcp: error: ${e}`);
    appendFileSync(evalLogFile, `\n[ERROR: ${e}]\n`);
    return { success: false, feedback: `Failed to run eval: ${e}` };
  } finally {
    await client.stop();
  }

  // Read result from file
  if (existsSync(resultFile)) {
    try {
      const resultData = JSON.parse(readFileSync(resultFile, "utf-8")) as EvalResult;
      console.log(`${logPrefix} runEvalAcp: got result, success=${resultData.success}`);
      return resultData;
    } catch (e) {
      console.error(`${logPrefix} runEvalAcp: failed to parse result: ${e}`);
      return { success: false, feedback: `Failed to parse eval result: ${e}` };
    }
  } else {
    console.error(`${logPrefix} runEvalAcp: no result file found after ${MAX_VERDICT_RETRIES} retries`);
    return {
      success: false,
      feedback: `Eval did not submit a verdict after ${MAX_VERDICT_RETRIES} reminders. Check eval log for errors.`,
    };
  }
}

/** Main eval runner */
async function runEval(
  runName: string,
  config: Config,
  files: Files,
  state: State,
  workerName: string | null = null
): Promise<{
  success: boolean;
  feedback: string;
  evalId?: number;
  evalName?: string;
  waited?: boolean;
  branchError?: boolean;
}> {
  console.log(`[${workerName || "unknown"}] runEval: starting for run=${runName}`);

  // Check for already running eval (debounce)
  const runningEvals = state.getRunningEvals();
  const runningEval = runningEvals.length > 0 ? runningEvals[0] : null;
  if (runningEval) {
    console.log(
      `[${workerName || "unknown"}] runEval: eval ${runningEval.id} already running, waiting for it`
    );
    const completedEval = waitForEval(state, DEFAULT_EVAL_TIMEOUT * 1000);
    if (completedEval) {
      const success = completedEval.status === "passed";
      return {
        success,
        feedback: completedEval.feedback || "",
        evalId: completedEval.id,
        waited: true,
      };
    } else {
      console.error(`[${workerName || "unknown"}] runEval: timed out waiting for running eval`);
      return {
        success: false,
        feedback: "Timed out waiting for running eval to complete",
        waited: true,
      };
    }
  }

  // Validate staging branch
  const validation = validateStagingBranch(files);
  if (!validation.valid) {
    console.error(`[${workerName || "unknown"}] runEval: branch validation failed: ${validation.error}`);
    return {
      success: false,
      feedback: validation.error,
      branchError: true,
    };
  }

  // Generate eval name for this run
  const evalName = generateEvalName();
  console.log(`[${workerName || "unknown"}] runEval: assigned eval name '${evalName}'`);

  // Create log file path
  const evalLogFile = join(files.tmpDir, `${evalName}.log`);
  mkdirSync(files.tmpDir, { recursive: true });

  // Start eval record
  const branch = "staging";
  const evalId = state.addEval(branch, evalName);
  console.log(`[${workerName || "unknown"}] runEval: started eval ${evalId} (${evalName})`);

  // Read specs and prompt
  if (!files.hasEvalSpec()) {
    const errorMsg = `Eval spec not found at ${files.evalSpec}`;
    console.error(`[${workerName || "unknown"}] runEval: ${errorMsg}`);
    state.setEvalStatus(evalId, EvalStatus.FAILED, errorMsg);
    return { success: false, feedback: errorMsg };
  }

  const spec = files.hasSpec() ? files.readSpec() : "(no spec provided)";
  const evalSpec = files.readEvalSpec();

  let prompt: string;
  try {
    prompt = getEvalPrompt();
  } catch (e) {
    console.error(`[${workerName || "unknown"}] runEval: ${e}`);
    state.setEvalStatus(evalId, EvalStatus.FAILED, String(e));
    return { success: false, feedback: String(e) };
  }

  // Determine work directory - use staging directory directly so eval agent
  // isn't confused by worker folders (capaneus, hippomedon, etc.)
  const stagingDir = join(files.work, "staging");
  let cwd: string;
  if (existsSync(stagingDir) && existsSync(join(stagingDir, ".git"))) {
    cwd = stagingDir;
  } else if (existsSync(files.work)) {
    cwd = files.work;
  } else {
    cwd = files.runDir;
  }

  // Build time context if time limit is set
  let timeContext = "";
  const timeInfo = state.getTimeInfo();
  const runState = state.getRunState();
  if (timeInfo && runState.timeLimitMinutes) {
    const elapsed = Math.round(timeInfo.elapsedMinutes * 10) / 10;
    const pct = Math.round((timeInfo.elapsedPct ?? 0) * 10) / 10;
    const limit = runState.timeLimitMinutes;
    timeContext = `
## Time Context
This run has a ${limit} minute time limit. ${elapsed} minutes elapsed (${pct}%).
Evaluate based on what was achievable in the time available.
`;
  }

  const context = `
## Run
${runName}

## Eval Name
${evalName}
${timeContext}
## Original Spec (what workers were asked to build)
${spec}

## Eval Specification (what to verify)
${evalSpec}

## Work Directory
${cwd}

You are positioned in the project directory. Evaluate the code according to the specifications above. Use the eval_submit tool to submit your verdict.
`;
  const fullPrompt = prompt + "\n\n" + context;

  // Create temp file for result
  const resultFile = join(files.tmpDir, "eval_result.json");
  mkdirSync(files.tmpDir, { recursive: true });
  if (existsSync(resultFile)) {
    unlinkSync(resultFile);
  }

  console.log(`[${workerName || "unknown"}] runEval: eval log at ${evalLogFile}`);

  // Run eval via ACP (same as workers)
  const result = await runEvalAcp({
    evalName,
    evalId,
    fullPrompt,
    resultFile,
    evalLogFile,
    cwd,
    branch,
    timeout: DEFAULT_EVAL_TIMEOUT * 1000,
    workerName,
    state,
    agentCommand: config.agentCommand,
  });

  // Update eval record
  const evalStatus = result.success ? EvalStatus.PASSED : EvalStatus.FAILED;
  state.setEvalStatus(evalId, evalStatus, result.feedback);
  console.log(
    `[${workerName || "unknown"}] runEval: eval ${evalId} (${evalName}) completed, success=${result.success}`
  );

  return {
    success: result.success,
    feedback: result.feedback,
    evalId,
    evalName,
  };
}
