/**
 * TypeScript type definitions for Hirsel
 *
 * These types mirror the Rust/SQLite models and are used throughout
 * the frontend for type safety with Tauri IPC calls.
 */

// =============================================================================
// Runner Types
// =============================================================================

/** SSH runner configuration */
export interface SshRunnerConfig {
  type: 'ssh';
  host: string;
  sshKey: string | null;
  sshPort: number;
  workBase: string;
  location: string | null;
}

/** Sprite (cloud VM) runner configuration */
export interface SpriteRunnerConfig {
  type: 'sprite';
  apiToken: string | null;
  baseCheckpoint: string | null;
  autoDestroy: boolean;
  idleTimeoutSecs: number;
  apiUrl: string;
}

/** Runner configuration - where workers execute */
export type RunnerConfig = { type: 'local' } | SshRunnerConfig | SpriteRunnerConfig;

/** Runner entry with name for display */
export interface RunnerEntry {
  name: string;
  config: RunnerConfig;
}

// =============================================================================
// Run Types
// =============================================================================

/** Run status values matching Rust Status enum */
export type RunStatus =
  | 'draft'
  | 'idle'
  | 'working'
  | 'paused'
  | 'runaway'
  | 'timed_out'
  | 'eval'
  | 'eval_failed'
  | 'waiting'
  | 'done'
  | 'delivered'
  | 'merged';

/** Summary of a run for the run list panel */
export interface RunSummary {
  name: string;
  status: RunStatus;
  tasksDone: number;
  tasksTotal: number;
  workersActive: number;
  workersTotal: number;
  elapsedMinutes: number;
  timeLimitMinutes: number | null;
  hasUnreadMessages: boolean;
  createdAt: string;
}

/** Full run details for the detail view */
export interface RunDetail {
  name: string;
  status: RunStatus;
  request: string | null;
  projectPath: string | null;
  remoteUrl: string | null;
  branch: string | null;
  workerScale: string | null;
  timeLimitMinutes: number | null;
  startedAt: string | null;
  summary: string | null;
  createdAt: string;
  updatedAt: string;
  iterationCount: number;
  maxIterations: number | null;
  humanInTheLoop: boolean;
  waitingReason: string | null;
  unreadCount: number;
  // Additional fields for status bar display
  tasksDone: number;
  tasksTotal: number;
  workersActive: number;
  workersTotal: number;
  elapsedMinutes: number;
  // Learnings info
  learningsCount: number;
  learningsProcessedAt: string | null;
  // Runner configuration
  runner: string | null;
}

/** Request to update a draft run */
export interface DraftUpdateRequest {
  spec?: string;
  workerScale?: string;
  timeLimitMinutes?: number;
  humanInTheLoop?: boolean;
  projectPath?: string;
  name?: string;
  branch?: string;
  runner?: string;
}

/** Result of validating a repository path/URL */
export interface RepoValidation {
  /** Whether the repo is valid and accessible */
  valid: boolean;
  /** Error message if not valid */
  error: string | null;
  /** Whether this is a remote URL (vs local path) */
  isRemote: boolean;
  /** Available branches in the repository */
  branches: string[];
  /** Currently checked out branch (for local repos) */
  currentBranch: string | null;
  /** The normalized repo URL (with branch stripped if it was in the URL) */
  repoUrl: string;
  /** Branch extracted from URL (if any) */
  urlBranch: string | null;
  /** Whether the URL branch exists in the repo */
  urlBranchValid: boolean;
  /** Whether the directory needs to be created (local paths only) */
  needsDirCreate: boolean;
  /** Whether git needs to be initialized (local paths only) */
  needsGitInit: boolean;
}

/** Run state from the database state table */
export interface RunState {
  id: number;
  status: RunStatus;
  request: string | null;
  projectPath: string | null;
  workerScale: string | null;
  timeLimitMinutes: number | null;
  startedAt: string | null;
  summary: string | null;
  createdAt: string;
  updatedAt: string;
  iterationCount: number;
  maxIterations: number | null;
  humanInTheLoop: boolean;
  waitingReason: string | null;
  unreadCount: number;
  learningsProcessedAt: string | null;
  lastTimeNotificationPct: number | null;
}

