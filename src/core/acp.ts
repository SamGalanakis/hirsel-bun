/**
 * Agent Control Protocol (ACP) Client
 * Port of Python reference implementation (acp.py)
 *
 * Provides communication with AI coding agents (Claude, Gemini, etc.) via stdio JSON-RPC.
 */

import { spawn, type Subprocess, type FileSink } from "bun";

// =============================================================================
// Types
// =============================================================================

export class ACPError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ACPError";
  }
}

/** Session update callback payload */
export interface SessionUpdate {
  sessionId: string;
  updateType: string;
  content: Record<string, unknown>;
}

/** Session metrics from the agent */
export interface SessionMetrics {
  turns: number;
  inputTokens: number;
  outputTokens: number;
  model: string | null;
  contextUtilization: number;
}

/** Permission option from agent */
interface PermissionOption {
  optionId: string;
  kind: "allow_always" | "allow_once" | "deny" | string;
  label: string;
}

/** Tool call update from agent */
interface ToolCallUpdate {
  title: string;
  toolName: string;
  status: string;
}

/** MCP server configuration */
export interface MCPServerConfig {
  name: string;
  command: string;
  args: string[];
  env?: Record<string, string>;
}

// =============================================================================
// JSON-RPC Message Types
// =============================================================================

interface JsonRpcRequest {
  jsonrpc: "2.0";
  id: number;
  method: string;
  params?: Record<string, unknown>;
}

interface JsonRpcResponse {
  jsonrpc: "2.0";
  id: number;
  result?: unknown;
  error?: {
    code: number;
    message: string;
    data?: unknown;
  };
}

interface JsonRpcNotification {
  jsonrpc: "2.0";
  method: string;
  params?: Record<string, unknown>;
}

// =============================================================================
// ACP Client
// =============================================================================

export class ACPClient {
  private command: string[];
  private cwd: string | null;
  private mcpServers: MCPServerConfig[];
  private onUpdate: ((update: SessionUpdate) => void) | null;

  private proc: Subprocess<"pipe", "pipe", "inherit"> | null = null;
  private requestId = 0;
  private pendingRequests = new Map<number, {
    resolve: (value: unknown) => void;
    reject: (error: Error) => void;
  }>();
  private buffer = "";

  constructor(options: {
    command: string[];
    cwd?: string;
    mcpServers?: MCPServerConfig[];
    onUpdate?: (update: SessionUpdate) => void;
  }) {
    this.command = options.command;
    this.cwd = options.cwd ?? null;
    this.mcpServers = options.mcpServers ?? [];
    this.onUpdate = options.onUpdate ?? null;
  }

  /** Start the agent process */
  async start(): Promise<Record<string, unknown>> {
    if (this.proc) {
      throw new ACPError("Agent already started");
    }

    console.log(`[ACP] Starting agent: ${this.command.join(" ")}`);

    // Build environment - pass through API keys
    // Filter out undefined values from process.env
    const env: Record<string, string> = {};
    for (const [key, value] of Object.entries(process.env)) {
      if (value !== undefined) {
        env[key] = value;
      }
    }

    // Ensure API keys are passed through
    const keysToForward = [
      "ANTHROPIC_API_KEY",
      "CLAUDE_ACCESS_TOKEN",
      "ANTHROPIC_MODEL",
      "MAX_THINKING_TOKENS",
      "GOOGLE_API_KEY",
      "OPENAI_API_KEY",
    ];

    for (const key of keysToForward) {
      if (process.env[key]) {
        env[key] = process.env[key]!;
      }
    }

    this.proc = spawn({
      cmd: this.command,
      cwd: this.cwd ?? undefined,
      env,
      stdin: "pipe",
      stdout: "pipe",
      stderr: "inherit",
    });

    // Start reading stdout
    this.readLoop();

    // Initialize the connection
    const result = await this.request("initialize", {
      protocolVersion: "2024-11-05",
      capabilities: {
        tools: true,
        resources: true,
        prompts: true,
      },
      clientInfo: {
        name: "hirsel",
        version: "0.1.0",
      },
    });

    console.log(`[ACP] Agent initialized`);
    return result as Record<string, unknown>;
  }

