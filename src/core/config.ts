import { homedir } from "os";
import { join } from "path";
import { existsSync, readFileSync, mkdirSync, writeFileSync } from "fs";

// Agent types supported
export enum AgentType {
  CLAUDE = "claude",
  GEMINI = "gemini",
  CODEX = "codex",
  GOOSE = "goose",
  UNKNOWN = "unknown",
}

// Detect agent type from command
export function getAgentType(command: string[]): AgentType {
  if (!command.length) return AgentType.UNKNOWN;
  const cmd = command[0].toLowerCase();
  if (cmd.includes("claude")) return AgentType.CLAUDE;
  if (cmd.includes("gemini")) return AgentType.GEMINI;
  if (cmd.includes("codex")) return AgentType.CODEX;
  if (cmd.includes("goose")) return AgentType.GOOSE;
  return AgentType.UNKNOWN;
}

// Get default env var for agent type
export function getDefaultEnvVar(agentType: AgentType): string | null {
  switch (agentType) {
    case AgentType.CLAUDE:
      return "ANTHROPIC_API_KEY";
    case AgentType.GEMINI:
      return "GOOGLE_API_KEY";
    case AgentType.CODEX:
      return "OPENAI_API_KEY";
    case AgentType.GOOSE:
      return "ANTHROPIC_API_KEY";
    default:
      return null;
  }
}

// Agent presets for easy configuration
export const AGENT_PRESETS: Record<
  string,
  { command: string[]; desc: string }
> = {
  claude: { command: ["claude-code-acp"], desc: "Anthropic Claude Code" },
  gemini: { command: ["gemini"], desc: "Google Gemini CLI" },
  opencode: { command: ["opencode", "acp"], desc: "OpenCode" },
  codex: { command: ["codex"], desc: "OpenAI Codex CLI" },
  goose: { command: ["goose"], desc: "Block Goose" },
};

// Worker scale configuration
export interface WorkerScale {
  min: number;
  max: number | null;
  isFixed: boolean;
}

export function parseWorkerScale(spec: string): WorkerScale {
  spec = spec.trim();

  // Fixed number: "3"
  if (/^\d+$/.test(spec)) {
    const n = parseInt(spec, 10);
    return { min: n, max: n, isFixed: true };
  }

  // Range: "1-5"
  const rangeMatch = spec.match(/^(\d+)-(\d+)$/);
  if (rangeMatch) {
    return {
      min: parseInt(rangeMatch[1], 10),
      max: parseInt(rangeMatch[2], 10),
      isFixed: false,
    };
  }

  // Unbounded: "2+"
  const unboundedMatch = spec.match(/^(\d+)\+$/);
  if (unboundedMatch) {
    return {
      min: parseInt(unboundedMatch[1], 10),
      max: null,
      isFixed: false,
    };
  }

  // Default to 1 fixed
  return { min: 1, max: 1, isFixed: true };
}

// Parse time limit string
export function parseTimeLimit(spec: string): number | null {
  spec = spec.trim().toLowerCase();

  // Just a number (minutes)
  if (/^\d+$/.test(spec)) {
    return parseInt(spec, 10);
  }

  // Minutes: "30m"
  const minMatch = spec.match(/^(\d+)m$/);
  if (minMatch) {
    return parseInt(minMatch[1], 10);
  }

  // Hours: "1h"
  const hourMatch = spec.match(/^(\d+)h$/);
  if (hourMatch) {
    return parseInt(hourMatch[1], 10) * 60;
  }

  // Hours and minutes: "1h30m"
  const hhmMatch = spec.match(/^(\d+)h(\d+)m$/);
  if (hhmMatch) {
    return parseInt(hhmMatch[1], 10) * 60 + parseInt(hhmMatch[2], 10);
  }

  return null;
}

// Configuration interface
export interface Config {
  // Paths
  root: string;
  runsDir: string;
  configFile: string;

  // Agent settings
  agentCommand: string[];
  agentType: AgentType;

  // Defaults
  defaultWorkerScale: string;
  defaultTimeLimit: number | null;

  // Context tracking
  contextWarningThreshold: number;
  contextWindow: number;

  // Behavior
  humanInTheLoop: boolean;
  autoCompleteShell: boolean;
}