// =============================================================================
// Task Types
// =============================================================================

/** Task status values */
export type TaskStatus = 'todo' | 'doing' | 'done';

/** Task from the database */
export interface Task {
  id: string;
  description: string;
  status: TaskStatus;
  claimedBy: string | null;
  claimedAt: string | null;
  completedAt: string | null;
  parentId: string | null;
  blockedBy: string[] | null;
  tokensUsed: number | null;
  createdAt: string;
}

/** Task with computed display properties */
export interface TaskDisplay extends Task {
  isBlocked: boolean;
  children: TaskDisplay[];
  depth: number;
}

// =============================================================================
// Worker Types
// =============================================================================

/** Worker status values */
export type WorkerStatus =
  | 'idle'
  | 'working'
  | 'waiting'
  | 'awaiting'
  | 'paused'
  | 'error';

/** Worker location */
export type WorkerLocation = 'local' | 'remote';

/** Deterministic sheep avatar configuration - generated from worker name */
export interface SheepConfig {
  /** Hat type: 0=none, 1=crown, 2=cowboy, 3=tophat, 4=beanie, 5=wizard, 6=chef, 7=hardhat */
  hat: number;
  /** Wool fluffiness level (0-3) */
  fluffiness: number;
  /** Body width modifier (-2 to +2) */
  bodyWidth: number;
  /** Body height modifier (-2 to +2) */
  bodyHeight: number;
  /** Ear position modifier (-1 to +1) */
  earPosition: number;
  /** Leg length modifier (-1 to +1) */
  legLength: number;
  /** Hue shift for wool color (0-359 degrees) */
  hueShift: number;
  /** Glasses: 0=none, 1=round, 2=square, 3=sunglasses, 4=eyepatch */
  glasses: number;
  /** Bow tie: 0=none, 1=red, 2=blue, 3=gold, 4=pink */
  bowtie: number;
}

/** Worker from the database */
export interface Worker {
  id: number;
  name: string;
  pid: number | null;
  sessionId: string | null;
  status: WorkerStatus;
  workDir: string | null;
  waitingThread: string | null;
  location: WorkerLocation;
  lastHeartbeat: string | null;
  createdAt: string;
  needsRestart: boolean;
  sessionStartedAt: string | null;
}

/** Worker with session metrics for display */
export interface WorkerDisplay extends Worker {
  isLeader: boolean;
  contextUtilization: number | null;
  inputTokens: number | null;
  outputTokens: number | null;
  turns: number | null;
  currentTask: string | null;
  /** Sheep avatar configuration */
  sheepConfig: SheepConfig;
}

// =============================================================================
// Message Types
// =============================================================================

/** Message from the database */
export interface Message {
  id: number;
  thread: string;
  sender: string;
  content: string;
  waiting: boolean;
  readBy: string[] | null;
  timestamp: string;
}

/** Thread summary for chat panel */
export interface ThreadSummary {
  name: string;
  messageCount: number;
  unreadCount: number;
  lastMessage: string | null;
  lastTimestamp: string | null;
}

/** Unread notification from backend */
export interface UnreadNotification {
  id: string;
  runName: string;
  thread: string;
  sender: string;
  content: string;
  timestamp: string;
}

/** Response for get_all_unread_notifications */
export interface UnreadNotificationsResponse {
  notifications: UnreadNotification[];
  totalRunsWithUnread: number;
}

// =============================================================================
// Eval Types
// =============================================================================

/** Eval status values */
export type EvalStatus = 'running' | 'passed' | 'failed';

/** Eval from the database */
export interface Eval {
  id: number;
  branch: string;
  evalName: string | null;
  status: EvalStatus;
  feedback: string | null;
  logFile: string | null;
  startedAt: string;
  finishedAt: string | null;
  /** Sheep avatar configuration (detective hat) */
  sheepConfig: SheepConfig;
}

// =============================================================================
// History Types
// =============================================================================

/** History entry for activity log */
export interface HistoryEntry {
  id: number;
  timestamp: string;
  action: string;
  detail: string | null;
}

