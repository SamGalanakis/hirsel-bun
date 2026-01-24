/**
 * Default configurations for settings modal
 */

import type {
  AgentAuth,
  AuthMethod,
  ContainerConfig,
  GitConfig,
  HostType,
  NavigationState,
  OrchestratorProfile,
  RunnerConfig,
  S3Config,
  Settings,
  SnapshotConfig,
  SnapshotStrategyType,
  SpriteHostConfig,
  SshHostConfig,
  StorageConfig,
  StorageProvider,
} from './types';

/**
 * Default agent auth configuration
 */
export const defaultAgentAuth = (): AgentAuth => ({
  method: 'env',
  apiKey: null,
  envVar: null,
});

/**
 * Default SSH host configuration
 */
export const defaultSshHostConfig = (): SshHostConfig => ({
  type: 'ssh',
  address: '',
  port: 22,
  sshKey: null,
  workBase: '/tmp/hirsel-remote',
  location: null,
});

/**
 * Default Sprite host configuration
 */
export const defaultSpriteHostConfig = (): SpriteHostConfig => ({
  type: 'sprite',
  apiToken: null,
  checkpoint: null,
  autoDestroy: true,
  idleTimeoutSecs: 30,
  apiUrl: 'https://api.sprites.dev',
  useFilePush: false,
});

/**
 * Default container configuration
 */
export const defaultContainerConfig = (): ContainerConfig => ({
  image: '',
});

/**
 * Default local runner configuration
 */
export const defaultLocalRunnerConfig = (): RunnerConfig => ({
  host: { type: 'local' },
});

/**
 * Default SSH runner configuration (host + no container)
 */
export const defaultSshRunnerConfig = (): RunnerConfig => ({
  host: defaultSshHostConfig(),
});

/**
 * Default Sprite runner configuration (host, no container - Sprites don't support Docker)
 */
export const defaultSpriteRunnerConfig = (): RunnerConfig => ({
  host: defaultSpriteHostConfig(),
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
export const defaultTailscaleAccess = (): {
  type: 'tailscale';
  oauth_client_id: string;
  oauth_client_secret: string;
  tag: string | null;
} => ({
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
 * Default S3 config for a given provider
 */
export const defaultS3Config = (provider: StorageProvider = 's3'): S3Config => ({
  provider,
  endpoint: provider === 'tigris' ? 'https://fly.storage.tigris.dev' : undefined,
  bucket: '',
  region: provider === 'tigris' ? 'auto' : 'us-east-1',
  accessKeyId: undefined,
  secretAccessKey: undefined,
});

/**
 * Default storage configuration
 */
export const defaultStorageConfig = (): StorageConfig => ({
  files: 'local',
  storages: {},
  defaultStorage: undefined,
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
  storage: defaultStorageConfig(),
});

/**
 * Get auth method display label
 */
export function getAuthMethodLabel(method: AuthMethod): string {
  switch (method) {
    case 'env':
      return 'Environment Variable';
    case 'apiKey':
      return 'API Key';
    case 'oauth':
      return 'OAuth';
    default:
      return method;
  }
}

/**
 * Get default env var for an agent
 */
export function getDefaultEnvVar(agent: string): string {
  switch (agent) {
    case 'claude':
      return 'ANTHROPIC_API_KEY';
    case 'gemini':
      return 'GOOGLE_API_KEY';
    case 'codex':
      return 'OPENAI_API_KEY';
    case 'goose':
      return 'ANTHROPIC_API_KEY';
    default:
      return '';
  }
}

/**
 * Get host type icon
 */
export function getHostIcon(type: HostType): string {
  switch (type) {
    case 'local':
      return 'laptop';
    case 'client':
      return 'monitor';
    case 'ssh':
      return 'server';
    case 'sprite':
      return 'cloud';
    default:
      return 'laptop';
  }
}

/**
 * Get host type label
 */
export function getHostTypeLabel(type: HostType): string {
  switch (type) {
    case 'local':
      return 'Local';
    case 'client':
      return 'Client';
    case 'ssh':
      return 'SSH';
    case 'sprite':
      return 'Sprites';
    default:
      return type;
  }
}

/**
 * Get access type label
 */
export function getAccessTypeLabel(type: string): string {
  switch (type) {
    case 'direct':
      return 'Direct';
    case 'tailscale':
      return 'Tailscale';
    default:
      return type;
  }
}

/**
 * Get default snapshot strategy for a host type
 */
export function getDefaultSnapshotStrategy(hostType: HostType): SnapshotStrategyType {
  switch (hostType) {
    case 'local':
    case 'ssh':
    case 'client':
      return 'persistent_disk';
    case 'sprite':
      return 'sprite_checkpoint';
    default:
      return 'persistent_disk';
  }
}

/**
 * Get snapshot strategy label
 */
export function getSnapshotStrategyLabel(strategy: SnapshotStrategyType): string {
  switch (strategy) {
    case 'persistent_disk':
      return 'Persistent Disk';
    case 's3':
      return 'S3 Storage';
    case 'sprite_checkpoint':
      return 'Sprite Checkpoint';
    default:
      return strategy;
  }
}

/**
 * Get snapshot strategy description
 */
export function getSnapshotStrategyDescription(strategy: SnapshotStrategyType): string {
  switch (strategy) {
    case 'persistent_disk':
      return 'Files remain on disk (no transfer needed)';
    case 's3':
      return 'Archive and upload to S3-compatible storage';
    case 'sprite_checkpoint':
      return 'Native Sprites VM checkpoint (recommended)';
    default:
      return '';
  }
}

/**
 * Get available snapshot strategies for a host type
 */
export function getAvailableSnapshotStrategies(hostType: HostType): SnapshotStrategyType[] {
  switch (hostType) {
    case 'local':
    case 'ssh':
    case 'client':
      return ['persistent_disk', 's3'];
    case 'sprite':
      return ['sprite_checkpoint', 's3'];
    default:
      return ['persistent_disk'];
  }
}

/**
 * Create default snapshot config for strategy type
 */
export function createSnapshotConfig(strategy: SnapshotStrategyType): SnapshotConfig {
  switch (strategy) {
    case 'persistent_disk':
      return { type: 'persistent_disk' };
    case 's3':
      return { type: 's3' };
    case 'sprite_checkpoint':
      return { type: 'sprite_checkpoint' };
    default:
      return { type: 'persistent_disk' };
  }
}

/**
 * Get storage provider label
 */
export function getStorageProviderLabel(provider: StorageProvider): string {
  switch (provider) {
    case 's3':
      return 'AWS S3';
    case 'tigris':
      return 'Tigris (Fly.io)';
    default:
      return provider;
  }
}

/**
 * Get storage provider description
 */
export function getStorageProviderDescription(provider: StorageProvider): string {
  switch (provider) {
    case 's3':
      return 'Amazon Web Services S3 object storage';
    case 'tigris':
      return 'S3-compatible storage from Fly.io';
    default:
      return '';
  }
}

/**
 * Get all available storage providers
 */
export function getAvailableStorageProviders(): StorageProvider[] {
  return ['s3', 'tigris'];
}

/**
 * Get default endpoint for a storage provider
 */
export function getDefaultEndpoint(provider: StorageProvider): string | undefined {
  switch (provider) {
    case 'tigris':
      return 'https://fly.storage.tigris.dev';
    default:
      return undefined;
  }
}

/**
 * Get default region for a storage provider
 */
export function getDefaultRegion(provider: StorageProvider): string {
  switch (provider) {
    case 'tigris':
      return 'auto';
    default:
      return 'us-east-1';
  }
}