// Load config from TOML file
function loadConfigFile(path: string): Record<string, unknown> {
  if (!existsSync(path)) {
    return {};
  }

  try {
    const content = readFileSync(path, "utf-8");
    // Simple TOML parser for our needs
    const result: Record<string, Record<string, unknown>> = {};
    let currentSection = "";

    for (const line of content.split("\n")) {
      const trimmed = line.trim();
      if (!trimmed || trimmed.startsWith("#")) continue;

      // Section header
      const sectionMatch = trimmed.match(/^\[([^\]]+)\]$/);
      if (sectionMatch) {
        currentSection = sectionMatch[1];
        result[currentSection] = {};
        continue;
      }

      // Key = value
      const kvMatch = trimmed.match(/^([^=]+)=(.+)$/);
      if (kvMatch && currentSection) {
        const key = kvMatch[1].trim();
        let value: unknown = kvMatch[2].trim();

        // Parse value type
        if (value === "true") value = true;
        else if (value === "false") value = false;
        else if (/^-?\d+$/.test(value as string)) value = parseInt(value as string, 10);
        else if (/^-?\d+\.\d+$/.test(value as string)) value = parseFloat(value as string);
        else if ((value as string).startsWith('"') && (value as string).endsWith('"')) {
          value = (value as string).slice(1, -1);
        } else if ((value as string).startsWith("[") && (value as string).endsWith("]")) {
          // Array of strings
          value = (value as string)
            .slice(1, -1)
            .split(",")
            .map((s) => s.trim().replace(/^"|"$/g, ""));
        }

        result[currentSection][key] = value;
      }
    }

    return result;
  } catch {
    return {};
  }
}

// Save config to TOML file
export function saveConfig(
  path: string,
  data: { agent?: { command?: string[] } }
): void {
  const dir = join(path, "..");
  if (!existsSync(dir)) {
    mkdirSync(dir, { recursive: true });
  }

  const lines: string[] = [];

  if (data.agent) {
    lines.push("[agent]");
    if (data.agent.command) {
      const cmdStr = data.agent.command.map((c) => `"${c}"`).join(", ");
      lines.push(`command = [${cmdStr}]`);
    }
  }

  writeFileSync(path, lines.join("\n") + "\n");
}

// Create config with defaults
export function createConfig(): Config {
  const root = join(homedir(), ".hirsel");
  const configFile = join(root, "config.toml");
  const runsDir = join(root, "runs");

  // Load user config
  const userConfig = loadConfigFile(configFile);
  const agentConfig = (userConfig.agent as Record<string, unknown>) || {};

  // Determine agent command
  const agentCommand = (agentConfig.command as string[]) || ["claude-code-acp"];
  const agentType = getAgentType(agentCommand);

  return {
    // Paths
    root,
    runsDir,
    configFile,

    // Agent
    agentCommand,
    agentType,

    // Defaults
    defaultWorkerScale: "1",
    defaultTimeLimit: null,

    // Context tracking (Claude-specific)
    contextWarningThreshold: 50,
    contextWindow: 200000,

    // Behavior
    humanInTheLoop: true,
    autoCompleteShell: true,
  };
}

// Global config instance
export const config = createConfig();

// Get credentials for current agent
export function getAgentCredentials(): Record<string, string> {
  const envVar = getDefaultEnvVar(config.agentType);
  if (!envVar) return {};

  const value = process.env[envVar];
  if (!value) return {};

  return { [envVar]: value };
}

// Get run directory
export function getRunDir(runName: string): string {
  return join(config.runsDir, runName);
}

// Get database path for a run
export function getDbPath(runName: string): string {
  return join(getRunDir(runName), "hirsel.db");
}

// Ensure directory exists
export function ensureDir(path: string): void {
  if (!existsSync(path)) {
    mkdirSync(path, { recursive: true });
  }
}

// Validate task ID format
const TASK_ID_REGEX = /^[a-z][a-z0-9_]*$/;

export function isValidTaskId(id: string): boolean {
  return TASK_ID_REGEX.test(id);
}

// Run-specific config that combines global config with run directory
export interface RunConfig extends Config {
  runDir: string;
  dbPath: string;
  agent: { command: string[] };
}

export function getRunConfig(runName: string): RunConfig {
  const runDir = getRunDir(runName);
  const dbPath = getDbPath(runName);
  return {
    ...config,
    runDir,
    dbPath,
    agent: { command: config.agentCommand },
  };
}

// Slugify run name
export function slugifyRunName(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "")
    .slice(0, 50);
}