// =============================================================================
// Config Types
// =============================================================================

/** Agent preset configuration */
export interface AgentPreset {
  name: string;
  command: string[];
  mcpConfig: Record<string, unknown> | null;
}

/** Application configuration */
export interface Config {
  runsDir: string;
  agent: string;
  agentPresets: Record<string, AgentPreset>;
  defaultWorkerScale: string;
  defaultTimeLimit: number | null;
}

// =============================================================================
// UI State Types
// =============================================================================

/** Selected item state for navigation */
export interface SelectionState {
  runName: string | null;
  workerId: number | null;
  taskId: string | null;
  threadName: string | null;
}

/** Panel visibility state */
export interface PanelState {
  chatOpen: boolean;
  helpOpen: boolean;
}

/** Theme preference */
export type Theme = 'dark' | 'light' | 'system';

// =============================================================================
// API Response Types
// =============================================================================

/** Error codes from the backend */
export type ErrorCode =
  | 'run_not_found'
  | 'task_not_found'
  | 'worker_not_found'
  | 'thread_not_found'
  | 'invalid_input'
  | 'state_error'
  | 'git_error'
  | 'io_error'
  | 'config_error'
  | 'invalid_state'
  | 'permission_denied'
  | 'network_error'
  | 'process_error'
  | 'internal_error';

/** Structured error from the backend */
export interface GuiError {
  code: ErrorCode;
  message: string;
  details?: string;
}

/** Check if an error is a user error (vs system error) */
export function isUserError(code: ErrorCode): boolean {
  return [
    'invalid_input',
    'run_not_found',
    'task_not_found',
    'worker_not_found',
    'thread_not_found',
    'invalid_state',
  ].includes(code);
}

/** Get a human-readable label for an error code */
export function getErrorLabel(code: ErrorCode): string {
  const labels: Record<ErrorCode, string> = {
    run_not_found: 'Not Found',
    task_not_found: 'Not Found',
    worker_not_found: 'Not Found',
    thread_not_found: 'Not Found',
    invalid_input: 'Invalid Input',
    state_error: 'Database Error',
    git_error: 'Git Error',
    io_error: 'File Error',
    config_error: 'Config Error',
    invalid_state: 'Invalid State',
    permission_denied: 'Permission Denied',
    network_error: 'Network Error',
    process_error: 'Process Error',
    internal_error: 'Internal Error',
  };
  return labels[code];
}

/** Generic API result wrapper */
export interface ApiResult<T> {
  success: boolean;
  data?: T;
  error?: string;
  guiError?: GuiError;
}

/** Paginated list response */
export interface PaginatedList<T> {
  items: T[];
  total: number;
  offset: number;
  limit: number;
}

// =============================================================================
// Event Types
// =============================================================================

/** Custom event payloads for Alpine.js communication */
export interface RunSelectedEvent {
  runName: string;
}

export interface WorkerSelectedEvent {
  workerId: number;
}

export interface TaskSelectedEvent {
  taskId: string;
}

export interface MessageSentEvent {
  thread: string;
  content: string;
}

// =============================================================================
// Utility Types
// =============================================================================

/** ISO 8601 timestamp string */
export type Timestamp = string;

/** Status color mapping for UI */
export const STATUS_COLORS: Record<RunStatus, string> = {
  draft: 'sky-500',
  idle: 'wool-500',
  working: 'amber-500',
  paused: 'golden',
  runaway: 'terra',
  timed_out: 'terra',
  eval: 'amber-400',
  eval_failed: 'terra',
  waiting: 'golden',
  done: 'sage',
  delivered: 'sage',
  merged: 'sage',
};

/** Task status icons */
export const TASK_ICONS: Record<TaskStatus, string> = {
  todo: '\u25cb', // ○
  doing: '\u25cf', // ●
  done: '\u2713', // ✓
};

/** Worker status icons */
export const WORKER_ICONS: Record<WorkerStatus, string> = {
  idle: '\u25cb', // ○
  working: '\u25cf', // ●
  waiting: '\u29d7', // ⧗
  awaiting: '\u25cc', // ◌
  paused: '\u23f8', // ⏸
  error: '\u2717', // ✗
};

