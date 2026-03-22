/**
 * Shared type definitions for the Settings modal
 */

export type SettingsTab = 'appearance' | 'shortcuts' | 'backend' | 'about';
export type BackendSection =
  | 'connection'
  | 'agents'
  | 'runners'
  | 'defaults'
  | 'git'
  | 'data'
  | 'services';

export interface RunnerContainer {
  image: string;
}

export interface Runner {
  container?: RunnerContainer;
}

export interface BackendConnection {
  url: string;
  apiKey?: string;
}

// Storage config
export interface StorageConfig {
  provider: 's3' | 'minio';
  endpoint?: string;
  bucket: string;
  region?: string;
  accessKeyId?: string;
  secretAccessKey?: string;
}

export interface Settings {
  evalTimeout: number;
  humanInTheLoop: boolean;
  userMessagePause: 'sender' | 'all' | 'none';
  autoLearn: boolean;
  contextWarningThreshold: number;
  coordinatorPort: number;
  backend: BackendConnection;
  runners: Record<string, Runner>;
  defaultRunner?: string;
  storage?: {
    defaultStorage?: string;
    configs: Record<string, StorageConfig>;
  };
  git?: {
    configuredProviders?: string[];
    defaultProvider?: string;
  };
  llm?: {
    provider?: 'codex' | 'openrouter';
    openrouterBaseUrl?: string;
  };
}

export interface BackendHealth {
  status: 'checking' | 'online' | 'offline';
  error?: string;
}

export interface CodexDeviceStartResponse {
  deviceAuthId: string;
  userCode: string;
  verifyUrl: string;
  interval: number;
}

export interface CodexDevicePollResponse {
  status: 'pending' | 'approved';
  authorizationCode?: string;
  codeVerifier?: string;
}

export interface CodexDeviceExchangeResponse {
  status: 'ok';
  expiresAt: number;
}

export const defaultSettings = (): Settings => ({
  evalTimeout: 300,
  humanInTheLoop: true,
  userMessagePause: 'sender',
  autoLearn: false,
  contextWarningThreshold: 0.5,
  coordinatorPort: 19700,
  backend: {
    url: '',
    apiKey: '',
  },
  runners: {},
  llm: {
    provider: 'codex',
    openrouterBaseUrl: '',
  },
});