  /** Stop the agent process */
  async stop(): Promise<void> {
    if (!this.proc) return;

    console.log("[ACP] Stopping agent");

    // Send shutdown request
    try {
      await this.request("shutdown", {});
    } catch {
      // Ignore errors during shutdown
    }

    // Kill the process
    this.proc.kill();
    this.proc = null;

    // Clear pending requests
    for (const [, { reject }] of this.pendingRequests) {
      reject(new ACPError("Agent stopped"));
    }
    this.pendingRequests.clear();

    console.log("[ACP] Agent stopped");
  }

  /** Kill the agent process immediately without graceful shutdown */
  kill(): void {
    if (!this.proc) return;

    console.log("[ACP] Killing agent");
    this.proc.kill();
    this.proc = null;

    // Clear pending requests
    for (const [, { reject }] of this.pendingRequests) {
      reject(new ACPError("Agent killed"));
    }
    this.pendingRequests.clear();
  }

  /** Create a new session */
  async newSession(options: {
    cwd: string;
    mcpServers?: MCPServerConfig[];
    mode?: string;
  }): Promise<string> {
    if (!this.proc) {
      throw new ACPError("Agent not started");
    }

    console.log(`[ACP] Creating new session, cwd=${options.cwd}`);

    const servers = options.mcpServers ?? this.mcpServers;

    const result = await this.request("session/new", {
      cwd: options.cwd,
      mcpServers: servers.map((s) => ({
        name: s.name,
        command: s.command,
        args: s.args,
        env: s.env ? Object.entries(s.env).map(([name, value]) => ({ name, value })) : undefined,
      })),
    }) as { sessionId: string };

    console.log(`[ACP] Session created: ${result.sessionId}`);

    // Set mode if specified
    if (options.mode) {
      await this.setMode(result.sessionId, options.mode);
    }

    return result.sessionId;
  }

  /** Load an existing session (creates new since ACP doesn't support resume) */
  async loadSession(options: {
    sessionId: string;
    cwd: string;
    mcpServers?: MCPServerConfig[];
  }): Promise<string> {
    console.log("[ACP] Creating new session (resume not supported via ACP)");
    return this.newSession({
      cwd: options.cwd,
      mcpServers: options.mcpServers,
    });
  }

  /** Set session mode (e.g., bypassPermissions) */
  async setMode(sessionId: string, mode: string): Promise<void> {
    if (!this.proc) {
      throw new ACPError("Agent not started");
    }

    console.log(`[ACP] Setting mode to ${mode} for session ${sessionId}`);

    await this.request("session/setMode", {
      sessionId,
      modeId: mode,
    });

    console.log(`[ACP] Mode set to ${mode}`);
  }

  /** Send a prompt to the agent */
  async prompt(options: {
    sessionId: string;
    text: string;
    resources?: Array<{ type: string; text?: string; [key: string]: unknown }>;
  }): Promise<Record<string, unknown>> {
    if (!this.proc) {
      throw new ACPError("Agent not started");
    }

    console.log(`[ACP] Sending prompt (${options.text.length} chars)`);

    const promptContent = [
      { type: "text", text: options.text },
    ];

    if (options.resources) {
      for (const resource of options.resources) {
        if (resource.type === "text" && resource.text) {
          promptContent.push({ type: "text", text: resource.text });
        } else {
          promptContent.push(resource as { type: string; text: string });
        }
      }
    }

    const result = await this.request("session/prompt", {
      sessionId: options.sessionId,
      prompt: promptContent,
    });

    console.log(`[ACP] Prompt completed`);
    return result as Record<string, unknown>;
  }

  /** Check if agent process is running */
  isRunning(): boolean {
    return this.proc !== null;
  }

  // ===========================================================================
  // JSON-RPC Communication
  // ===========================================================================

