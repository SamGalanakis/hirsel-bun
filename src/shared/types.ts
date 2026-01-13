// Status enums matching the Python reference implementation

export enum Status {
  IDLE = "idle",
  WORKING = "working",
  PAUSED = "paused",
  RUNAWAY = "runaway",
  TIMED_OUT = "timed_out",
  EVAL = "eval",
  EVAL_FAILED = "eval_failed",
  WAITING = "waiting",
  DONE = "done",
  DELIVERED = "delivered",
  MERGED = "merged",
}

export enum EvalStatus {
  RUNNING = "running",
  PASSED = "passed",
  FAILED = "failed",
}

export enum WorkerStatus {
  IDLE = "idle",
  WORKING = "working",
  WAITING = "waiting",
  AWAITING = "awaiting",
  PAUSED = "paused",
  DONE = "done",
  ERROR = "error",
}

export enum TaskStatus {
  TODO = "todo",
  DOING = "doing",
  DONE = "done",
}

// Database row types

export interface RunState {
  id: number;
  status: Status;
  createdAt: string;
  updatedAt: string;
  request: string | null;
  projectPath: string | null;
  unreadCount: number;
  humanInTheLoop: boolean;
  summary: string | null;
  waitingReason: string | null;
  workerScale: string | null;
  timeLimitMinutes: number | null;
  startedAt: string | null;
  lastTimeNotificationPct: number | null;
  iterationCount: number;
  maxIterations: number | null;
  learningsProcessedAt: string | null;
}

export interface Worker {
  id: number;
  name: string;
  pid: number | null;
  sessionId: string | null;
  sessionStartedAt: string | null;
  status: WorkerStatus;
  workDir: string | null;
  waitingThread: string | null;
  needsRestart: boolean;
  location: "local" | "remote";
  lastHeartbeat: string | null;
  createdAt: string;
}

export interface Task {
  id: string;
  name: string;
  status: TaskStatus;
  createdAt: string;
  completedAt: string | null;
  claimedBy: string | null;
  claimedAt: string | null;
  tokensUsed: number | null;
  parentId: string | null;
  blockedBy: string | null;
  pendingDoneAt: string | null;
}

export interface Message {
  id: number;
  thread: string;
  sender: string;
  content: string;
  timestamp: string;
  waiting: boolean;
}

export interface MessageRead {
  workerName: string;
  thread: string;
  lastReadId: number;
}

export interface HistoryEntry {
  id: number;
  timestamp: string;
  action: string;
  detail: string | null;
}

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

export interface Amendment {
  id: number;
  message: string;
  timestamp: string;
  author: string;
  specHash: string;
}

// Run details for UI

export interface RunSummary {
  name: string;
  status: Status;
  projectPath: string | null;
  tasksDone: number;
  tasksTotal: number;
  workersActive: number;
  workersTotal: number;
  elapsedMinutes: number;
  timeLimitMinutes: number | null;
  hasUnread: boolean;
}

export interface RunDetails extends RunSummary {
  workers: Worker[];
  tasks: Task[];
  history: HistoryEntry[];
  evals: Eval[];
  request: string | null;
  summary: string | null;
}

// Time info

export interface TimeInfo {
  elapsedMinutes: number;
  remainingMinutes: number | null;
  elapsedPct: number | null;
  remainingPct: number | null;
  isExpired: boolean;
  limitMinutes?: number;
}

// Worker scale configuration

export interface WorkerScale {
  min: number;
  max: number | null;
  isFixed: boolean;
}

// Git info

export interface BranchInfo {
  name: string;
  isCurrent: boolean;
  isMerged: boolean;
  commitCount: number;
}

export interface GitInfo {
  currentBranch: string;
  unmergedBranches: string[];
  branchHistory: BranchInfo[];
  branchGraph: string;
}

// Session metrics (for Claude context tracking)

export interface SessionMetrics {
  turns: number;
  inputTokens: number;
  outputTokens: number;
  model: string | null;
  contextUtilization: number;
}
