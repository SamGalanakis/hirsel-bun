/**
 * Tauri API wrappers for Hirsel
 *
 * This module provides typed wrappers around Tauri's invoke() function
 * for calling Rust backend commands. All functions handle errors gracefully
 * and provide type-safe interfaces.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from './toast';
import type {
  ApiResult,
  ChatEvent,
  Config,
  DraftUpdateRequest,
  Eval,
  GuiError,
  HistoryEntry,
  Message,
  RepoValidation,
  RunDetail,
  RunSummary,
  StartingPoint,
  Task,
  ThreadSummary,
  UIContext,
  Worker,
  WorkerEvent,
  WorkerEventsResponse,
  WorkerLogResponse,
  WorkerStreamEvent,
} from './types';

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
  return invoke<RunDetail>('get_run_detail', { runName: name });
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
  },
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
  return invoke('pause_run', { runName: name });
}

/**
 * Resume a paused or timed-out run
 */
export async function resumeRun(name: string, timeLimit?: string): Promise<void> {
  return invoke('resume_run', { runName: name, timeLimit: timeLimit });
}

/**
 * Delete a run and all its data
 */
export async function deleteRun(name: string): Promise<void> {
  return invoke('delete_run', { runName: name });
}

/**
 * Clone a run to a new draft
 *
 * Creates a new draft run with the same settings, spec, and eval as the source run.
 * Does not copy messages, tasks, workers, or any runtime state.
 */
export async function cloneRun(sourceRun: string, newName: string): Promise<RunDetail> {
  return invoke<RunDetail>('clone_run', { sourceRun, newName });
}

// =============================================================================
// Draft Management API
// =============================================================================

/**
 * Create a new draft run
 *
 * Creates a draft run with a random friendly name. The draft can be configured
 * before being started. No workspace is created - that happens in startDraft
 * when the user chooses a starting point.
 */
export async function createDraft(): Promise<RunDetail> {
  return invoke<RunDetail>('create_draft');
}

/**
 * Update a draft run's configuration
 *
 * Allows updating the spec, worker scale, time limit, HITL mode, and project path
 * before the draft is started.
 */
export async function updateDraft(runName: string, updates: DraftUpdateRequest): Promise<void> {
  return invoke('update_draft', { runName, updates });
}

/**
 * Start a draft run
 *
 * Creates the workspace based on the starting point, spawns workers,
 * and transitions the draft to a running state.
 *
 * @param runName - The run name
 * @param startingPoint - How to initialize the workspace (greenfield, localFolder, or gitRepo)
 * @param profile - Optional profile name for remote orchestrator mode
 */
export async function startDraft(
  runName: string,
  startingPoint?: StartingPoint,
  profile?: string,
): Promise<RunDetail> {
  return invoke<RunDetail>('start_draft', { runName, startingPoint, profile });
}

/**
 * Change the starting point for a draft run
 *
 * Deletes the existing workspace and re-initializes it with a new starting point.
 * Only works for drafts (not running or completed runs).
 *
 * @param runName - The run name
 * @param startingPoint - How to initialize the new workspace
 */
export async function changeStartingPoint(
  runName: string,
  startingPoint: StartingPoint,
): Promise<RunDetail> {
  return invoke<RunDetail>('change_starting_point', { runName, startingPoint });
}

/**
 * Open a native folder picker dialog
 *
 * @returns The selected folder path, or null if cancelled
 */
export async function pickFolder(): Promise<string | null> {
  return invoke<string | null>('pick_folder');
}

/**
 * Get path suggestions for autocomplete
 *
 * @param partial - Partial path to complete
 * @returns List of matching directory paths
 */
export async function suggestPaths(partial: string): Promise<string[]> {
  return invoke<string[]>('suggest_paths', { partial });
}

/**
 * Validate a repository path or URL
 *
 * Checks if the path/URL is valid, extracts branch information from URLs,
 * and lists available branches in the repository.
 */
