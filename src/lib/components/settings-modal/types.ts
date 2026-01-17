/**
 * Type definitions for settings modal
 */

export type AuthMethod = 'env' | 'apiKey' | 'oauth';
export type RunnerType = 'ssh' | 'sprite';

export interface AgentAuth {
  method: AuthMethod;
  apiKey: string | null;
  envVar: string | null;
}

export interface AuthConfig {
  defaultMethod: AuthMethod;
  claude: AgentAuth | null;
  gemini: AgentAuth | null;
  codex: AgentAuth | null;
  goose: AgentAuth | null;
}

// Runner configs
export interface SshRunnerConfig {
  type: 'ssh';
  host: string;
  sshKey: string | null;
  sshPort: number;
  workBase: string;
  location: string | null;
}

export interface SpriteRunnerConfig {
  type: 'sprite';
  apiToken: string | null;
  baseCheckpoint: string | null;
  autoDestroy: boolean;
  idleTimeoutSecs: number;
  apiUrl: string;
}

export type RunnerConfig = { type: 'local' } | SshRunnerConfig | SpriteRunnerConfig;

export interface Settings {
  agentCommand: string;
  evalTimeout: number;
  autoLearn: boolean;
  maxIterations: number | null;
  userMessagePause: string;
  humanInTheLoop: boolean;
  compactionEnabled: boolean;
  compactionThreshold: number | null;
  compactionKeepMessages: number;
  autoImprove: boolean;
  contextWarningThreshold: number;
  coordinatorPort: number;
  auth: AuthConfig;
  runners: Record<string, RunnerConfig>;
  defaultRunner: string | null;
  workerRunners: Record<string, string>;
}
