/**
 * TypeScript type definitions for Hirsel
 *
 * These types mirror the Rust/SQLite models and are used throughout
 * the frontend for type safety with Tauri IPC calls.
 */

// =============================================================================
// Runner Types (Host + Container Model)
// =============================================================================

/** Host type - where compute runs */
export type HostType = 'local' | 'client' | 'ssh' | 'fly';

/** Container configuration for Docker */
export interface ContainerConfig {
  image: string;
}

/** SSH host configuration */
export interface SshHostConfig {
  type: 'ssh';
  address: string;
  port: number;
  sshKey: string | null;
  workBase: string;
  location: string | null;
}

/** Fly.io host configuration */
export interface FlyHostConfig {
  type: 'fly';
  apiToken: string | null;
  app: string;
  region: string | null;
  cpuKind: string;
  cpus: number;
  memoryMb: number;
  autoDestroy: boolean;
}

/** Host configuration - where workers run */
export type HostConfig = { type: 'local' } | { type: 'client' } | SshHostConfig | FlyHostConfig;

/** Runner configuration - Host + optional Container */
export interface RunnerConfig {
  host: HostConfig;
  container?: ContainerConfig;
}

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
  | 'working'
  | 'paused'
  | 'failed'
  | 'eval'
  | 'done'
  | 'delivered'
  | 'waiting'
  | 'merged'
  | 'idle'
  | 'runaway'
  | 'timed_out'
  | 'eval_failed';

/** Failure reason values (only meaningful when status is 'failed') */
export type FailureReason = 'iteration_limit' | 'time_limit' | 'eval_failed' | 'manual';

/** Summary of a run for the run list panel */
export interface RunSummary {
  name: string;
  status: RunStatus;
  tasksDone: number;
  tasksTotal: number;
  workersActive: number;
  workersTotal: number;
  workersDesired: number;
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
  humanInTheLoop: boolean;
  waitingReason: string | null;
  unreadCount: number;
  // Additional fields for status bar display
  tasksDone: number;
  tasksTotal: number;
  workersActive: number;
  workersTotal: number;
  workersDesired: number;
  elapsedMinutes: number;
  // Runner configuration
  runner: string | null;
  workerRunners: Record<string, string> | null;
  // Agent/metrics info
  agentType: string;
  metricsAvailable: boolean;
}

/** Starting point for a draft workspace */
export type StartingPoint =
  | { type: 'greenfield' }
  | { type: 'localFolder'; path: string }
  | { type: 'gitRepo'; url: string; branch?: string };

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
  workerRunners?: Record<string, string>;
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
  humanInTheLoop: boolean;
  waitingReason: string | null;
  unreadCount: number;
  lastTimeNotificationPct: number | null;
}

// =============================================================================
// Worker Types
// =============================================================================

/** Worker status values */
export type WorkerStatus = 'idle' | 'working' | 'waiting' | 'awaiting' | 'paused' | 'error';

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
  hitlWaiting: boolean;
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
  /** Whether worker is waiting for HITL input */
  hitlWaiting: boolean;
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
  projectId: number;
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

/** Config defaults for project settings inheritance */
export interface ConfigDefaults {
  /** Default worker scale (typically "1") */
  workerScale: string;
  /** Default time limit in minutes (null = no limit) */
  timeLimitMinutes: number | null;
  /** Default human-in-the-loop setting */
  humanInTheLoop: boolean;
  /** Available runner names from global config */
  runners: string[];
  /** Default runner name from global config */
  defaultRunner: string | null;
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
  working: 'amber-500',
  paused: 'golden',
  failed: 'terra',
  eval: 'amber-400',
  done: 'sage',
  delivered: 'sage',
  waiting: 'golden',
  merged: 'sage',
  idle: 'wool-500',
  runaway: 'terra',
  timed_out: 'terra',
  eval_failed: 'terra',
};

/** Worker status icons */
export const WORKER_ICONS: Record<WorkerStatus, string> = {
  idle: '\u25cb', // ○
  working: '\u25cf', // ●
  waiting: '\u25d4', // ◔
  awaiting: '\u25cc', // ◌
  paused: '\u23f8', // ⏸
  error: '\u2717', // ✗
};

// =============================================================================
// Worker Log Types
// =============================================================================

/** Response for log content (used by eval logs) */
export interface WorkerLogResponse {
  content: string;
  byteOffset: number;
  fileSize: number;
  exists: boolean;
}

