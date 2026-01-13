/**
 * Hirsel Desktop App - Main View Entry Point
 * Implements the webview UI for the Electrobun desktop app
 */

import { Electroview } from "electrobun/view";
import type { HirselRPC, RunSummary, RunDetail, HistoryEntry } from "../../shared/rpc-types";

// =============================================================================
// Status Icons (matching theme.ts)
// =============================================================================

const STATUS_ICONS: Record<string, string> = {
  // Run/Worker status
  working: "●",
  waiting: "◐",
  awaiting: "◔",
  idle: "○",
  done: "✓",
  error: "✗",
  paused: "⏸",
  // Task status
  doing: "►",
  todo: "○",
  // Run-specific
  delivered: "✓",
  merged: "✓",
  eval: "●",
  eval_failed: "✗",
  runaway: "✗",
  timed_out: "⏸",
};

function getStatusIcon(status: string): string {
  return STATUS_ICONS[status] ?? "○";
}

// =============================================================================
// State
// =============================================================================

interface ChatMessage {
  sender: string;
  content: string;
  timestamp: string;
  isOutgoing: boolean;
}

interface AppState {
  runs: RunSummary[];
  selectedRunName: string | null;
  selectedRun: RunDetail | null;
  chatOpen: boolean;
  chatThread: string;
  chatThreads: string[];
  chatMessages: Map<string, ChatMessage[]>;
  helpOpen: boolean;
}

const state: AppState = {
  runs: [],
  selectedRunName: null,
  selectedRun: null,
  chatOpen: false,
  chatThread: "user",
  chatThreads: ["user"],
  chatMessages: new Map(),
  helpOpen: false,
};

// =============================================================================
// DOM Elements
// =============================================================================

const runListEl = document.getElementById("run-list") as HTMLElement;
const emptyStateEl = document.getElementById("empty-state") as HTMLElement;
const runDetailEl = document.getElementById("run-detail") as HTMLElement;
const runNameEl = document.getElementById("run-name") as HTMLElement;
const runStatusEl = document.getElementById("run-status") as HTMLElement;
const workersListEl = document.getElementById("workers-list") as HTMLElement;
const tasksListEl = document.getElementById("tasks-list") as HTMLElement;
const activityLogEl = document.getElementById("activity-log") as HTMLElement;
const statusTextEl = document.getElementById("status-text") as HTMLElement;

// Chat elements
const chatPanelEl = document.getElementById("chat-panel") as HTMLElement;
const chatThreadsEl = document.getElementById("chat-threads") as HTMLElement;
const chatMessagesEl = document.getElementById("chat-messages") as HTMLElement;
const chatInputEl = document.getElementById("chat-input") as HTMLInputElement;
const chatSendEl = document.getElementById("chat-send") as HTMLButtonElement;
const chatCloseEl = document.getElementById("chat-close") as HTMLButtonElement;

// Help overlay
const helpOverlayEl = document.getElementById("help-overlay") as HTMLElement;

// =============================================================================
// RunList Component
// =============================================================================

function renderRunList(runs: RunSummary[], selectedName: string | null): void {
  if (runs.length === 0) {
    runListEl.innerHTML = '<div class="empty-state">No runs yet</div>';
    return;
  }

  runListEl.innerHTML = runs
    .map((run) => {
      const isSelected = run.name === selectedName;
      const statusClass = run.status;
      const icon = getStatusIcon(run.status);
      const progress = run.tasksTotal > 0
        ? `${run.tasksDone}/${run.tasksTotal}`
        : "";

      return `
        <div class="run-item ${statusClass} ${isSelected ? "selected" : ""}"
             data-run="${run.name}">
          <span class="icon">${icon}</span>
          <span class="name">${run.name}</span>
          ${progress ? `<span class="progress">${progress}</span>` : ""}
        </div>
      `;
    })
    .join("");

  // Add click handlers
  runListEl.querySelectorAll(".run-item").forEach((el) => {
    el.addEventListener("click", () => {
      const runName = el.getAttribute("data-run");
      if (runName) {
        selectRun(runName);
      }
    });
  });
}

// =============================================================================
// Run Detail Component
// =============================================================================