export async function validateRepo(path: string): Promise<RepoValidation> {
  return invoke<RepoValidation>('validate_repo', { path });
}

// =============================================================================
// Spec/Eval File API (file-first editing)
// =============================================================================

/**
 * Read the spec.md file for a run
 */
export async function readSpecFile(runName: string): Promise<string> {
  return invoke<string>('read_spec_file', { runName });
}

/**
 * Write the spec.md file for a run
 */
export async function writeSpecFile(runName: string, content: string): Promise<void> {
  return invoke('write_spec_file', { runName, content });
}

/**
 * Read the eval.md file for a run
 */
export async function readEvalFile(runName: string): Promise<string> {
  return invoke<string>('read_eval_file', { runName });
}

/**
 * Write the eval.md file for a run
 */
export async function writeEvalFile(runName: string, content: string): Promise<void> {
  return invoke('write_eval_file', { runName, content });
}

// =============================================================================
// Asset API
// =============================================================================

/**
 * Save an asset file (image, etc.) to a run's assets directory
 * @returns The filename that was saved (may differ from original if name conflict)
 */
export async function saveAsset(
  runName: string,
  filename: string,
  data: number[],
): Promise<string> {
  return invoke<string>('save_asset', { runName, filename, data });
}

/**
 * Import an asset from a filesystem path (used for native drag-drop)
 * @returns The filename that was saved (may differ from original if name conflict)
 */
export async function importAssetFromPath(runName: string, filePath: string): Promise<string> {
  return invoke<string>('import_asset_from_path', { runName, filePath });
}

/**
 * Open the assets folder for a run in the system file browser
 */
export async function openAssetsFolder(runName: string): Promise<void> {
  return invoke('open_assets_folder', { runName });
}

/**
 * Get the assets directory path for a run
 */
export async function getAssetsPath(runName: string): Promise<string> {
  return invoke<string>('get_assets_path', { runName });
}

// =============================================================================
// Run Delivery API
// =============================================================================

/**
 * Deliver run changes to project repo as a branch
 *
 * @param runName - The run name
 * @param branchName - Optional branch name (defaults to saved branch or hirsel/{runName})
 * @returns The branch name that was created
 */
export async function deliverRun(runName: string, branchName?: string): Promise<string> {
  return invoke<string>('deliver_run', { runName, branchName });
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
  return invoke<Task[]>('get_tasks', { runName: runName });
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
  },
): Promise<void> {
  return invoke('add_task', {
    runName: runName,
    taskId: taskId,
    description,
    parentId: options?.parentId,
    blockedBy: options?.blockedBy,
  });
}

/**
 * Delete a task from a run
 */
export async function deleteTask(runName: string, taskId: string): Promise<void> {
  return invoke('delete_task', { runName: runName, taskId: taskId });
}

/**
 * Mark a task as done
 */
export async function completeTask(runName: string, taskId: string): Promise<void> {
  return invoke('complete_task', { runName: runName, taskId: taskId });
}

/**
 * Reopen a completed task
 */
export async function reopenTask(runName: string, taskId: string): Promise<void> {
  return invoke('reopen_task', { runName: runName, taskId: taskId });
}

/**
 * Unclaim a task (release it back to todo)
 */
export async function unclaimTask(runName: string, taskId: string): Promise<void> {
  return invoke('unclaim_task', { runName: runName, taskId: taskId });
}

// =============================================================================
// Worker Management API
// =============================================================================

/**
 * Get all workers for a run
 */
export async function getWorkers(runName: string): Promise<Worker[]> {
  return invoke<Worker[]>('get_workers', { runName: runName });
}

/**
 * Spawn a new worker (creates worker clone and starts process)
 * Note: This doesn't attach to terminal, it creates a new worker.
 */
export async function attachWorker(runName: string, workerName: string): Promise<void> {
  return invoke('attach_worker', { runName: runName, workerName: workerName });
}