// =============================================================================
// Worker Log Types
// =============================================================================

/** Response for worker log content */
export interface WorkerLogResponse {
  content: string;
  byteOffset: number;
  fileSize: number;
  exists: boolean;
}

/** Parsed log line with tool activity info */
export interface ParsedLogLine {
  text: string;
  isToolStart: boolean;
  isToolEnd: boolean;
  toolName: string | null;
}

// =============================================================================
// Worker Events Types (ACP-based streaming)
// =============================================================================

/** Worker event type */
export type WorkerEventType = 'text' | 'tool_start' | 'tool_update' | 'thought';

/** Tool call status */
export type ToolCallStatus = 'pending' | 'in_progress' | 'completed' | 'failed';

/** Worker event for real-time streaming */
export interface WorkerEvent {
  id: number;
  workerName: string;
  eventType: WorkerEventType;
  timestamp: string;
  /** Text content (for text/thought events) */
  content: string | null;
  /** Tool call ID (for tool events) */
  toolCallId: string | null;
  /** Tool title/name */
  toolTitle: string | null;
  /** Tool kind (read, edit, execute, search, etc.) */
  toolKind: string | null;
  /** Tool execution status */
  toolStatus: ToolCallStatus | null;
  /** Tool input (JSON string) */
  toolInput: string | null;
  /** Tool output (JSON string) */
  toolOutput: string | null;
}

/** Response for worker events query */
export interface WorkerEventsResponse {
  events: WorkerEvent[];
  lastId: number | null;
  /** Worker status for determining if still streaming */
  workerStatus: WorkerStatus | null;
}

/** Worker stream event - emitted via Tauri events */
export type WorkerStreamEvent =
  | {
      type: 'history';
      runName: string;
      workerName: string;
      events: WorkerEvent[];
      workerStatus: string | null;
    }
  | {
      type: 'event';
      runName: string;
      workerName: string;
      event: WorkerEvent;
    }
  | {
      type: 'status';
      runName: string;
      workerName: string;
      workerStatus: string | null;
    }
  | {
      type: 'ended';
      runName: string;
      workerName: string;
    };

// =============================================================================
// Direct Chat Session Types (ACP-based AI chat)
// =============================================================================

/** UI context injected before user messages */
export interface UIContext {
  selectedRun: string | null;
  selectedWorker: string | null;
  uiSection: string;
  extra?: Record<string, string>;
}

/** Permission option in a permission request */
export interface PermissionOption {
  optionId: string;
  label: string;
  kind: string;
}

/** Pending permission request */
export interface PendingPermission {
  requestId: string;
  sessionId: string;
  title: string;
  description: string | null;
  options: PermissionOption[];
}

/** Chat event types */
export type ChatEventType =
  | 'textDelta'
  | 'thinkingDelta'
  | 'toolCallStart'
  | 'toolCallUpdate'
  | 'permissionRequest'
  | 'messageComplete'
  | 'error'
  | 'sessionEnded';

/** Base chat event */
export interface ChatEventBase {
  type: ChatEventType;
  sessionId: string;
}

/** Text delta event */
export interface TextDeltaEvent extends ChatEventBase {
  type: 'textDelta';
  text: string;
}

/** Thinking delta event */
export interface ThinkingDeltaEvent extends ChatEventBase {
  type: 'thinkingDelta';
  text: string;
}

/** Tool call start event */
export interface ToolCallStartEvent extends ChatEventBase {
  type: 'toolCallStart';
  toolCallId: string;
  title: string;
  kind: string | null;
  input: string | null;
}

/** Tool call update event */
export interface ToolCallUpdateEvent extends ChatEventBase {
  type: 'toolCallUpdate';
  toolCallId: string;
  status: string;
  title: string | null;
  output: string | null;
}

/** Permission request event */
export interface PermissionRequestEvent extends ChatEventBase {
  type: 'permissionRequest';
  request: PendingPermission;
}

/** Message complete event */
export interface MessageCompleteEvent extends ChatEventBase {
  type: 'messageComplete';
}

