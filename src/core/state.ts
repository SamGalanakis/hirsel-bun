import { Database } from "bun:sqlite";
import { readFileSync, existsSync } from "fs";
import { join, dirname } from "path";
import { fileURLToPath } from "url";
import type {
  WorkerStatus,
  TaskStatus,
  EvalStatus,
  Worker,
  Task,
  Message,
  HistoryEntry,
  Eval,
  TimeInfo,
  RunState,
} from "../shared/types";
import { Status } from "../shared/types";
import { config, getDbPath } from "./config";

const __dirname = dirname(fileURLToPath(import.meta.url));

function getSchema(): string {
  return readFileSync(join(__dirname, "schema.sql"), "utf-8");
}

function now(): string {
  return new Date().toISOString();
}

/**
 * SQLite-based state management for hirsel runs.
 * Direct port of the Python SQLiteState class.
 */
export class State {
  private db: Database;
  readonly dbPath: string;

  constructor(dbPath: string) {
    this.dbPath = dbPath;
    this.db = new Database(dbPath, { create: true });
    this.db.exec("PRAGMA journal_mode = WAL");
    this.db.exec("PRAGMA busy_timeout = 30000");
    this.initDb();
  }

  private initDb(): void {
    this.db.exec(getSchema());
  }

  close(): void {
    this.db.close();
  }

  // --- State (singleton row) ---

  getRunState(): RunState {
    const row = this.db.query("SELECT * FROM state WHERE id = 1").get() as Record<string, unknown> | null;
    if (!row) {
      // Return default state
      return {
        id: 1,
        status: Status.IDLE,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        request: null,
        projectPath: null,
        unreadCount: 0,
        humanInTheLoop: true,
        summary: null,
        waitingReason: null,
        workerScale: null,
        timeLimitMinutes: null,
        startedAt: null,
        lastTimeNotificationPct: null,
        iterationCount: 0,
        maxIterations: null,
        learningsProcessedAt: null,
      };
    }
    return {
      id: row.id as number,
      status: (row.status as Status) ?? Status.IDLE,
      createdAt: row.created_at as string,
      updatedAt: row.updated_at as string,
      request: row.request as string | null,
      projectPath: row.project_path as string | null,
      unreadCount: (row.unread_count as number) ?? 0,
      humanInTheLoop: (row.human_in_the_loop as number) === 1,
      summary: row.summary as string | null,
      waitingReason: row.waiting_reason as string | null,
      workerScale: row.worker_scale as string | null,
      timeLimitMinutes: row.time_limit_minutes as number | null,
      startedAt: row.started_at as string | null,
      lastTimeNotificationPct: row.last_time_notification_pct as number | null,
      iterationCount: (row.iteration_count as number) ?? 0,
      maxIterations: row.max_iterations as number | null,
      learningsProcessedAt: row.learnings_processed_at as string | null,
    };
  }

  get status(): Status {
    const row = this.db.query("SELECT status FROM state WHERE id = 1").get() as
      | { status: string }
      | null;
    return (row?.status ?? "idle") as Status;
  }

  set status(value: Status) {
    const oldStatus = this.status;
    if (oldStatus === value) return;

    const timestamp = now();
    this.db.exec(`
      INSERT INTO state (id, status, created_at, updated_at)
      VALUES (1, '${value}', '${timestamp}', '${timestamp}')
      ON CONFLICT(id) DO UPDATE SET status = '${value}', updated_at = '${timestamp}'
    `);
    this.logHistory("status_change", `run ${value}`);
  }

  getRequest(): string | null {
    const row = this.db
      .query("SELECT request FROM state WHERE id = 1")
      .get() as { request: string | null } | null;
    return row?.request ?? null;
  }

  setRequest(request: string | null): void {
    this.db
      .query("UPDATE state SET request = ?, updated_at = ? WHERE id = 1")
      .run(request, now());
  }

  getProjectPath(): string | null {
    const row = this.db
      .query("SELECT project_path FROM state WHERE id = 1")
      .get() as { project_path: string | null } | null;
    return row?.project_path ?? null;
  }

  setProjectPath(path: string): void {
    this.db
      .query("UPDATE state SET project_path = ?, updated_at = ? WHERE id = 1")
      .run(path, now());
  }

  getSummary(): string | null {
    const row = this.db.query("SELECT summary FROM state WHERE id = 1").get() as
      | { summary: string | null }
      | null;
    return row?.summary ?? null;
  }

  setSummary(summary: string): void {
    this.db
      .query("UPDATE state SET summary = ?, updated_at = ? WHERE id = 1")
      .run(summary, now());
  }

