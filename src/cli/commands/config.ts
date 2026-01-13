/**
 * hirsel config [agent] - Configure agent
 *
 * Interactive agent selection or direct configuration.
 */

import { isJsonOutput, jsonOutput, ICONS } from "../index";
import { AGENT_PRESETS } from "../../core/config";
import { dim, bold, colorize } from "../../shared/theme";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "fs";
import { join } from "path";
import { homedir } from "os";
import { spawn, which } from "bun";

// Get hirsel root directory
function getHirselRoot(): string {
  return process.env.HIRSEL_ROOT || join(homedir(), ".hirsel");
}

// Get config file path
function getConfigPath(): string {
  return join(getHirselRoot(), "config.toml");
}

// Read current config
function readConfig(): Record<string, unknown> {
  const configPath = getConfigPath();
  if (!existsSync(configPath)) {
    return {};
  }

  try {
    const content = readFileSync(configPath, "utf-8");
    // Simple TOML parsing for [agent] section
    const config: Record<string, unknown> = {};
    const lines = content.split("\n");
    let currentSection = "";

    for (const line of lines) {
      const trimmed = line.trim();
      if (trimmed.startsWith("[") && trimmed.endsWith("]")) {
        currentSection = trimmed.slice(1, -1);
        config[currentSection] = {};
      } else if (trimmed.includes("=") && currentSection) {
        const [key, ...valueParts] = trimmed.split("=");
        const value = valueParts.join("=").trim();
        // Parse value
        let parsed: unknown = value;
        if (value.startsWith("[") && value.endsWith("]")) {
          // Array - simple parsing
          const items = value.slice(1, -1).split(",").map((s) => {
            const t = s.trim();
            if (t.startsWith('"') && t.endsWith('"')) {
              return t.slice(1, -1);
            }
            return t;
          });
          parsed = items;
        } else if (value.startsWith('"') && value.endsWith('"')) {
          parsed = value.slice(1, -1);
        } else if (value === "true") {
          parsed = true;
        } else if (value === "false") {
          parsed = false;
        } else if (!isNaN(Number(value))) {
          parsed = Number(value);
        }
        (config[currentSection] as Record<string, unknown>)[key.trim()] = parsed;
      }
    }

    return config;
  } catch (error) {
    console.error("Error reading config:", error);
    return {};
  }
}

// Write config
function writeConfig(data: Record<string, unknown>): void {
  const root = getHirselRoot();
  if (!existsSync(root)) {
    mkdirSync(root, { recursive: true });
  }

  const lines: string[] = [];
  for (const [section, values] of Object.entries(data)) {
    if (typeof values === "object" && values !== null) {
      lines.push(`[${section}]`);
      for (const [key, value] of Object.entries(values as Record<string, unknown>)) {
        if (Array.isArray(value)) {
          const items = value.map((v) => `"${v}"`).join(", ");
          lines.push(`${key} = [${items}]`);
        } else if (typeof value === "string") {
          lines.push(`${key} = "${value}"`);
        } else if (typeof value === "boolean" || typeof value === "number") {
          lines.push(`${key} = ${value}`);
        }
      }
      lines.push("");
    }
  }

  writeFileSync(getConfigPath(), lines.join("\n"));
}

// Get current agent from config
function getCurrentAgent(): string | null {
  const config = readConfig();
  const agentConfig = config.agent as Record<string, unknown> | undefined;
  if (!agentConfig?.command) {
    return null;
  }

  const command = agentConfig.command as string[];
  for (const [name, preset] of Object.entries(AGENT_PRESETS)) {
    if (JSON.stringify(preset.command) === JSON.stringify(command)) {
      return name;
    }
  }

  return null;
}

// Interactive agent selection using simple numbered menu
async function interactiveAgentSelect(): Promise<string | null> {
  const agents = Object.keys(AGENT_PRESETS);
  const current = getCurrentAgent();

  console.log(bold("Select AI Agent\n"));

  for (let i = 0; i < agents.length; i++) {
    const name = agents[i];
    const preset = AGENT_PRESETS[name as keyof typeof AGENT_PRESETS];
    const isCurrent = name === current;
    const marker = isCurrent ? colorize(ICONS.working, "yellow") : dim(ICONS.idle);
    const num = `${i + 1}`;

    if (isCurrent) {
      console.log(`  ${marker} ${bold(num)} ${bold(name.padEnd(12))} ${dim(preset.desc)}`);
    } else {
      console.log(`  ${marker} ${dim(num)} ${dim(name.padEnd(12))} ${dim(preset.desc)}`);
    }
  }

  console.log();
  process.stdout.write("> ");

  // Read input
  const reader = Bun.stdin.stream().getReader();
  const decoder = new TextDecoder();

  try {
    const { value, done } = await reader.read();
    if (done || !value) {
      return null;
    }

    const input = decoder.decode(value).trim().toLowerCase();

    // Check for number input
    if (/^\d+$/.test(input)) {
      const idx = parseInt(input, 10) - 1;
      if (idx >= 0 && idx < agents.length) {
        return agents[idx];
      }
    }

    // Check for name input
    if (input in AGENT_PRESETS) {
      return input;
    }

    return null;
  } finally {
    reader.releaseLock();
  }
}

// Main command handler
export default async function config(args: string[]): Promise<void> {
  let agentName: string | null = args[0] || null;

  // If no agent specified, go interactive
  if (!agentName) {
    agentName = await interactiveAgentSelect();
    if (!agentName) {
      console.log(dim("No changes made"));
      return;
    }
  }

  agentName = agentName.toLowerCase();

  // Validate agent name
  if (!(agentName in AGENT_PRESETS)) {
    console.error(`${ICONS.error} Unknown agent: ${agentName}`);
    console.error(dim(`Available: ${Object.keys(AGENT_PRESETS).join(", ")}`));
    process.exit(1);
  }

  const preset = AGENT_PRESETS[agentName as keyof typeof AGENT_PRESETS];

  // Check if command is installed
  const cmd = preset.command[0];
  const cmdPath = which(cmd);
  if (!cmdPath) {
    console.error(`${ICONS.error} Command not found: ${cmd}`);
    if (agentName === "claude") {
      console.log();
      console.log(dim("Install the ACP adapter:"));
      console.log("  npm install -g @zed-industries/claude-code-acp");
    } else {
      console.log();
      console.log(dim(`Make sure '${cmd}' is installed and in your PATH`));
    }
    process.exit(1);
  }

  // Update config
  const currentConfig = readConfig();
  currentConfig.agent = {
    command: preset.command,
  };
  writeConfig(currentConfig);

  console.log();
  console.log(`${ICONS.done} Agent set to ${bold(agentName)}`);
  console.log(dim(preset.desc));
}
