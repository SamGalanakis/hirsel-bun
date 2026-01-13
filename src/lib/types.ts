/**
 * TypeScript type definitions for Hirsel
 *
 * These types mirror the Rust/SQLite models and are used throughout
 * the frontend for type safety with Tauri IPC calls.
 */

// =============================================================================
// Run Types
// =============================================================================

/** Run status values matching Rust Status enum */
export type RunStatus =
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
  parentId: string | null;
  blockedBy: string[] | null;
  tokensUsed: number | null;
  createdAt: string;
  pendingDoneAt: string | null;
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
}
