/**
 * Tauri API wrappers for Hirsel
 *
 * This module provides typed wrappers around Tauri's invoke() function
 * for calling Rust backend commands. All functions handle errors gracefully
 * and provide type-safe interfaces.
 */

import { invoke } from '@tauri-apps/api/core';
import type {
  RunSummary,
  RunDetail,
  Task,
  Worker,
  Message,
  ThreadSummary,
  Eval,
  HistoryEntry,
  Config,
  ApiResult,
  GuiError,
} from './types';
import { toast } from './toast';

// =============================================================================
// Run Management API
// =============================================================================

/**
 * Get list of all runs with summary information
 */
export async function getRuns(): Promise<RunSummary[]> {
  return invoke<RunSummary[]>('get_runs');
}

/**
 * Get detailed information about a specific run
 */
export async function getRunDetail(name: string): Promise<RunDetail> {
  return invoke<RunDetail>('get_run_detail', { name });
}

/**
 * Start a new run
 */
export async function startRun(
  name: string,
  specPath: string,
  options?: {
    workers?: string;
    timeLimit?: string;
    humanInTheLoop?: boolean;
  }
): Promise<void> {
  return invoke('start_run', {
    name,
    specPath,
    workers: options?.workers,
    timeLimit: options?.timeLimit,
    humanInTheLoop: options?.humanInTheLoop,
  });
}

/**
 * Pause a run (stops all workers)
 */
export async function pauseRun(name: string): Promise<void> {
  return invoke('pause_run', { name });
}

/**
 * Resume a paused or timed-out run
 */
export async function resumeRun(
  name: string,
  timeLimit?: string
): Promise<void> {
  return invoke('resume_run', { name, timeLimit });
}

/**
 * Delete a run and all its data
 */
export async function deleteRun(name: string): Promise<void> {
  return invoke('delete_run', { name });
}

/**
 * Deliver run changes to project repo as a branch
 */
export async function deliverRun(
  name: string,
  branchName?: string
): Promise<string> {
  return invoke<string>('deliver_run', { name, branchName });
}

/**
 * Delete all delivered runs
 */
export async function pruneRuns(): Promise<number> {
  return invoke<number>('prune_runs');
}

/**
 * Get run summary/report
 */
export async function getRunSummary(name: string): Promise<string> {
  return invoke<string>('get_run_summary', { name });
}

// =============================================================================
// Task Management API
// =============================================================================

/**
 * Get all tasks for a run
 */
export async function getTasks(runName: string): Promise<Task[]> {
  return invoke<Task[]>('get_tasks', { runName });
}

/**
 * Add a new task to a run
 */
export async function addTask(
  runName: string,
  taskId: string,
  description: string,
  options?: {
    parentId?: string;
    blockedBy?: string[];
  }
): Promise<void> {
  return invoke('add_task', {
    runName,
    taskId,
    description,
    parentId: options?.parentId,
    blockedBy: options?.blockedBy,
  });
}

/**
 * Delete a task from a run
 */
export async function deleteTask(runName: string, taskId: string): Promise<void> {
  return invoke('delete_task', { runName, taskId });
}

/**
 * Mark a task as done
 */
export async function completeTask(runName: string, taskId: string): Promise<void> {
  return invoke('complete_task', { runName, taskId });
}

/**
 * Reopen a completed task
 */
export async function reopenTask(runName: string, taskId: string): Promise<void> {
  return invoke('reopen_task', { runName, taskId });
}

/**
 * Unclaim a task (release it back to todo)
 */
export async function unclaimTask(runName: string, taskId: string): Promise<void> {
  return invoke('unclaim_task', { runName, taskId });
}

// =============================================================================
// Worker Management API
// =============================================================================

/**
 * Get all workers for a run
 */
export async function getWorkers(runName: string): Promise<Worker[]> {
  return invoke<Worker[]>('get_workers', { runName });
}

/**
 * Attach to a worker's tmux session
 */
export async function attachWorker(runName: string, workerName: string): Promise<void> {
  return invoke('attach_worker', { runName, workerName });
}

/**
 * Add a new worker to a run
 */
export async function addWorker(runName: string): Promise<string> {
  return invoke<string>('add_worker', { runName });
}

/**
 * Get worker session metrics (tokens, context utilization)
 */
export async function getWorkerMetrics(
  runName: string,
  workerName: string
): Promise<{
  inputTokens: number;
  outputTokens: number;
  turns: number;
  contextUtilization: number;
}> {
  return invoke('get_worker_metrics', { runName, workerName });
}

// =============================================================================
// Message/Chat API
// =============================================================================

/**
 * Get all threads for a run
 */
export async function getThreads(runName: string): Promise<ThreadSummary[]> {
  return invoke<ThreadSummary[]>('get_threads', { runName });
}

/**
 * Get messages for a specific thread
 */
export async function getMessages(
  runName: string,
  threadName: string
): Promise<Message[]> {
  return invoke<Message[]>('get_messages', { runName, threadName });
}

/**
 * Send a message to a thread
 */
export async function sendMessage(
  runName: string,
  threadName: string,
  content: string
): Promise<void> {
  return invoke('send_message', {
    run: runName,
    thread: threadName,
    message: content,
  });
}

/**
 * Mark messages as read
 */
export async function markMessagesRead(
  runName: string,
  threadName: string
): Promise<void> {
  return invoke('mark_messages_read', { runName, threadName });
}

