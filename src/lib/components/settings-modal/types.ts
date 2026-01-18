/**
 * Type definitions for settings modal
 */

// Navigation types for profile-centric UI
export type AppSection = 'theme' | 'controls';
export type ProfileTab = 'connection' | 'agents' | 'runners' | 'defaults' | 'git' | 'data';
export type ActiveSection = 'app' | 'profile';

export interface NavigationState {
  activeSection: ActiveSection;
  appSection: AppSection;
  selectedProfile: string | null;
  profileTab: ProfileTab;
}

// Profile settings cache for remote profiles
export interface ProfileSettingsCache {
  settings: Partial<Settings>;
  loadedAt: number;
  dirty: boolean;
}

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

// Orchestrator profile types
export type OrchestratorMode = 'local' | 'remote';
export type OrchestratorAccessType = 'direct' | 'tailscale';

export interface TailscaleAccess {
  type: 'tailscale';
  oauth_client_id: string;
  oauth_client_secret: string;
  tag: string | null;
}

export interface DirectAccess {
  type: 'direct';
}

export type OrchestratorAccess = DirectAccess | TailscaleAccess;

export interface OrchestratorProfile {
  mode: OrchestratorMode;
  url: string | null;
  apiKey: string | null;
  access: OrchestratorAccess;
}

export type GitProvider = 'github';

export interface GitConfig {
  defaultProvider: GitProvider | null;
  configuredProviders: GitProvider[];
}

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
  profiles: Record<string, OrchestratorProfile>;
  defaultProfile: string;
  git: GitConfig;
}

// Remote config response from server API
export interface RemoteConfig {
  agentCommand: string[];
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
  auth: AuthConfig;
  runners: Record<string, RunnerConfig>;
  defaultRunner: string | null;
  workerRunners: Record<string, string>;
  git: { defaultProvider: string | null; configuredProviders: string[] };
}

// Tailscale info for "This Machine" feature
export interface TailscaleInfo {
  connected: boolean;
  hostname: string | null;
  dns_name: string | null;
  tailscale_ips: string[];
}

// SSH runner health check result
export interface SshCheckResult {
  reachable: boolean;
  error: string | null;
  latency_ms: number | null;
}

// Runner health status for polling
export interface RunnerHealthStatus {
  status: 'checking' | 'online' | 'offline';
  latencyMs?: number;
  error?: string;
}