function renderRunDetail(detail: RunDetail | null): void {
  if (!detail) {
    emptyStateEl.classList.remove("hidden");
    runDetailEl.classList.add("hidden");
    return;
  }

  emptyStateEl.classList.add("hidden");
  runDetailEl.classList.remove("hidden");

  // Header
  runNameEl.textContent = detail.name;
  runStatusEl.textContent = detail.status;
  runStatusEl.className = `status-badge ${detail.status}`;

  // Workers - pass tasks to determine current task
  renderWorkersList(detail.workers, detail.tasks);

  // Tasks
  renderTasksList(detail.tasks);

  // Activity
  renderActivityLog(detail.activity);
}

function renderWorkersList(workers: RunDetail["workers"], _tasks: RunDetail["tasks"]): void {
  if (workers.length === 0) {
    workersListEl.innerHTML = '<div class="empty-state">No workers</div>';
    return;
  }

  workersListEl.innerHTML = workers
    .map((worker) => {
      const icon = getStatusIcon(worker.status);
      const badge = worker.isLeader ? "★" : "";
      const task = worker.currentTask ? `→ ${worker.currentTask}` : "";

      return `
        <div class="worker-item ${worker.status}" data-worker="${worker.name}">
          <span class="icon">${icon}</span>
          <span class="name">${worker.name}</span>
          ${badge ? `<span class="badge">${badge}</span>` : ""}
          ${task ? `<span class="task">${task}</span>` : ""}
        </div>
      `;
    })
    .join("");

  // Add double-click to attach
  workersListEl.querySelectorAll(".worker-item").forEach((el) => {
    el.addEventListener("dblclick", () => {
      const workerName = el.getAttribute("data-worker");
      if (workerName && state.selectedRunName) {
        attachWorker(state.selectedRunName, workerName);
      }
    });
  });
}

function renderTasksList(tasks: RunDetail["tasks"]): void {
  if (tasks.length === 0) {
    tasksListEl.innerHTML = '<div class="empty-state">No tasks</div>';
    return;
  }

  // Render tasks as flat list (hierarchical rendering can be added later)
  const flatTasks = flattenTasks(tasks);

  tasksListEl.innerHTML = flatTasks
    .map(({ task, depth }) => {
      const icon = getStatusIcon(task.status);
      const indent = depth > 0 ? `padding-left: ${depth * 16}px;` : "";
      const worker = task.claimedBy ? `(${task.claimedBy})` : "";

      return `
        <div class="task-item ${task.status}" style="${indent}">
          <span class="icon">${icon}</span>
          <span class="id">${task.id}</span>
          ${worker ? `<span class="worker">${worker}</span>` : ""}
        </div>
      `;
    })
    .join("");
}

function flattenTasks(
  tasks: RunDetail["tasks"],
  depth = 0
): { task: RunDetail["tasks"][0]; depth: number }[] {
  const result: { task: RunDetail["tasks"][0]; depth: number }[] = [];
  for (const task of tasks) {
    result.push({ task, depth });
    if (task.children && task.children.length > 0) {
      result.push(...flattenTasks(task.children, depth + 1));
    }
  }
  return result;
}

function renderActivityLog(activity: HistoryEntry[]): void {
  if (activity.length === 0) {
    activityLogEl.innerHTML = '<div class="empty-state">No activity</div>';
    return;
  }

  // Show most recent first, limit to 50
  const recent = activity.slice(-50).reverse();

  activityLogEl.innerHTML = recent
    .map((entry) => {
      const time = formatTime(entry.timestamp);
      return `
        <div class="activity-item">
          <span class="time">${time}</span>
          <span class="action">${entry.action}</span>
          ${entry.detail ? ` ${entry.detail}` : ""}
        </div>
      `;
    })
    .join("");
}

// =============================================================================
// Chat Panel Component
// =============================================================================

function toggleChatPanel(): void {
  state.chatOpen = !state.chatOpen;
  if (state.chatOpen) {
    chatPanelEl.classList.remove("hidden");
    chatInputEl.focus();
  } else {
    chatPanelEl.classList.add("hidden");
  }
}

function closeChatPanel(): void {
  state.chatOpen = false;
  chatPanelEl.classList.add("hidden");
}