// =============================================================================
// Worker Events Types (streaming)
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
// Direct Chat Session Types
// =============================================================================

/** UI context injected before user messages */
export interface UIContext {
  selectedRun: string | null;
  selectedWorker: string | null;
  uiSection: string;
  extra?: Record<string, string>;
}

/** Chat event types */
export type ChatEventType =
  | 'textDelta'
  | 'toolCallStart'
  | 'toolCallUpdate'
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
  | ToolCallStartEvent
  | ToolCallUpdateEvent
  | MessageCompleteEvent
  | ChatErrorEvent
  | SessionEndedEvent;

/** Chat message role */
export type ChatMessageRole = 'user' | 'assistant' | 'system';

/** Image attached to a chat message */
export interface ChatImage {
  src: string;
  mimeType?: string;
  name?: string;
  dataBase64?: string;
}

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
  images?: ChatImage[];
  thinking?: string;
  toolCalls?: ChatToolCall[];
  timestamp: Date;
  streaming?: boolean;
}

// =============================================================================
// Shepherd Session Types
// =============================================================================

/** Task focus for board context */
export interface TaskFocus {
  taskId: string;
  taskName: string;
}

/** Image payload accepted by Shepherd send contract */
export interface ShepherdImageInput {
  mimeType: string;
  dataBase64: string;
  name?: string;
}

/** Structured Shepherd message chunks */
export type ShepherdMessageChunk =
  | { type: 'text'; content: string }
  | { type: 'thinking'; content: string }
  | {
      type: 'tool';
      id: string;
      title: string;
      kind: string | null;
      status: string;
      input: string | null;
      output: string | null;
    }
  | ({ type: 'image' } & ShepherdImageInput);

/** Shepherd session scope - determines prompt and context */
export type ShepherdScope =
  | { type: 'general' }
  | { type: 'run'; runName: string; workspacePath: string; projectPath?: string }
  | { type: 'board'; projectId: number; workspacePath?: string; focus?: TaskFocus };

/** Request to start a Shepherd session */
export type StartShepherdSessionRequest =
  | { type: 'general' }
  | { type: 'run'; runName: string }
  | { type: 'board'; projectId: number }
  | { type: 'boardFocused'; projectId: number; taskId: string; taskName: string };

/** Response from starting a Shepherd session */
export interface StartShepherdSessionResponse {
  sessionId: string;
  scope: ShepherdScope;
}

// =============================================================================
// IDE Types
// =============================================================================

/** Result of opening a run's staging directory in IDE */
export interface OpenIdeResult {
  success: boolean;
  ideUsed: string;
  pathOpened: string;
  wasDownloaded: boolean;
}

// =============================================================================
// Version Info
// =============================================================================

/** Version and build information */
export interface VersionInfo {
  version: string;
  gitSha: string;
  buildDate: string;
  features: string[];
  fullVersion: string;
}

// =============================================================================
// Global Window Extensions
// =============================================================================

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

/** Extend the global Window interface */
declare global {
  interface Window {
    // UI utilities
    toast: ToastAPI;
    confirmDialog: ConfirmDialogAPI;

    // Lucide icons
    lucide?: {
      createIcons: (options?: { inTemplates?: boolean; nodes?: Element[] }) => void;
    };
  }
}

// =============================================================================
// SpecFlow Board Types (Tasks + Evals Model)
// =============================================================================

/** Task status values */
export type BoardTaskStatus = 'todo' | 'doing' | 'done' | 'blocked';

/** Eval status values */
export type BoardEvalStatus = 'blocked' | 'queued' | 'in_progress' | 'passed' | 'failed';

/**
 * A task in the board (flat, from DB)
 */
export interface BoardTask {
  id: string; // Slug ID (e.g., "build-api")
  parentId: string | null;
  position: number;
  name: string;
  status: BoardTaskStatus;
  content: string;
  x: number | null;
  y: number | null;
  createdAt: string;
  updatedAt: string;
}

/**
 * Task tree (nested, for rendering)
 *
 * Validation Rules:
 * - A task is "validated" if it has a passing check OR all children are validated
 * - Validation propagates up the tree
 */
export interface TaskTree {
  id: string;
  name: string;
  status: BoardTaskStatus;
  content: string;
  children: TaskTree[];
  x: number | null;
  y: number | null;
  validated?: boolean; // Computed field
}

/**
 * A check in the board
 *
 * Checks are flat (not nested) and have a computed validates[] from nodes' checked_by
 */