  isHumanInTheLoop(): boolean {
    const row = this.db
      .query("SELECT human_in_the_loop FROM state WHERE id = 1")
      .get() as { human_in_the_loop: number | null } | null;
    return (row?.human_in_the_loop ?? 1) === 1;
  }

  setHumanInTheLoop(value: boolean): void {
    this.db
      .query(
        "UPDATE state SET human_in_the_loop = ?, updated_at = ? WHERE id = 1"
      )
      .run(value ? 1 : 0, now());
  }

  // --- Time tracking ---

  setTimeLimit(minutes: number): void {
    const timestamp = now();
    this.db
      .query(
        "UPDATE state SET time_limit_minutes = ?, started_at = ?, updated_at = ? WHERE id = 1"
      )
      .run(minutes, timestamp, timestamp);
  }

  getTimeInfo(): TimeInfo | null {
    const row = this.db
      .query(
        "SELECT time_limit_minutes, started_at FROM state WHERE id = 1"
      )
      .get() as {
      time_limit_minutes: number | null;
      started_at: string | null;
    } | null;

    if (!row?.time_limit_minutes || !row?.started_at) {
      return null;
    }

    const startedAt = new Date(row.started_at);
    const nowTime = new Date();
    const elapsedMs = nowTime.getTime() - startedAt.getTime();
    const elapsedMinutes = elapsedMs / 60000;
    const remainingMinutes = row.time_limit_minutes - elapsedMinutes;
    const elapsedPct = (elapsedMinutes / row.time_limit_minutes) * 100;
    const remainingPct = 100 - elapsedPct;

    return {
      elapsedMinutes,
      remainingMinutes: Math.max(0, remainingMinutes),
      elapsedPct: Math.min(100, elapsedPct),
      remainingPct: Math.max(0, remainingPct),
      isExpired: remainingMinutes <= 0,
    };
  }

  /** Check if time limit has expired */
  isTimeExpired(): boolean {
    const timeInfo = this.getTimeInfo();
    return timeInfo?.isExpired ?? false;
  }

  /** Get the last time notification percentage that was sent */
  getLastTimeNotificationPct(): number | null {
    const row = this.db
      .query("SELECT last_time_notification_pct FROM state WHERE id = 1")
      .get() as { last_time_notification_pct: number | null } | null;
    return row?.last_time_notification_pct ?? null;
  }

  /** Set the last time notification percentage */
  setLastTimeNotificationPct(pct: number): void {
    this.db
      .query("UPDATE state SET last_time_notification_pct = ?, updated_at = ? WHERE id = 1")
      .run(pct, now());
  }

  /** Get the worker scale setting (e.g., "1-3" or "2+") */
  getWorkerScale(): string | null {
    const row = this.db
      .query("SELECT worker_scale FROM state WHERE id = 1")
      .get() as { worker_scale: string | null } | null;
    return row?.worker_scale ?? null;
  }

  /** Set the waiting reason for the run */
  setWaitingReason(reason: string | null): void {
    this.db
      .query("UPDATE state SET waiting_reason = ?, updated_at = ? WHERE id = 1")
      .run(reason, now());
  }

  /** Increment the iteration count and return the new value */
  incrementIteration(): number {
    this.db.exec("UPDATE state SET iteration_count = iteration_count + 1, updated_at = datetime('now') WHERE id = 1");
    const row = this.db
      .query("SELECT iteration_count FROM state WHERE id = 1")
      .get() as { iteration_count: number } | null;
    return row?.iteration_count ?? 0;
  }

  /** Get the maximum iterations limit */
  getMaxIterations(): number | null {
    const row = this.db
      .query("SELECT max_iterations FROM state WHERE id = 1")
      .get() as { max_iterations: number | null } | null;
    return row?.max_iterations ?? null;
  }

  /** Re-open the database connection (useful in long-running processes) */
  reconnect(): void {
    // Close and re-open to ensure fresh data
    this.db.close();
    this.db = new Database(this.dbPath);
  }

  // --- Workers ---

  addWorker(name: string, workDir: string | null = null): void {
    const timestamp = now();
    this.db
      .query(
        "INSERT INTO workers (name, work_dir, created_at) VALUES (?, ?, ?)"
      )
      .run(name, workDir, timestamp);
    this.logHistory("worker_add", name);
  }

  getWorker(name: string): Worker | null {
    const row = this.db
      .query("SELECT * FROM workers WHERE name = ?")
      .get(name) as Record<string, unknown> | null;
    return row ? this.rowToWorker(row) : null;
  }

