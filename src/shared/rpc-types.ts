/**
 * RPC Types for Main Process ↔ Webview Communication
 *
 * Based on Electrobun RPC pattern with typed requests and messages.
 * Re-exports domain types from shared/types.ts for consistency.
 */

import type { RPCSchema } from "electrobun/bun";

// Re-export domain types from shared types module
export {
  Status,
  EvalStatus,
  WorkerStatus,
  TaskStatus,
  type RunSummary,
  type RunDetails,
  type Worker,
  type Task,
  type Message,
  type HistoryEntry,
  type Eval,
  type BranchInfo,
} from "./types";

// Import for use in RPC definitions
import type {
  RunSummary,
  RunDetails,
  Worker,
  Task,
  Message,
  HistoryEntry,
  Eval,
} from "./types";

// =============================================================================
// RPC-specific Types (not in shared/types.ts)
// =============================================================================

/** Activity entry alias */
export type ActivityEntry = HistoryEntry;

/**
 * UI-friendly run detail type.
 * Maps `history` to `activity` for cleaner UI code.
 */
export interface RunDetail extends Omit<RunDetails, "history"> {
  /** Activity log (alias for history) */
  activity: HistoryEntry[];
  /** Workers with UI-friendly properties */
  workers: Array<
    Worker & {
      /** Whether worker is the leader (first worker) */
      isLeader?: boolean;
      /** Current task being worked on */
      currentTask?: string | null;
    }
  >;
  /** Tasks with children for tree rendering */
  tasks: Array<
    Task & {
      /** Child tasks */
      children?: Task[];
    }
  >;
}

/** Thread summary for UI */
export interface Thread {
  name: string;
  messageCount: number;
  unreadCount: number;
  lastMessage: string | null;
}

/** Options for creating a new run */
export interface CreateRunOptions {
  workers: number | string;
  timeLimit: string | null;
  humanInTheLoop: boolean;
}

// =============================================================================
// Status type aliases for compatibility
// =============================================================================

export type RunStatus =
  | "idle"
  | "working"
  | "paused"
  | "runaway"
  | "timed_out"
  | "eval"
  | "eval_failed"
  | "waiting"
  | "done"
  | "delivered"
  | "merged";

// =============================================================================
// RPC Schema Definition
// =============================================================================

/**
 * Main RPC type definition for Electrobun.
 *
 * - bun: Handlers in the main Bun process (called by webview)
 * - webview: Handlers in the webview (called by main process)
 */
export type HirselRPC = {
  bun: RPCSchema<{
    requests: {
      /** Get list of all runs */
      getRuns: {
        params: {};
        response: RunSummary[];
      };

      /** Get detailed info for a specific run */
      getRunDetail: {
        params: { runName: string };
        response: RunDetail | null;
      };

      /** Get messages for a thread */
      getMessages: {
        params: { runName: string; thread: string; limit?: number };
        response: Message[];
      };

      /** Get activity log for a run */
      getActivity: {
        params: { runName: string; limit?: number };
        response: HistoryEntry[];
      };

      /** Pause a run */
      pauseRun: {
        params: { runName: string };
        response: { success: boolean; error?: string };
      };

      /** Resume a run */
      resumeRun: {
        params: { runName: string; timeLimit?: string };
        response: { success: boolean; error?: string };
      };

      /** Delete a run */
      deleteRun: {
        params: { runName: string };
        response: { success: boolean; error?: string };
      };

      /** Deliver a run (create branch in target repo) */
      deliverRun: {
        params: { runName: string; branchName?: string };
        response: { success: boolean; branchName?: string; error?: string };
      };

      /** Attach to a worker (opens terminal) */
      attachWorker: {
        params: { runName: string; workerName: string };
        response: { success: boolean; error?: string };
      };

      /** Send a message to a thread */
      sendMessage: {
        params: { runName: string; thread: string; message: string };
        response: { success: boolean; error?: string };
      };

      /** Create a new run */
      createRun: {
        params: {
          runName: string;
          specPath: string;
          options: CreateRunOptions;
        };
        response: { success: boolean; error?: string };
      };

      /** Get diff for a run */
      getDiff: {
        params: { runName: string };
        response: { diff: string; stat: string };
      };

      /** Mark a task as done */
      markTaskDone: {
        params: { runName: string; taskId: string };
        response: { success: boolean; error?: string };
      };

      /** Unclaim a task */
      unclaimTask: {
        params: { runName: string; taskId: string };
        response: { success: boolean; error?: string };
      };

      /** Add a new task */
      addTask: {
        params: {
          runName: string;
          taskId: string;
          description: string;
          parentId?: string;
          blockedBy?: string[];
        };
        response: { success: boolean; error?: string };
      };

      /** Delete a task */
      deleteTask: {
        params: { runName: string; taskId: string };
        response: { success: boolean; error?: string };
      };

      /** Reopen a completed task */
      reopenTask: {
        params: { runName: string; taskId: string };
        response: { success: boolean; error?: string };
      };
    };

    messages: {
      /** Log message from webview */
      log: { level: "debug" | "info" | "warn" | "error"; message: string };
    };
  }>;

  webview: RPCSchema<{
    requests: {
      /** Update UI with new state (for immediate refresh) */
      refreshState: {
        params: {};
        response: void;
      };
    };

    messages: {
      /** Push state update to webview */
      stateUpdate: {
        runs: RunSummary[];
        selectedRun: RunDetail | null;
      };

      /** Notification for user attention */
      notification: {
        type: "info" | "warning" | "error";
        title: string;
        message: string;
      };

      /** New activity entry */
      activityUpdate: {
        runName: string;
        entry: HistoryEntry;
      };

      /** New message in a thread */
      messageUpdate: {
        runName: string;
        thread: string;
        message: Message;
      };
    };
  }>;
};