export interface BoardCheck {
  id: string; // Slug ID (e.g., "api-test")
  name: string;
  status: BoardEvalStatus;
  content: string;
  validates: string[]; // Computed: node IDs where checked_by includes this check
  x: number | null;
  y: number | null;
  createdAt: string;
  updatedAt: string;
}

/** Bookmark for saved viewport positions */
export interface Bookmark {
  id: string;
  name: string;
  x: number;
  y: number;
  zoom: number;
  createdAt: string;
}

/** Result of a board sync operation */
export interface BoardSyncResult {
  /** Number of changes applied */
  changes: number;
  /** Tasks that were added */
  tasksAdded: string[];
  /** Tasks that were updated */
  tasksUpdated: string[];
  /** Tasks that were deleted */
  tasksDeleted: string[];
  /** Checks that were added */
  checksAdded: string[];
  /** Checks that were updated */
  checksUpdated: string[];
  /** Checks that were deleted */
  checksDeleted: string[];
}

/** Status color mapping for tasks */
export const BOARD_TASK_COLORS: Record<BoardTaskStatus, string> = {
  todo: 'wool-500',
  doing: 'amber-500',
  done: 'sage',
  blocked: 'terra',
};

/** Status color mapping for checks */
export const BOARD_EVAL_COLORS: Record<BoardEvalStatus, string> = {
  blocked: 'wool-600',
  queued: 'sky-500',
  in_progress: 'amber-500',
  passed: 'sage',
  failed: 'terra',
};

// =============================================================================
// Dispatch & Delivery Types
// =============================================================================

/** Delivery status - tracks the publication state of a run's changes */
export type DeliveryStatus = 'pending' | 'pushed' | 'pr_open' | 'merged' | 'abandoned';

/** Merge state - tracks whether a run can be cleanly merged */
export type MergeState = 'unknown' | 'clean' | 'conflicts';

/** A record of a run dispatched from a task */
export interface TaskRun {
  id: number;
  projectId: number;
  taskId: string;
  runName: string;
  dispatchedAt: string;
}

/** Preview of what will be dispatched from a task */
export interface DispatchPreview {
  taskIds: string[];
  checkIds: string[];
  taskCount: number;
  checkCount: number;
}

/** Board snapshot taken at dispatch time */
export interface BoardSnapshot {
  tasks: TaskTree[];
  checks: BoardCheck[];
  dispatchedAt: string;
}

/** Configuration for dispatching a run */
export interface DispatchConfig {
  runName?: string;
  targetBranch?: string;
  workerScale?: string;
  timeLimitMinutes?: number;
}

/** Result of a dispatch preparation */
export interface DispatchInfo {
  runName: string;
  runPath: string;
  taskIds: string[];
  checkIds: string[];
  featureContent: string;
  checkContent?: string;
  targetBranch?: string;
  branchOffCommit?: string;
}

/** Current delivery state of a run */
export interface DeliveryState {
  status: DeliveryStatus;
  mergeState: MergeState;
  stalenessCommits: number;
  deliveryBranch?: string;
  prUrl?: string;
  prNumber?: number;
  conflictingFiles: string[];
}

/** Result of a push operation */
export interface PushResult {
  branch: string;
  remote: string;
  url?: string;
}

/** Information about a pull request */
export interface PrInfo {
  number: number;
  url: string;
  title: string;
  state: string;
  headBranch: string;
  baseBranch: string;
  mergeable?: boolean;
  merged: boolean;
}

/** Result of a merge operation */
export interface MergeInfo {
  merged: boolean;
  sha?: string;
  message: string;
}

/** Run with dispatch/delivery info for display */
export interface DispatchedRun {
  runName: string;
  status: RunStatus;
  deliveryStatus: DeliveryStatus;
  dispatchedAt: string;
  stalenessCommits: number;
  mergeState: MergeState;
  prUrl?: string;
}

/** Status color mapping for delivery status */
export const DELIVERY_STATUS_COLORS: Record<DeliveryStatus, string> = {
  pending: 'wool-500',
  pushed: 'sky-500',
  pr_open: 'amber-500',
  merged: 'sage',
  abandoned: 'wool-600',
};

/** Status color mapping for merge state */
export const MERGE_STATE_COLORS: Record<MergeState, string> = {
  unknown: 'wool-500',
  clean: 'sage',
  conflicts: 'terra',
};

