/**
 * Shared type definitions for the Settings modal
 */

export type SettingsTab = 'appearance' | 'shortcuts' | 'backend' | 'about';
export type BackendSection = 'connection' | 'llm' | 'services';

export interface BackendConnection {
  url: string;
  apiKey?: string;
}

export interface Settings {
  backend: BackendConnection;
  llm?: {
    provider?: 'codex' | 'openrouter';
    openrouterBaseUrl?: string;
  };
  mcpServersText: string;
}

export interface McpStdioServerConfig {
  transport: 'stdio';
  command: string;
  args?: string[];
  env?: Record<string, string>;
  cwd?: string | null;
  startupTimeoutMs?: number;
  callTimeoutMs?: number;
}

export type McpServerConfig = McpStdioServerConfig;

export interface SettingsResponse {
  backend?: BackendConnection;
  llm?: Settings['llm'];
  mcpServers?: Record<string, McpServerConfig>;
}

export interface SettingsSaveRequest {
  backend: BackendConnection;
  llm?: Settings['llm'];
  mcpServers: Record<string, McpServerConfig>;
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
  backend: {
    url: '',
    apiKey: '',
  },
  llm: {
    provider: 'codex',
    openrouterBaseUrl: '',
  },
  mcpServersText: '',
});