function selectChatThread(thread: string): void {
  state.chatThread = thread;
  renderChatThreads();
  renderChatMessages();
}

function renderChatThreads(): void {
  chatThreadsEl.innerHTML = state.chatThreads
    .map((thread) => {
      const isActive = thread === state.chatThread;
      const messages = state.chatMessages.get(thread) || [];
      const hasUnread = messages.some((m) => !m.isOutgoing); // Simplified unread check
      return `
        <button class="chat-thread-btn ${isActive ? "active" : ""} ${hasUnread ? "unread" : ""}"
                data-thread="${thread}">
          ${thread}
        </button>
      `;
    })
    .join("");

  // Add click handlers
  chatThreadsEl.querySelectorAll(".chat-thread-btn").forEach((el) => {
    el.addEventListener("click", () => {
      const thread = el.getAttribute("data-thread");
      if (thread) {
        selectChatThread(thread);
      }
    });
  });
}

function renderChatMessages(): void {
  const messages = state.chatMessages.get(state.chatThread) || [];

  if (messages.length === 0) {
    chatMessagesEl.innerHTML = '<div class="empty-state">No messages</div>';
    return;
  }

  chatMessagesEl.innerHTML = messages
    .map((msg) => {
      const direction = msg.isOutgoing ? "outgoing" : "incoming";
      const time = formatTime(msg.timestamp);
      return `
        <div class="chat-message ${direction}">
          ${!msg.isOutgoing ? `<div class="sender">${msg.sender}</div>` : ""}
          <div class="content">${escapeHtml(msg.content)}</div>
          <div class="time">${time}</div>
        </div>
      `;
    })
    .join("");

  // Scroll to bottom
  chatMessagesEl.scrollTop = chatMessagesEl.scrollHeight;
}

async function sendChatMessage(): Promise<void> {
  const content = chatInputEl.value.trim();
  if (!content || !state.selectedRunName) return;

  chatInputEl.value = "";
  chatSendEl.disabled = true;

  try {
    await rpc.request.sendMessage({
      runName: state.selectedRunName,
      thread: state.chatThread,
      message: content,
    });

    // Add to local state immediately
    const messages = state.chatMessages.get(state.chatThread) || [];
    messages.push({
      sender: "user",
      content,
      timestamp: new Date().toISOString(),
      isOutgoing: true,
    });
    state.chatMessages.set(state.chatThread, messages);
    renderChatMessages();
  } catch (e) {
    console.error("Failed to send message:", e);
    updateStatusBar("Failed to send message");
  } finally {
    chatSendEl.disabled = false;
  }
}

function escapeHtml(text: string): string {
  const div = document.createElement("div");
  div.textContent = text;
  return div.innerHTML;
}

// =============================================================================
// Help Overlay
// =============================================================================

function toggleHelp(): void {
  state.helpOpen = !state.helpOpen;
  if (state.helpOpen) {
    helpOverlayEl.classList.remove("hidden");
  } else {
    helpOverlayEl.classList.add("hidden");
  }
}

function closeHelp(): void {
  state.helpOpen = false;
  helpOverlayEl.classList.add("hidden");
}

// =============================================================================
// Helpers
// =============================================================================