  getWorkers(): Worker[] {
    const rows = this.db.query("SELECT * FROM workers ORDER BY id").all() as Record<string, unknown>[];
    return rows.map((row) => this.rowToWorker(row));
  }

  setWorkerStatus(name: string, status: WorkerStatus): void {
    this.db
      .query("UPDATE workers SET status = ? WHERE name = ?")
      .run(status, name);
    this.logHistory("worker_status", `${name} ${status}`);
  }

  setWorkerPid(name: string, pid: number | null): void {
    this.db.query("UPDATE workers SET pid = ? WHERE name = ?").run(pid, name);
  }

  setWorkerSessionId(name: string, sessionId: string | null): void {
    const timestamp = sessionId ? now() : null;
    this.db
      .query(
        "UPDATE workers SET session_id = ?, session_started_at = ? WHERE name = ?"
      )
      .run(sessionId, timestamp, name);
  }

  setWorkerNeedsRestart(name: string, needsRestart: boolean): void {
    this.db
      .query("UPDATE workers SET needs_restart = ? WHERE name = ?")
      .run(needsRestart ? 1 : 0, name);
  }

  setWorkerWaitingThread(name: string, thread: string | null): void {
    this.db
      .query("UPDATE workers SET waiting_thread = ? WHERE name = ?")
      .run(thread, name);
  }

  removeWorker(name: string): void {
    this.db.query("DELETE FROM workers WHERE name = ?").run(name);
    this.logHistory("worker_remove", name);
  }

  /** Update multiple worker properties at once */
  updateWorker(
    name: string,
    updates: {
      status?: WorkerStatus;
      pid?: number | null;
      sessionId?: string | null;
      needsRestart?: boolean;
      waitingThread?: string | null;
    }
  ): void {
    if (updates.status !== undefined) {
      this.setWorkerStatus(name, updates.status);
    }
    if (updates.pid !== undefined) {
      this.setWorkerPid(name, updates.pid);
    }
    if (updates.sessionId !== undefined) {
      this.setWorkerSessionId(name, updates.sessionId);
    }
    if (updates.needsRestart !== undefined) {
      this.setWorkerNeedsRestart(name, updates.needsRestart);
    }
    if (updates.waitingThread !== undefined) {
      this.setWorkerWaitingThread(name, updates.waitingThread);
    }
  }

  private rowToWorker(row: Record<string, unknown>): Worker {
    return {
      id: row.id as number,
      name: row.name as string,
      pid: row.pid as number | null,
      sessionId: row.session_id as string | null,
      sessionStartedAt: row.session_started_at as string | null,
      status: row.status as WorkerStatus,
      workDir: row.work_dir as string | null,
      waitingThread: row.waiting_thread as string | null,
      needsRestart: (row.needs_restart as number) === 1,
      location: (row.location as "local" | "remote") ?? "local",
      lastHeartbeat: row.last_heartbeat as string | null,
      createdAt: row.created_at as string,
    };
  }

  // --- Tasks ---

  addTask(
    id: string,
    name: string,
    parentId: string | null = null,
    blockedBy: string[] | null = null
  ): void {
    const timestamp = now();
    const blockedByStr = blockedBy?.join(",") ?? null;
    this.db
      .query(
        "INSERT INTO tasks (id, name, created_at, parent_id, blocked_by) VALUES (?, ?, ?, ?, ?)"
      )
      .run(id, name, timestamp, parentId, blockedByStr);
    this.logHistory("task_add", id);
  }

  getTask(id: string): Task | null {
    const row = this.db
      .query("SELECT * FROM tasks WHERE id = ?")
      .get(id) as Record<string, unknown> | null;
    return row ? this.rowToTask(row) : null;
  }

  getTasks(): Task[] {
    const rows = this.db
      .query("SELECT * FROM tasks ORDER BY created_at")
      .all() as Record<string, unknown>[];
    return rows.map((row) => this.rowToTask(row));
  }