  private async request(method: string, params: Record<string, unknown>): Promise<unknown> {
    if (!this.proc?.stdin) {
      throw new ACPError("Agent not started or stdin not available");
    }

    const id = ++this.requestId;
    const request: JsonRpcRequest = {
      jsonrpc: "2.0",
      id,
      method,
      params,
    };

    const message = JSON.stringify(request) + "\n";
    this.proc.stdin.write(message);

    return new Promise((resolve, reject) => {
      this.pendingRequests.set(id, { resolve, reject });

      // Timeout after 5 minutes
      setTimeout(() => {
        if (this.pendingRequests.has(id)) {
          this.pendingRequests.delete(id);
          reject(new ACPError(`Request ${method} timed out`));
        }
      }, 5 * 60 * 1000);
    });
  }

  private async readLoop(): Promise<void> {
    if (!this.proc?.stdout) return;

    const reader = this.proc.stdout.getReader();
    const decoder = new TextDecoder();

    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        this.buffer += decoder.decode(value, { stream: true });
        this.processBuffer();
      }
    } catch (e) {
      console.error("[ACP] Read error:", e);
    }
  }

  private processBuffer(): void {
    const lines = this.buffer.split("\n");
    this.buffer = lines.pop() ?? "";

    for (const line of lines) {
      if (!line.trim()) continue;

      try {
        const message = JSON.parse(line);
        this.handleMessage(message);
      } catch (e) {
        console.error("[ACP] Failed to parse message:", line);
      }
    }
  }

  private handleMessage(message: JsonRpcResponse | JsonRpcNotification): void {
    // Handle response to a request
    if ("id" in message && message.id !== undefined) {
      const pending = this.pendingRequests.get(message.id);
      if (pending) {
        this.pendingRequests.delete(message.id);

        if (message.error) {
          pending.reject(new ACPError(message.error.message));
        } else {
          pending.resolve(message.result);
        }
      }
      return;
    }

    // Handle notification
    if ("method" in message) {
      this.handleNotification(message as JsonRpcNotification);
    }
  }

  private handleNotification(notification: JsonRpcNotification): void {
    const { method, params } = notification;

    // Handle permission requests - auto-approve
    if (method === "permission/request") {
      this.handlePermissionRequest(params as {
        sessionId: string;
        options: PermissionOption[];
        toolCall?: ToolCallUpdate;
      });
      return;
    }

    // Handle session updates
    if (method.startsWith("session/")) {
      const updateType = method.replace("session/", "");
      const sessionId = (params as { sessionId?: string })?.sessionId ?? "unknown";

      if (this.onUpdate) {
        this.onUpdate({
          sessionId,
          updateType,
          content: params ?? {},
        });
      }
      return;
    }

    console.log(`[ACP] Unhandled notification: ${method}`);
  }

  private async handlePermissionRequest(params: {
    sessionId: string;
    options: PermissionOption[];
    toolCall?: ToolCallUpdate;
  }): Promise<void> {
    const { options, toolCall } = params;

    // Find best option: prefer allow_always > allow_once
    let selectedId = "allow";
    for (const opt of options) {
      if (opt.kind === "allow_always") {
        selectedId = opt.optionId;
        break;
      }
      if (opt.kind === "allow_once") {
        selectedId = opt.optionId;
      }
    }

    console.log(`[ACP] Auto-approving permission: ${toolCall?.title ?? "unknown"}`);

    // Send response
    if (this.proc?.stdin) {
      const response: JsonRpcRequest = {
        jsonrpc: "2.0",
        id: ++this.requestId,
        method: "permission/respond",
        params: {
          selectedOptionId: selectedId,
        },
      };
      this.proc.stdin.write(JSON.stringify(response) + "\n");
    }
  }
}

// =============================================================================
// Helper Functions
// =============================================================================

/** Create MCP server configuration */
export function createMCPServerConfig(
  name: string,
  command: string,
  args: string[],
  env?: Record<string, string>
): MCPServerConfig {
  return { name, command, args, env };
}

/** Get session metrics (requires Claude Code specific API) */
export async function getSessionMetrics(
  sessionId: string,
  projectPath: string
): Promise<SessionMetrics | null> {
  // This requires parsing Claude's session file
  // For now, return null - can be implemented later
  console.log(`[ACP] getSessionMetrics not yet implemented for ${sessionId}`);
  return null;
}