/** Icons for delivery status */
export const DELIVERY_STATUS_ICONS: Record<DeliveryStatus, string> = {
  pending: '\u25cb', // ○
  pushed: '\u2191', // ↑
  pr_open: '\u21bb', // ↻
  merged: '\u2713', // ✓
  abandoned: '\u2717', // ✗
};

/** Icons for merge state */
export const MERGE_STATE_ICONS: Record<MergeState, string> = {
  unknown: '\u003f', // ?
  clean: '\u2713', // ✓
  conflicts: '\u26a0', // ⚠
};

// =============================================================================
// Board Tree Types (Unified Feature/Task/Check Model)
// =============================================================================

/** Node kind - feature, task, or check */
export type NodeKind = 'feature' | 'task' | 'check' | 'plan';

/** Status of a board node */
export type BoardNodeStatus =
  | 'draft'
  | 'pending'
  | 'working'
  | 'done'
  | 'awaiting_check'
  | 'validated'
  | 'needs_repair'
  | 'failed';

/** Source of a board node - where it originated */
export type BoardNodeSource = 'user' | 'plan' | 'worker' | 'system';

/** Status of a project's persistent run */
export type ProjectRunStatus = 'paused' | 'working' | 'failed';

/** A board node (flat, from DB) */
export interface BoardNode {
  id: string;
  projectId: number;
  parentId: string | null;
  position: number;
  name: string;
  kind: NodeKind;
  source: BoardNodeSource;
  content: string;
  status: BoardNodeStatus;
  validates: string[];
  validatedBy: string[];
  blockedBy: string[];
  x: number | null;
  y: number | null;
  createdAt: string;
  updatedAt: string;
  completedAt: string | null;
  lastCommitSha: string | null;
  resolves: string | null;
  claimedBy: string | null;
  claimedAt: string | null;
  completedBy: string | null;
  checkResult: 'pass' | 'fail' | null;
  checkFeedback: string | null;
  tokensUsed: number | null;
}

/** Board node tree (nested for rendering) */
export interface BoardNodeTree {
  id: string;
  parentId: string | null;
  name: string;
  kind: NodeKind;
  source: BoardNodeSource;
  content: string;
  status: BoardNodeStatus;
  validates: string[];
  validatedBy: string[];
  blockedBy: string[];
  children: BoardNodeTree[];
  x: number | null;
  y: number | null;
  completedAt: string | null;
  lastCommitSha: string | null;
  resolves: string | null;
  claimedBy: string | null;
  claimedAt: string | null;
  completedBy: string | null;
  checkResult: 'pass' | 'fail' | null;
  checkFeedback: string | null;
  tokensUsed: number | null;
}

/** A persistent run for a project */
export interface ProjectRun {
  id: number;
  projectId: number;
  runName: string;
  status: ProjectRunStatus;
  createdAt: string;
  lastDispatchAt: string | null;
}

/** Request to create a board node */
export interface CreateBoardNodeRequest {
  parentId?: string | null;
  name: string;
  kind?: NodeKind;
  content?: string;
  validatedBy?: string[];
  blockedBy?: string[];
  x?: number | null;
  y?: number | null;
}

/** Request to update a board node */
export interface UpdateBoardNodeRequest {
  name?: string;
  content?: string;
  validatedBy?: string[];
  blockedBy?: string[];
  x?: number | null;
  y?: number | null;
}

/** Response from starting Shepherd orchestration */
export interface ShepherdRunResponse {
  runName: string;
  nodeCount: number;
  featureCount: number;
  planTaskCount: number;
  versionNumber: number;
  versionId: number;
}

/** Response containing the board tree */
export interface BoardTreeResponse {
  tree: BoardNodeTree[];
  projectRun: ProjectRun | null;
  generation: number;
}

/** Status colors for board nodes */
export const BOARD_NODE_STATUS_COLORS: Record<BoardNodeStatus, string> = {
  draft: 'sky-500',
  pending: 'wool-500',
  working: 'amber-500',
  done: 'sage',
  awaiting_check: 'amber-400',
  validated: 'sage',
  needs_repair: 'terra',
  failed: 'terra',
};

/** Status icons for board nodes */
export const BOARD_NODE_STATUS_ICONS: Record<BoardNodeStatus, string> = {
  draft: '\u270e', // ✎
  pending: '\u25cb', // ○
  working: '\u25cf', // ●
  done: '\u2713', // ✓
  awaiting_check: '\u25d4', // ◔
  validated: '\u2713', // ✓
  needs_repair: '\u26a0', // ⚠
  failed: '\u2717', // ✗
};