function formatTime(timestamp: string): string {
  const date = new Date(timestamp);
  return date.toLocaleTimeString("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

function formatElapsed(minutes: number): string {
  if (minutes < 1) return "<1m";
  if (minutes < 60) return `${Math.floor(minutes)}m`;
  const hours = Math.floor(minutes / 60);
  const mins = Math.floor(minutes % 60);
  return mins > 0 ? `${hours}h${mins}m` : `${hours}h`;
}

function updateStatusBar(message: string): void {
  statusTextEl.textContent = message;
}

// =============================================================================
// RPC Setup
// =============================================================================

let rpc: ReturnType<typeof Electroview.defineRPC<HirselRPC>>;

async function selectRun(runName: string): Promise<void> {
  state.selectedRunName = runName;
  updateStatusBar(`Loading ${runName}...`);

  try {
    const detail = await rpc.request.getRunDetail({ runName });
    state.selectedRun = detail;
    renderRunList(state.runs, runName);
    renderRunDetail(detail);
    updateStatusBar(`Run: ${runName}`);
  } catch (e) {
    console.error("Failed to load run detail:", e);
    updateStatusBar(`Error loading ${runName}`);
  }
}

async function attachWorker(runName: string, workerName: string): Promise<void> {
  try {
    const result = await rpc.request.attachWorker({ runName, workerName });
    if (!result.success) {
      console.error("Failed to attach:", result.error);
    }
  } catch (e) {
    console.error("Failed to attach worker:", e);
  }
}

async function refreshRuns(): Promise<void> {
  try {
    const runs = await rpc.request.getRuns({});
    state.runs = runs;
    renderRunList(runs, state.selectedRunName);

    // Refresh selected run detail if one is selected
    if (state.selectedRunName) {
      const detail = await rpc.request.getRunDetail({ runName: state.selectedRunName });
      state.selectedRun = detail;
      renderRunDetail(detail);
    }
  } catch (e) {
    console.error("Failed to refresh runs:", e);
  }
}

// =============================================================================
// Keyboard Shortcuts
// =============================================================================

function setupKeyboardShortcuts(): void {
  document.addEventListener("keydown", (e) => {
    // If typing in an input, only handle Escape
    const target = e.target as HTMLElement;
    const isInput = target.tagName === "INPUT" || target.tagName === "TEXTAREA";

    if (isInput) {
      if (e.key === "Escape") {
        (target as HTMLInputElement).blur();
        if (state.chatOpen) closeChatPanel();
      }
      if (e.key === "Enter" && target === chatInputEl) {
        e.preventDefault();
        sendChatMessage();
      }
      return;
    }

    // Help overlay - any key closes it
    if (state.helpOpen) {
      e.preventDefault();
      closeHelp();
      return;
    }

    // Cmd/Ctrl + R to refresh
    if ((e.metaKey || e.ctrlKey) && e.key === "r") {
      e.preventDefault();
      refreshRuns();
      return;
    }

    // Arrow keys to navigate runs
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      navigateRuns(e.key === "ArrowDown" ? 1 : -1);
      return;
    }

    // Escape to close panels or deselect
    if (e.key === "Escape") {
      if (state.chatOpen) {
        closeChatPanel();
      } else if (state.selectedRunName) {
        state.selectedRunName = null;
        state.selectedRun = null;
        renderRunList(state.runs, null);
        renderRunDetail(null);
      }
      return;
    }

    // Single-key shortcuts
    switch (e.key.toLowerCase()) {
      case "?":
        toggleHelp();
        break;
      case "c":
        toggleChatPanel();
        break;
      case "m":
        // Open chat and focus input for quick message
        if (!state.chatOpen) toggleChatPanel();
        chatInputEl.focus();
        break;
      case "a":
        // Attach to first available worker
        if (state.selectedRun && state.selectedRunName) {
          const worker = state.selectedRun.workers.find(
            (w) => w.status === "working" || w.status === "waiting"
          );
          if (worker) {
            attachWorker(state.selectedRunName, worker.name);
          }
        }
        break;
      case "p":
        // Pause selected run
        if (state.selectedRunName) {
          pauseRun(state.selectedRunName);
        }
        break;
      case "r":
        // Resume selected run (without Cmd/Ctrl)
        if (!e.metaKey && !e.ctrlKey && state.selectedRunName) {
          resumeRun(state.selectedRunName);
        }
        break;
      case "d":
        // Deliver selected run
        if (state.selectedRunName) {
          deliverRun(state.selectedRunName);
        }
        break;
      case "n":
        // New run - show prompt/dialog
        showNewRunPrompt();
        break;
    }
  });
}

async function pauseRun(runName: string): Promise<void> {
  try {
    updateStatusBar(`Pausing ${runName}...`);
    await rpc.request.pauseRun({ runName });
    updateStatusBar(`Paused ${runName}`);
    refreshRuns();
  } catch (e) {
    console.error("Failed to pause run:", e);
    updateStatusBar(`Failed to pause ${runName}`);
  }
}

async function resumeRun(runName: string): Promise<void> {
  try {
    updateStatusBar(`Resuming ${runName}...`);
    await rpc.request.resumeRun({ runName });
    updateStatusBar(`Resumed ${runName}`);
    refreshRuns();
  } catch (e) {
    console.error("Failed to resume run:", e);
    updateStatusBar(`Failed to resume ${runName}`);
  }
}

async function deliverRun(runName: string): Promise<void> {
  try {
    updateStatusBar(`Delivering ${runName}...`);
    await rpc.request.deliverRun({ runName });
    updateStatusBar(`Delivered ${runName}`);
    refreshRuns();
  } catch (e) {
    console.error("Failed to deliver run:", e);
    updateStatusBar(`Failed to deliver ${runName}`);
  }
}

function showNewRunPrompt(): void {
  // For now, just show a simple prompt
  // In production, would use a modal dialog
  const name = prompt("Enter run name:");
  if (name) {
    createNewRun(name);
  }
}

async function createNewRun(name: string): Promise<void> {
  // Note: In production, this would show a dialog to select spec file and options
  // For now, just shows that we would create a run
  updateStatusBar(`Use 'hirsel go' to create a new run`);
}

function navigateRuns(direction: number): void {
  if (state.runs.length === 0) return;

  const currentIndex = state.selectedRunName
    ? state.runs.findIndex((r) => r.name === state.selectedRunName)
    : -1;

  let newIndex = currentIndex + direction;
  if (newIndex < 0) newIndex = state.runs.length - 1;
  if (newIndex >= state.runs.length) newIndex = 0;

  selectRun(state.runs[newIndex].name);
}

// =============================================================================
// Initialize
// =============================================================================

async function init(): Promise<void> {
  console.log("Hirsel main view initializing...");

  // Setup RPC - note: Electroview.defineRPC uses handlers.requests for request handlers
  // and handlers.messages for message handlers
  rpc = Electroview.defineRPC<HirselRPC>({
    handlers: {
      requests: {
        refreshState: async () => {
          await refreshRuns();
        },
      },
      messages: {
        stateUpdate: (params: unknown) => {
          const data = params as { runs: RunSummary[]; selectedRun: RunDetail | null };
          state.runs = data.runs;
          state.selectedRun = data.selectedRun;
          if (data.selectedRun) {
            state.selectedRunName = data.selectedRun.name;
          }
          renderRunList(state.runs, state.selectedRunName);
          renderRunDetail(state.selectedRun);
        },
        notification: (params: unknown) => {
          const data = params as { type: "info" | "warning" | "error"; title: string; message: string };
          console.log(`[${data.type}] ${data.title}: ${data.message}`);
          // Could show a toast notification here
        },
        activityUpdate: (params: unknown) => {
          const data = params as { runName: string; entry: HistoryEntry };
          if (state.selectedRunName === data.runName && state.selectedRun) {
            state.selectedRun.activity.push(data.entry);
            renderActivityLog(state.selectedRun.activity);
          }
        },
        messageUpdate: (params: unknown) => {
          const data = params as { runName: string; thread: string; message: { sender: string; content: string; timestamp: string } };
          // Add to chat messages
          if (!state.chatThreads.includes(data.thread)) {
            state.chatThreads.push(data.thread);
          }
          const messages = state.chatMessages.get(data.thread) || [];
          messages.push({
            sender: data.message.sender,
            content: data.message.content,
            timestamp: data.message.timestamp,
            isOutgoing: false,
          });
          state.chatMessages.set(data.thread, messages);

          // Re-render if chat is open
          if (state.chatOpen) {
            renderChatThreads();
            if (data.thread === state.chatThread) {
              renderChatMessages();
            }
          }
        },
      },
    },
  });

  // Setup keyboard shortcuts
  setupKeyboardShortcuts();

  // Setup chat panel
  chatCloseEl.addEventListener("click", closeChatPanel);
  chatSendEl.addEventListener("click", sendChatMessage);
  renderChatThreads();
  renderChatMessages();

  // Initial render
  renderRunList([], null);
  renderRunDetail(null);

  // Fetch initial data
  await refreshRuns();

  updateStatusBar("Ready");
  console.log("Hirsel main view ready");
}

// Start the app
init().catch(console.error);