  claimTask(taskId: string, workerName: string): boolean {
    const timestamp = now();
    try {
      this.db.exec("BEGIN IMMEDIATE");

      const task = this.getTask(taskId);
      if (!task || task.status !== "todo" || task.claimedBy) {
        this.db.exec("ROLLBACK");
        return false;
      }

      if (task.blockedBy) {
        const blockers = task.blockedBy.split(",");
        for (const blockerId of blockers) {
          const blocker = this.getTask(blockerId.trim());
          if (blocker && blocker.status !== "done") {
            this.db.exec("ROLLBACK");
            return false;
          }
        }
      }

      const children = this.db
        .query("SELECT COUNT(*) as count FROM tasks WHERE parent_id = ?")
        .get(taskId) as { count: number };
      if (children.count > 0) {
        this.db.exec("ROLLBACK");
        return false;
      }

      const existing = this.db
        .query(
          "SELECT id FROM tasks WHERE claimed_by = ? AND status = 'doing'"
        )
        .get(workerName);
      if (existing) {
        this.db.exec("ROLLBACK");
        return false;
      }

      this.db
        .query(
          "UPDATE tasks SET status = 'doing', claimed_by = ?, claimed_at = ? WHERE id = ?"
        )
        .run(workerName, timestamp, taskId);

      this.db.exec("COMMIT");
      this.logHistory("task_claim", `${workerName} ${taskId}`);
      return true;
    } catch {
      this.db.exec("ROLLBACK");
      return false;
    }
  }

  completeTask(taskId: string): void {
    const timestamp = now();
    this.db
      .query(
        "UPDATE tasks SET status = 'done', completed_at = ? WHERE id = ?"
      )
      .run(timestamp, taskId);
    this.logHistory("task_done", taskId);

    const task = this.getTask(taskId);
    if (task?.parentId) {
      this.checkParentCompletion(task.parentId);
    }
  }

  private checkParentCompletion(parentId: string): void {
    const children = this.db
      .query("SELECT status FROM tasks WHERE parent_id = ?")
      .all(parentId) as { status: string }[];

    const allDone = children.every((c) => c.status === "done");
    if (allDone) {
      this.completeTask(parentId);
    }
  }

  unclaimTask(taskId: string): void {
    this.db
      .query(
        "UPDATE tasks SET status = 'todo', claimed_by = NULL, claimed_at = NULL WHERE id = ?"
      )
      .run(taskId);
    this.logHistory("task_unclaim", taskId);
  }

  deleteTask(taskId: string): void {
    this.db.query("DELETE FROM tasks WHERE parent_id = ?").run(taskId);
    this.db.query("DELETE FROM tasks WHERE id = ?").run(taskId);
    this.logHistory("task_delete", taskId);
  }

  /** Reopen a completed task */
  reopenTask(taskId: string): void {
    this.db
      .query(
        "UPDATE tasks SET status = 'todo', completed_at = NULL, claimed_by = NULL, claimed_at = NULL WHERE id = ?"
      )
      .run(taskId);
    this.logHistory("task_reopen", taskId);
  }

  getClaimableTasks(): Task[] {
    const tasks = this.getTasks();
    return tasks.filter((task) => {
      if (task.status !== "todo" || task.claimedBy) return false;

      if (task.blockedBy) {
        const blockers = task.blockedBy.split(",");
        for (const blockerId of blockers) {
          const blocker = this.getTask(blockerId.trim());
          if (blocker && blocker.status !== "done") return false;
        }
      }

      const children = this.db
        .query("SELECT COUNT(*) as count FROM tasks WHERE parent_id = ?")
        .get(task.id) as { count: number };
      if (children.count > 0) return false;

      return true;
    });
  }

  private rowToTask(row: Record<string, unknown>): Task {
    return {
      id: row.id as string,
      name: row.name as string,
      status: row.status as TaskStatus,
      createdAt: row.created_at as string,
      completedAt: row.completed_at as string | null,
      claimedBy: row.claimed_by as string | null,
      claimedAt: row.claimed_at as string | null,
      tokensUsed: row.tokens_used as number | null,
      parentId: row.parent_id as string | null,
      blockedBy: row.blocked_by as string | null,
      pendingDoneAt: row.pending_done_at as string | null,
    };
  }

  // --- Messages ---

  addMessage(
    thread: string,
    sender: string,
    content: string,
    waiting = false
  ): number {
    const timestamp = now();
    const result = this.db
      .query(
        "INSERT INTO messages (thread, sender, content, timestamp, waiting) VALUES (?, ?, ?, ?, ?) RETURNING id"
      )
      .get(thread, sender, content, timestamp, waiting ? 1 : 0) as { id: number };
    return result.id;
  }

  getMessages(thread: string, afterId = 0): Message[] {
    const rows = this.db
      .query(
        "SELECT * FROM messages WHERE thread = ? AND id > ? ORDER BY id"
      )
      .all(thread, afterId) as Record<string, unknown>[];
    return rows.map((row) => this.rowToMessage(row));
  }