// =============================================================================
// Board Delivery Types
// =============================================================================

/** A version of the board (created on each dispatch) */
export interface BoardVersion {
  id: number;
  projectId: number;
  versionNumber: number;
  createdAt: string;
  description: string | null;
}

/** Status of a board delivery */
export type BoardDeliveryStatus =
  | 'pending'
  | 'in_progress'
  | 'resolving_conflicts'
  | 'pushed'
  | 'pr_open'
  | 'merged'
  | 'failed'
  | 'abandoned';

/** A delivery tracks the publication of a board version */
export interface BoardDelivery {
  id: number;
  projectId: number;
  versionId: number;
  status: BoardDeliveryStatus;
  targetBranch: string;
  deliveryBranch: string | null;
  prUrl: string | null;
  prNumber: number | null;
  startedAt: string | null;
  completedAt: string | null;
  failureReason: string | null;
}

/** Status of a delivery attempt */
export type DeliveryAttemptStatus = 'success' | 'failed' | 'cancelled';

/** A delivery attempt (retry history) */
export interface DeliveryAttempt {
  id: number;
  deliveryId: number;
  attemptNumber: number;
  status: DeliveryAttemptStatus;
  startedAt: string;
  completedAt: string | null;
  errorMessage: string | null;
}

/** Result of validating a delivery target branch */
export interface DeliveryValidation {
  hasRemote: boolean;
  hasForge: boolean;
  targetExistsOnRemote: boolean;
  mergeState: MergeState;
  conflictingFiles: string[];
  availableActions: Array<'push' | 'pr' | 'merge'>;
  remoteBranches: string[];
  remoteUrl: string | null;
  isLocal: boolean;
  needsInit: boolean;
  error: string | null;
}

/** Status colors for board delivery */
export const BOARD_DELIVERY_STATUS_COLORS: Record<BoardDeliveryStatus, string> = {
  pending: 'wool-500',
  in_progress: 'amber-500',
  resolving_conflicts: 'amber-400',
  pushed: 'sky-500',
  pr_open: 'sky-400',
  merged: 'sage',
  failed: 'terra',
  abandoned: 'wool-600',
};

/** Status icons for board delivery */
export const BOARD_DELIVERY_STATUS_ICONS: Record<BoardDeliveryStatus, string> = {
  pending: '\u25cb', // ○
  in_progress: '\u25cf', // ●
  resolving_conflicts: '\u2699', // ⚙
  pushed: '\u2191', // ↑
  pr_open: '\u21bb', // ↻
  merged: '\u2713', // ✓
  failed: '\u2717', // ✗
  abandoned: '\u2205', // ∅
};

// =============================================================================
// Route Types (Parallel Exploration Branches)
// =============================================================================

/** Repository/workspace linked to a route */
export interface RouteRepo {
  id: number;
  projectId: number;
  routeId: number;
  name: string;
  startingPoint: StartingPoint;
  targetBranch: string | null;
  runner: string | null;
  isArchived: boolean;
  createdAt: string;
  updatedAt: string;
}

/** A route within a project (parallel exploration branch) */
export interface Route {
  id: number;
  projectId: number;
  name: string;
  parentRouteId: number | null;
  parentVersionId: number | null;
  createdAt: string;
  updatedAt: string;
  repos: RouteRepo[];
  defaultRepoId: number | null;
  workerScale: string | null;
  timeLimitMinutes: number | null;
  humanInTheLoop: boolean;
  docsPath: string;
  persistDocsChanges: boolean;
  targetBranch: string | null;
  runner: string | null;
}

/** Route with ancestry information for tree display */
export interface RouteTree {
  id: number;
  name: string;
  parentRouteId: number | null;
  parentVersionId: number | null;
  children: RouteTree[];
  createdAt: string;
}

// =============================================================================
// Project Messages Types (Sheepfold)
// =============================================================================

/** Project message (Meadow or worker DM) */
export interface ProjectMessage {
  id: number;
  projectId: number;
  thread: string; // 'chat' or worker_name
  sender: string; // 'user' or worker_name
  content: string;
  waiting: boolean;
  timestamp: string;
}

/** Project thread summary with unread count */
export interface ProjectThreadSummary {
  thread: string;
  messageCount: number;
  unreadCount: number;
  lastMessage: string | null;
  lastTimestamp: string | null;
}