/**
 * Open an external terminal attached to a worker's tmux session
 */
export async function openWorkerTerminal(runName: string, workerName: string): Promise<void> {
  return invoke('open_worker_terminal', { runName: runName, workerName: workerName });
}

/**
 * Add a new worker to a run
 */
export async function addWorker(runName: string): Promise<string> {
  return invoke<string>('add_worker', { runName: runName });
}

/**
 * Get worker session metrics (tokens, context utilization)
 */
export async function getWorkerMetrics(
  runName: string,
  workerName: string,
): Promise<{
  inputTokens: number;
  outputTokens: number;
  turns: number;
  contextUtilization: number;
}> {
  return invoke('get_worker_metrics', { runName: runName, workerName: workerName });
}

// =============================================================================
// Eval Log API
// =============================================================================

/**
 * Get eval log content
 */
export async function getEvalLog(
  runName: string,
  options?: {
    lines?: number;
    fromOffset?: number;
  },
): Promise<WorkerLogResponse> {
  return invoke<WorkerLogResponse>('get_eval_log', {
    runName: runName,
    lines: options?.lines,
    fromOffset: options?.fromOffset,
  });
}

// =============================================================================
// Worker Events API (ACP-based streaming)
// =============================================================================

/**
 * Get worker events for real-time streaming
 *
 * @param runName - The run name
 * @param workerName - The worker name
 * @param afterId - Only return events after this ID (for efficient polling)
 * @param limit - Maximum number of events to return
 */
export async function getWorkerEvents(
  runName: string,
  workerName: string,
  options?: {
    afterId?: number;
    limit?: number;
  },
): Promise<WorkerEventsResponse> {
  return invoke<WorkerEventsResponse>('get_worker_events', {
    runName: runName,
    workerName: workerName,
    afterId: options?.afterId,
    limit: options?.limit,
  });
}

/**
 * Clear worker events (for cleanup when attaching/detaching)
 */
export async function clearWorkerEvents(runName: string, workerName: string): Promise<void> {
  return invoke('clear_worker_events', {
    runName: runName,
    workerName: workerName,
  });
}

/**
 * Create a poller for worker events
 *
 * This creates a poller that efficiently fetches only new events
 * since the last poll by tracking event IDs.
 */
export function createWorkerEventsPoller(
  runName: string,
  workerName: string,
  onEvents: (events: WorkerEvent[], isNew: boolean, workerStatus: string | null) => void,
  intervalMs = 200,
): { start: () => void; stop: () => void } {
  let intervalId: ReturnType<typeof setInterval> | null = null;
  let lastId: number | null = null;
  let isFirstPoll = true;

  const poll = async () => {
    try {
      const response = await getWorkerEvents(runName, workerName, {
        afterId: lastId ?? undefined,
      });

      // Always call onEvents to update worker status (even if no new events)
      onEvents(response.events, !isFirstPoll && response.events.length > 0, response.workerStatus);

      if (response.events.length > 0) {
        lastId = response.lastId;
      }

      isFirstPoll = false;
    } catch (error) {
      console.error('Error polling worker events:', error);
    }
  };

  return {
    start() {
      if (intervalId) return;
      poll(); // Fetch immediately
      intervalId = setInterval(poll, intervalMs);
    },
    stop() {
      if (intervalId) {
        clearInterval(intervalId);
        intervalId = null;
      }
      // Reset state
      lastId = null;
      isFirstPoll = true;
    },
  };
}

// =============================================================================
// Worker Event Streaming API (Tauri events)
// =============================================================================

/**
 * Start streaming worker events via Tauri events
 *
 * This starts a background task that polls the database and emits
 * `worker-event` events. Use listenWorkerEvents to receive them.
 */
export async function startWorkerEventStream(runName: string, workerName: string): Promise<void> {
  return invoke('start_worker_event_stream', { runName, workerName });
}

