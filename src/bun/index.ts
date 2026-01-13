// Hirsel Desktop App - Main Process Entry Point
import { BrowserWindow, BrowserView, ApplicationMenu } from "electrobun/bun";
import type { HirselRPC } from "../shared/rpc-types";
import * as bridge from "./state-bridge";

// Set up application menu
ApplicationMenu.setApplicationMenu([
  {
    label: "File",
    submenu: [{ role: "quit" }],
  },
  {
    label: "Edit",
    submenu: [
      { role: "undo" },
      { role: "redo" },
      { type: "separator" },
      { role: "cut" },
      { role: "copy" },
      { role: "paste" },
      { role: "selectAll" },
    ],
  },
  {
    label: "View",
    submenu: [
      { role: "reload" },
      { role: "toggleDevTools" },
      { type: "separator" },
      { role: "zoomIn" },
      { role: "zoomOut" },
      { role: "resetZoom" },
    ],
  },
  {
    label: "Run",
    submenu: [
      {
        label: "New Run...",
        accelerator: "CmdOrCtrl+N",
        action: "new-run",
      },
      { type: "separator" },
      {
        label: "Pause",
        accelerator: "CmdOrCtrl+P",
        action: "pause-run",
      },
      {
        label: "Resume",
        accelerator: "CmdOrCtrl+Shift+R",
        action: "resume-run",
      },
    ],
  },
  {
    label: "Help",
    submenu: [
      {
        label: "Documentation",
        action: "show-docs",
      },
    ],
  },
]);

// Handle application menu actions
ApplicationMenu.on("application-menu-clicked", (event: { id: number; action: string; data?: unknown }) => {
  switch (event.action) {
    case "new-run":
      console.log("New run requested");
      break;
    case "pause-run":
      console.log("Pause requested");
      break;
    case "resume-run":
      console.log("Resume requested");
      break;
    case "show-docs":
      console.log("Help requested");
      break;
  }
});

// Define RPC handlers for webview communication
const mainRPC = BrowserView.defineRPC<HirselRPC>({
  maxRequestTime: 30000,
  handlers: {
    requests: {
      getRuns: async () => {
        return bridge.getRuns();
      },

      getRunDetail: async (params: unknown) => {
        const { runName } = params as { runName: string };
        return bridge.getRunDetail(runName);
      },

      getMessages: async (params: unknown) => {
        const { runName, thread, limit } = params as { runName: string; thread: string; limit?: number };
        return bridge.getMessages(runName, thread, limit);
      },

      getActivity: async (params: unknown) => {
        const { runName, limit } = params as { runName: string; limit?: number };
        return bridge.getActivity(runName, limit);
      },

      pauseRun: async (params: unknown) => {
        const { runName } = params as { runName: string };
        return bridge.pauseRun(runName);
      },

      resumeRun: async (params: unknown) => {
        const { runName, timeLimit } = params as { runName: string; timeLimit?: string };
        return bridge.resumeRun(runName, timeLimit);
      },

      deleteRun: async (params: unknown) => {
        const { runName } = params as { runName: string };
        return bridge.deleteRun(runName);
      },

      deliverRun: async (params: unknown) => {
        const { runName, branchName } = params as { runName: string; branchName?: string };
        return bridge.deliverRun(runName, branchName);
      },

      attachWorker: async (params: unknown) => {
        const { runName, workerName } = params as { runName: string; workerName: string };
        const { spawn } = await import("bun");
        try {
          const state = bridge.openState(runName);
          if (!state) {
            return { success: false, error: `Run '${runName}' not found` };
          }

          const worker = state.getWorker(workerName);
          state.close();

          if (!worker?.sessionId) {
            return { success: false, error: `Worker '${workerName}' has no active session` };
          }

          spawn(["tmux", "attach-session", "-t", worker.sessionId], {
            stdin: "inherit",
            stdout: "inherit",
            stderr: "inherit",
          });

          return { success: true };
        } catch (error) {
          return { success: false, error: `Failed to attach: ${(error as Error).message}` };
        }
      },

      sendMessage: async (params: unknown) => {
        const { runName, thread, message } = params as { runName: string; thread: string; message: string };
        return bridge.sendMessage(runName, thread, message);
      },

      createRun: async (params: unknown) => {
        const { runName, specPath, options } = params as { runName: string; specPath: string; options: unknown };
        // TODO: Implement run creation via bridge
        return { success: false, error: "Not implemented - use CLI: hirsel go" };
      },

      getDiff: async (params: unknown) => {
        const { runName } = params as { runName: string };
        return bridge.getDiff(runName);
      },

      markTaskDone: async (params: unknown) => {
        const { runName, taskId } = params as { runName: string; taskId: string };
        return bridge.markTaskDone(runName, taskId);
      },

      unclaimTask: async (params: unknown) => {
        const { runName, taskId } = params as { runName: string; taskId: string };
        return bridge.unclaimTask(runName, taskId);
      },

      addTask: async (params: unknown) => {
        const { runName, taskId, description, parentId, blockedBy } = params as {
          runName: string; taskId: string; description: string; parentId?: string; blockedBy?: string[];
        };
        return bridge.addTask(runName, taskId, description, parentId, blockedBy);
      },

      deleteTask: async (params: unknown) => {
        const { runName, taskId } = params as { runName: string; taskId: string };
        return bridge.deleteTask(runName, taskId);
      },

      reopenTask: async (params: unknown) => {
        const { runName, taskId } = params as { runName: string; taskId: string };
        return bridge.reopenTask(runName, taskId);
      },
    },
    messages: {
      log: (params: unknown) => {
        const { level, message } = params as { level: string; message: string };
        const logFn = level === 'error' ? console.error :
                      level === 'warn' ? console.warn :
                      level === 'debug' ? console.debug : console.log;
        logFn(`[UI:${level}]`, message);
      },
    },
  },
});

// Create main window
const mainWindow = new BrowserWindow({
  title: "Hirsel",
  url: "views://main/index.html",
  frame: { width: 1200, height: 800, x: 100, y: 100 },
  rpc: mainRPC,
});

console.log("Hirsel desktop app started");