// =============================================================================
// Eval API
// =============================================================================

/**
 * Get all evals for a run
 */
export async function getEvals(runName: string): Promise<Eval[]> {
  return invoke<Eval[]>('get_evals', { runName });
}

/**
 * Attach to an eval's log output
 */
export async function attachEval(runName: string, evalName: string): Promise<void> {
  return invoke('attach_eval', { runName, evalName });
}

// =============================================================================
// History/Activity Log API
// =============================================================================

/**
 * Get activity history for a run
 */
export async function getHistory(
  runName: string,
  limit?: number
): Promise<HistoryEntry[]> {
  return invoke<HistoryEntry[]>('get_history', { runName, limit });
}

// =============================================================================
// Configuration API
// =============================================================================

/**
 * Get application configuration
 */
export async function getConfig(): Promise<Config> {
  return invoke<Config>('get_config');
}

/**
 * Set the active agent
 */
export async function setAgent(agentName: string): Promise<void> {
  return invoke('set_agent', { agentName });
}

/**
 * Get list of available agents
 */
export async function getAgents(): Promise<string[]> {
  return invoke<string[]>('get_agents');
}

// =============================================================================
// Diff API
// =============================================================================

/**
 * Get diff between run's work and original project
 */
export async function getDiff(runName: string): Promise<string> {
  return invoke<string>('get_diff', { runName });
}

/**
 * Get diff statistics (files changed, insertions, deletions)
 */
export async function getDiffStats(runName: string): Promise<{
  filesChanged: number;
  insertions: number;
  deletions: number;
}> {
  return invoke('get_diff_stats', { runName });
}

// =============================================================================
// Utility Functions
// =============================================================================

/**
 * Check if the Tauri API is available (running in desktop app)
 */
export function isTauriAvailable(): boolean {
  return typeof window !== 'undefined' && '__TAURI__' in window;
}

/**
 * Parse error from Tauri invoke
 * Errors from Tauri can be GuiError objects (JSON) or plain strings
 */
function parseError(error: unknown): { message: string; guiError?: GuiError } {
  // Try to parse as GuiError
  if (error && typeof error === 'object' && 'code' in error && 'message' in error) {
    return {
      message: (error as GuiError).message,
      guiError: error as GuiError,
    };
  }

  // Try to parse JSON string (Tauri sometimes returns errors as JSON strings)
  if (typeof error === 'string') {
    try {
      const parsed = JSON.parse(error);
      if (parsed && typeof parsed === 'object' && 'code' in parsed && 'message' in parsed) {
        return {
          message: parsed.message,
          guiError: parsed as GuiError,
        };
      }
    } catch {
      // Not JSON, use as-is
    }
    return { message: error };
  }

  // Handle standard Error
  if (error instanceof Error) {
    return { message: error.message };
  }

  return { message: 'An unexpected error occurred' };
}

/**
 * Safe invoke wrapper that returns ApiResult
 */
export async function safeInvoke<T>(
  command: string,
  args?: Record<string, unknown>
): Promise<ApiResult<T>> {
  try {
    const data = await invoke<T>(command, args);
    return { success: true, data };
  } catch (error) {
    const { message, guiError } = parseError(error);
    return {
      success: false,
      error: message,
      guiError,
    };
  }
}

/**
 * Safe invoke with automatic toast notification on error
 */
export async function safeInvokeWithToast<T>(
  command: string,
  args?: Record<string, unknown>,
  options?: {
    successMessage?: string;
    errorPrefix?: string;
  }
): Promise<ApiResult<T>> {
  const result = await safeInvoke<T>(command, args);

  if (result.success) {
    if (options?.successMessage) {
      toast.success(options.successMessage);
    }
  } else {
    const prefix = options?.errorPrefix ? `${options.errorPrefix}: ` : '';
    if (result.guiError) {
      toast.fromGuiError(result.guiError);
    } else {
      toast.error('Error', `${prefix}${result.error}`);
    }
  }

  return result;
}

/**
 * Poll for updates at regular intervals
 */
export function createPoller<T>(
  fetcher: () => Promise<T>,
  onUpdate: (data: T) => void,
  intervalMs: number = 2000
): { start: () => void; stop: () => void } {
  let intervalId: ReturnType<typeof setInterval> | null = null;

  return {
    start() {
      if (intervalId) return;

      // Fetch immediately
      fetcher().then(onUpdate).catch(console.error);

      // Then poll at interval
      intervalId = setInterval(() => {
        fetcher().then(onUpdate).catch(console.error);
      }, intervalMs);
    },

    stop() {
      if (intervalId) {
        clearInterval(intervalId);
        intervalId = null;
      }
    },
  };
}

// =============================================================================
// Event Helpers
// =============================================================================

/**
 * Dispatch a custom event for Alpine.js components
 */
export function dispatchEvent<T>(eventName: string, detail: T): void {
  window.dispatchEvent(new CustomEvent(eventName, { detail }));
}

/**
 * Listen for a custom event
 */
export function onEvent<T>(
  eventName: string,
  handler: (detail: T) => void
): () => void {
  const listener = (event: Event) => {
    handler((event as CustomEvent<T>).detail);
  };

  window.addEventListener(eventName, listener);

  // Return cleanup function
  return () => window.removeEventListener(eventName, listener);
}