/**
 * Stop streaming worker events
 */
export async function stopWorkerEventStream(runName: string, workerName: string): Promise<void> {
  return invoke('stop_worker_event_stream', { runName, workerName });
}

/**
 * Listen for worker events from a stream
 *
 * @param handler - Function to handle worker stream events
 * @returns Cleanup function to stop listening
 */
export async function listenWorkerEvents(
  handler: (event: WorkerStreamEvent) => void,
): Promise<() => void> {
  const unlisten = await listen<WorkerStreamEvent>('worker-event', (event) => {
    handler(event.payload);
  });
  return unlisten;
}

// =============================================================================
// Message/Chat API
// =============================================================================

/**
 * Get all threads for a run
 */
export async function getThreads(runName: string): Promise<ThreadSummary[]> {
  return invoke<ThreadSummary[]>('get_threads', { runName: runName });
}

/**
 * Get messages for a specific thread
 */
export async function getMessages(runName: string, threadName: string): Promise<Message[]> {
  return invoke<Message[]>('get_messages', { runName: runName, threadName: threadName });
}

/**
 * Send a message to a thread
 */
export async function sendMessage(
  runName: string,
  threadName: string,
  content: string,
): Promise<void> {
  return invoke('send_message', {
    runName: runName,
    threadName: threadName,
    content,
  });
}

/**
 * Mark messages as read
 */
export async function markMessagesRead(runName: string, threadName: string): Promise<void> {
  return invoke('mark_messages_read', { runName: runName, threadName: threadName, reader: 'user' });
}

// =============================================================================
// Eval API
// =============================================================================

/**
 * Get the eval spec (eval.md) content for a run
 */
export async function getEvalSpec(runName: string): Promise<string | null> {
  return invoke<string | null>('get_eval_spec', { runName: runName });
}

/**
 * Attach to an eval's log output
 */
export async function attachEval(runName: string, evalName: string): Promise<void> {
  return invoke('attach_eval', { runName: runName, evalName: evalName });
}

// =============================================================================
// History/Activity Log API
// =============================================================================

/**
 * Get activity history for a run
 */
export async function getHistory(runName: string, limit?: number): Promise<HistoryEntry[]> {
  return invoke<HistoryEntry[]>('get_history', { runName: runName, limit });
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
  return invoke<string>('get_diff', { runName: runName });
}

/**
 * Get diff statistics (files changed, insertions, deletions)
 */
export async function getDiffStats(runName: string): Promise<{
  filesChanged: number;
  insertions: number;
  deletions: number;
}> {
  return invoke('get_diff_stats', { runName: runName });
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
  args?: Record<string, unknown>,
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
  },
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
  intervalMs = 2000,
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
export function onEvent<T>(eventName: string, handler: (detail: T) => void): () => void {
  const listener = (event: Event) => {
    handler((event as CustomEvent<T>).detail);
  };

  window.addEventListener(eventName, listener);

  // Return cleanup function
  return () => window.removeEventListener(eventName, listener);
}

// =============================================================================
// Direct Chat Session API (ACP-based AI chat)
// =============================================================================

/**
 * Start a new direct chat session with an AI agent
 *
 * @param agentCommand - Command to run the agent (e.g., ["claude", "acp"])
 * @param options - Session options
 * @returns Session ID
 */
export async function startChatSession(
  agentCommand: string[],
  options?: {
    workingDir?: string;
    runName?: string;
    systemPrompt?: string;
  },
): Promise<string> {
  return invoke<string>('start_chat_session', {
    agentCommand,
    workingDir: options?.workingDir,
    runName: options?.runName,
    systemPrompt: options?.systemPrompt,
  });
}

/**
 * Send a message to a chat session
 *
 * @param sessionId - The session ID
 * @param content - Message content
 * @param context - UI context to inject (invisible to user)
 */
