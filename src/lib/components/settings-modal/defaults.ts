/**
 * Default configurations for settings modal
 */

import type { AgentAuth, SshRunnerConfig, SpriteRunnerConfig, Settings, AuthMethod, OrchestratorProfile, GitConfig, NavigationState } from './types';

/**
 * Default agent auth configuration
 */
export const defaultAgentAuth = (): AgentAuth => ({
  method: 'env',
  apiKey: null,
  envVar: null,
});

/**
 * Default SSH runner configuration
 */
export const defaultSshRunnerConfig = (): SshRunnerConfig => ({
  type: 'ssh',
  host: '',
  sshKey: null,
  sshPort: 22,
  workBase: '/tmp/hirsel-remote',
  location: null,
});

/**
 * Default Sprite runner configuration
 */
export const defaultSpriteRunnerConfig = (): SpriteRunnerConfig => ({
  type: 'sprite',
  apiToken: null,
  baseCheckpoint: null,
  autoDestroy: true,
  idleTimeoutSecs: 30,
  apiUrl: 'https://api.sprites.dev',
});

/**
 * Default remote orchestrator profile configuration
 */
export const defaultRemoteProfile = (): OrchestratorProfile => ({
  mode: 'remote',
  url: null,
  apiKey: null,
  access: { type: 'direct' },
});

/**
 * Default Tailscale access configuration
 */
export const defaultTailscaleAccess = (): { type: 'tailscale'; oauth_client_id: string; oauth_client_secret: string; tag: string | null } => ({
  type: 'tailscale',
  oauth_client_id: '',
  oauth_client_secret: '',
  tag: null,
});

/**
 * Default git configuration
 */
export const defaultGitConfig = (): GitConfig => ({
  defaultProvider: null,
  configuredProviders: [],
});

/**
 * Default navigation state - starts with default profile selected
 */
export const defaultNavigationState = (): NavigationState => ({
  activeSection: 'profile',
  appSection: 'theme',
  selectedProfile: 'local',
  profileTab: 'defaults',
});

/**
 * Default settings
 */
export const defaultSettings = (): Settings => ({
  agentCommand: 'claude-code-acp',
  evalTimeout: 1800,
  autoLearn: true,
  maxIterations: null,
  userMessagePause: 'sender',
  humanInTheLoop: true,
  compactionEnabled: true,
  compactionThreshold: 10000,
  compactionKeepMessages: 40,
  autoImprove: true,
  contextWarningThreshold: 0.5,
  coordinatorPort: 19700,
  auth: {
    defaultMethod: 'env' as AuthMethod,
    claude: null,
    gemini: null,
    codex: null,
    goose: null,
  },
  runners: {},
  defaultRunner: null,
  workerRunners: {},
  profiles: {
    local: { mode: 'local', url: null, apiKey: null, access: { type: 'direct' } },
  },
  defaultProfile: 'local',
  git: defaultGitConfig(),
});

/**
 * Get auth method display label
 */
export function getAuthMethodLabel(method: AuthMethod): string {
  switch (method) {
    case 'env': return 'Environment Variable';
    case 'apiKey': return 'API Key';
    case 'oauth': return 'OAuth';
    default: return method;
  }
}

/**
 * Get default env var for an agent
 */
export function getDefaultEnvVar(agent: string): string {
  switch (agent) {
    case 'claude': return 'ANTHROPIC_API_KEY';
    case 'gemini': return 'GOOGLE_API_KEY';
    case 'codex': return 'OPENAI_API_KEY';
    case 'goose': return 'ANTHROPIC_API_KEY';
    default: return '';
  }
}

/**
 * Get runner type icon
 */
export function getRunnerIcon(type: string): string {
  switch (type) {
    case 'ssh': return 'server';
    case 'sprite': return 'cloud';
    default: return 'laptop';
  }
}

/**
 * Get runner type label
 */
export function getRunnerTypeLabel(type: string): string {
  switch (type) {
    case 'ssh': return 'SSH';
    case 'sprite': return 'Sprites';
    default: return type;
  }
}

/**
 * Get access type label
 */
export function getAccessTypeLabel(type: string): string {
  switch (type) {
    case 'direct': return 'Direct';
    case 'tailscale': return 'Tailscale';
    default: return type;
  }
}