/** Error event */
export interface ChatErrorEvent extends ChatEventBase {
  type: 'error';
  message: string;
}

/** Session ended event */
export interface SessionEndedEvent extends ChatEventBase {
  type: 'sessionEnded';
}

/** Union type for all chat events */
export type ChatEvent =
  | TextDeltaEvent
  | ThinkingDeltaEvent
  | ToolCallStartEvent
  | ToolCallUpdateEvent
  | PermissionRequestEvent
  | MessageCompleteEvent
  | ChatErrorEvent
  | SessionEndedEvent;

/** Chat message role */
export type ChatMessageRole = 'user' | 'assistant' | 'system';

/** Tool call in a message */
export interface ChatToolCall {
  id: string;
  title: string;
  kind: string | null;
  status: string;
  /** Tool input (JSON string, e.g. command for terminal tools) */
  input: string | null;
  output: string | null;
  /** Whether the tool details are expanded */
  expanded?: boolean;
}

/** Chat message for display */
export interface ChatMessage {
  id: string;
  role: ChatMessageRole;
  content: string;
  thinking?: string;
  toolCalls?: ChatToolCall[];
  timestamp: Date;
  streaming?: boolean;
}

// =============================================================================
// Global Window Extensions
// =============================================================================

import type Alpine from 'alpinejs';

/** Toast API */
export interface ToastAPI {
  success: (message: string, icon?: string) => void;
  error: (message: string, icon?: string) => void;
  info: (message: string, icon?: string) => void;
  warning: (message: string, icon?: string) => void;
}

/** Confirm dialog API */
export interface ConfirmDialogAPI {
  show: (options: {
    title: string;
    message: string;
    confirmText?: string;
    cancelText?: string;
    danger?: boolean;
  }) => Promise<boolean>;
  delete: (itemName: string, itemType?: string) => Promise<boolean>;
}

// Shortcut types for global functions
import type { ShortcutConfig, ShortcutBinding } from './shortcuts';

/** Extend the global Window interface */
declare global {
  interface Window {
    // Alpine.js
    Alpine: typeof Alpine;

    // Tauri APIs
    tauriInvoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
    tauriGetCurrentWindow: () => ReturnType<typeof import('@tauri-apps/api/window').getCurrentWindow>;

    // Icon utilities
    getIcon: (name: string) => string;
    getActionIcon: (action: string) => string;
    getTaskStatusIcon: (status: string) => string;
    getWorkerStatusIcon: (status: string) => string;

    // Sheep avatar utilities
    generateSheepSvg: (config: SheepConfig) => string;
    getWorkerSheepSvg: (workerName: string) => string;
    getHatName: (hatIndex: number) => string;
    generateAgentSheepSvg: () => string;

    // Keyboard shortcuts utilities
    getShortcuts: () => ShortcutConfig[];
    formatBinding: (binding: ShortcutBinding) => string;

    // Alpine components (functions that return component data)
    appState: () => Record<string, unknown>;
    runList: () => Record<string, unknown>;
    runDetail: () => Record<string, unknown>;
    draftEditor: () => Record<string, unknown>;
    workerPanel: () => Record<string, unknown>;
    taskPanel: () => Record<string, unknown>;
    activityLog: () => Record<string, unknown>;
    chatPanel: () => Record<string, unknown>;
    directChat: () => Record<string, unknown>;
    permissionModal: () => Record<string, unknown>;
    notifications: () => Record<string, unknown>;
    tasksTab: () => Record<string, unknown>;
    sheepClickerGame: () => Record<string, unknown>;
    settingsModal: () => Record<string, unknown>;
    aiMessageStream: () => Record<string, unknown>;
    workerOutputViewer: () => Record<string, unknown>;
    sortToggle: () => Record<string, unknown>;
    sortButton: () => Record<string, unknown>;
    debugPanel: () => Record<string, unknown>;

    // UI utilities
    toast: ToastAPI;
    confirmDialog: ConfirmDialogAPI;

    // Lucide icons
    lucide?: {
      createIcons: (options?: { nodes?: Element[] }) => void;
    };
  }
}