export async function sendChatMessage(
  sessionId: string,
  content: string,
  context?: UIContext,
): Promise<void> {
  return invoke('send_chat_message', {
    sessionId,
    content,
    context,
  });
}

/**
 * Respond to a permission request from a chat session
 *
 * @param sessionId - The session ID
 * @param requestId - The permission request ID
 * @param optionId - The selected option ID
 */
export async function respondChatPermission(
  sessionId: string,
  requestId: string,
  optionId: string,
): Promise<void> {
  return invoke('respond_chat_permission', {
    sessionId,
    requestId,
    optionId,
  });
}

/**
 * Stop a chat session
 *
 * @param sessionId - The session ID
 */
export async function stopChatSession(sessionId: string): Promise<void> {
  return invoke('stop_chat_session', { sessionId });
}

/**
 * List active chat sessions
 */
export async function listChatSessions(): Promise<string[]> {
  return invoke<string[]>('list_chat_sessions');
}

/**
 * Listen for chat events from a session
 *
 * @param handler - Function to handle chat events
 * @returns Cleanup function to stop listening
 */
export async function listenChatEvents(handler: (event: ChatEvent) => void): Promise<() => void> {
  const unlisten = await listen<ChatEvent>('chat-event', (event) => {
    handler(event.payload);
  });

  return unlisten;
}

// =============================================================================
// Gyp Chat History API
// =============================================================================

/** Gyp chat message stored in database */
export interface GypChatMessage {
  id: number;
  runName: string | null;
  role: string;
  timestamp: string;
  chunksJson: string;
}

/**
 * Get Gyp chat history for a run (or no-run if null)
 *
 * @param runName - The run name, or null for no-run conversations
 * @returns Array of saved chat messages
 */
export async function getGypChatHistory(runName: string | null): Promise<GypChatMessage[]> {
  return invoke<GypChatMessage[]>('get_gyp_chat_history', { runName });
}

/**
 * Save a Gyp chat message for a run (or no-run if null)
 *
 * @param runName - The run name, or null for no-run conversations
 * @param role - Message role ('user', 'assistant', 'system')
 * @param chunksJson - JSON-encoded chunks array
 * @returns The saved message ID
 */
export async function saveGypMessage(
  runName: string | null,
  role: string,
  chunksJson: string,
): Promise<number> {
  return invoke<number>('save_gyp_message', { runName, role, chunksJson });
}

/**
 * Clear Gyp chat history for a run (or no-run if null)
 *
 * @param runName - The run name, or null for no-run conversations
 */
export async function clearGypChatHistory(runName: string | null): Promise<void> {
  return invoke('clear_gyp_chat_history', { runName });
}

// =============================================================================
// Project Messages API (Sheepfold)
// =============================================================================

import type { ProjectMessage, ProjectThreadSummary } from './types';

/**
 * Get messages for a project thread (Meadow or worker DM)
 */
export async function getProjectMessages(
  projectId: number,
  thread: string,
  limit?: number,
): Promise<ProjectMessage[]> {
  return invoke<ProjectMessage[]>('get_project_messages', { projectId, thread, limit });
}

/**
 * Get all threads for a project with unread counts
 */
export async function getProjectThreads(projectId: number): Promise<ProjectThreadSummary[]> {
  return invoke<ProjectThreadSummary[]>('get_project_threads', { projectId });
}

/**
 * Send a message to a project thread
 */
export async function sendProjectMessage(
  projectId: number,
  thread: string,
  content: string,
): Promise<ProjectMessage> {
  return invoke<ProjectMessage>('send_project_message', { projectId, thread, content });
}

/**
 * Mark messages in a thread as read
 */
export async function markProjectMessagesRead(projectId: number, thread: string): Promise<void> {
  return invoke('mark_project_messages_read', { projectId, thread });
}

/**
 * Get total unread count for a project
 */
export async function getProjectUnreadCount(projectId: number): Promise<number> {
  return invoke<number>('get_project_unread_count', { projectId });
}