  getUnreadMessages(workerName: string, thread: string): Message[] {
    const lastRead = this.db
      .query(
        "SELECT last_read_id FROM message_reads WHERE worker_name = ? AND thread = ?"
      )
      .get(workerName, thread) as { last_read_id: number } | null;

    const afterId = lastRead?.last_read_id ?? 0;
    return this.getMessages(thread, afterId);
  }

  markMessagesRead(workerName: string, thread: string, lastReadId: number): void {
    this.db
      .query(
        "INSERT INTO message_reads (worker_name, thread, last_read_id) VALUES (?, ?, ?) ON CONFLICT(worker_name, thread) DO UPDATE SET last_read_id = ?"
      )
      .run(workerName, thread, lastReadId, lastReadId);
  }

  getThreadNames(): string[] {
    const rows = this.db
      .query("SELECT DISTINCT thread FROM messages ORDER BY thread")
      .all() as { thread: string }[];
    return rows.map((r) => r.thread);
  }

  private rowToMessage(row: Record<string, unknown>): Message {
    return {
      id: row.id as number,
      thread: row.thread as string,
      sender: row.sender as string,
      content: row.content as string,
      timestamp: row.timestamp as string,
      waiting: (row.waiting as number) === 1,
    };
  }

  // --- History ---

  logHistory(action: string, detail: string | null = null): void {
    const timestamp = now();
    this.db
      .query("INSERT INTO history (timestamp, action, detail) VALUES (?, ?, ?)")
      .run(timestamp, action, detail);
  }

  getHistory(limit = 50): HistoryEntry[] {
    const rows = this.db
      .query("SELECT * FROM history ORDER BY id DESC LIMIT ?")
      .all(limit) as Record<string, unknown>[];
    return rows.reverse().map((row) => ({
      id: row.id as number,
      timestamp: row.timestamp as string,
      action: row.action as string,
      detail: row.detail as string | null,
    }));
  }

  // --- Evals ---

  addEval(branch: string, evalName: string | null = null): number {
    const timestamp = now();
    const result = this.db
      .query(
        "INSERT INTO evals (branch, eval_name, started_at) VALUES (?, ?, ?) RETURNING id"
      )
      .get(branch, evalName, timestamp) as { id: number };
    this.logHistory("eval_start", evalName ?? branch);
    return result.id;
  }

  getEval(id: number): Eval | null {
    const row = this.db.query("SELECT * FROM evals WHERE id = ?").get(id) as Record<string, unknown> | null;
    return row ? this.rowToEval(row) : null;
  }

  getRunningEvals(): Eval[] {
    const rows = this.db
      .query("SELECT * FROM evals WHERE status = 'running' ORDER BY id")
      .all() as Record<string, unknown>[];
    return rows.map((row) => this.rowToEval(row));
  }

  getEvals(): Eval[] {
    const rows = this.db
      .query("SELECT * FROM evals ORDER BY id DESC")
      .all() as Record<string, unknown>[];
    return rows.map((row) => this.rowToEval(row));
  }

  setEvalStatus(id: number, status: EvalStatus, feedback: string | null = null): void {
    const timestamp = now();
    this.db
      .query(
        "UPDATE evals SET status = ?, feedback = ?, finished_at = ? WHERE id = ?"
      )
      .run(status, feedback, timestamp, id);
    this.logHistory("eval_finish", String(status));
  }

  cancelRunningEvals(): void {
    this.db.exec("UPDATE evals SET status = 'failed' WHERE status = 'running'");
  }

  private rowToEval(row: Record<string, unknown>): Eval {
    return {
      id: row.id as number,
      branch: row.branch as string,
      evalName: row.eval_name as string | null,
      status: row.status as EvalStatus,
      feedback: row.feedback as string | null,
      logFile: row.log_file as string | null,
      startedAt: row.started_at as string,
      finishedAt: row.finished_at as string | null,
    };
  }
}

// =============================================================================
// Singleton state instances
// =============================================================================

const stateInstances = new Map<string, State>();

/**
 * Get a State instance for a run.
 * Creates the instance if it doesn't exist.
 */
export function getState(runName: string): State {
  let state = stateInstances.get(runName);
  if (!state) {
    const dbPath = getDbPath(runName);
    if (!existsSync(dbPath)) {
      throw new Error(`Run '${runName}' not found (no database at ${dbPath})`);
    }
    state = new State(dbPath);
    stateInstances.set(runName, state);
  }
  return state;
}

/**
 * Close all state instances.
 */
export function closeAllStates(): void {
  for (const state of stateInstances.values()) {
    state.close();
  }
  stateInstances.clear();
}

// Re-export RunState for convenience
export type { RunState };
