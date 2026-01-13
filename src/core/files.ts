/**
 * File system utilities for hirsel runs.
 *
 * Manages the run directory structure:
 * - spec.md, eval.md, tasks.md
 * - tasks/ directory for task details
 * - chats/ directory for message threads
 * - work/ directory for git worktrees
 * - tmp/ directory for logs
 */

import { existsSync, mkdirSync, readFileSync, writeFileSync, appendFileSync } from "fs";
import { join } from "path";
import type { Task } from "../shared/types";

/** Regex for parsing task table rows: | id | STATUS | worker | name | */
const TASK_ROW_RE = /^\| ([a-z_][a-z0-9_]*) \| (TODO|DOING|DONE) \| ([^|]*)\| (.+) \|$/;

/**
 * Files helper for a hirsel run.
 *
 * Provides paths and utilities for managing run files.
 */
export class Files {
  readonly runDir: string;

  constructor(runDir: string) {
    this.runDir = runDir;
  }

  /** Path to spec.md */
  get spec(): string {
    return join(this.runDir, "spec.md");
  }

  /** Path to tasks.md */
  get tasksMd(): string {
    return join(this.runDir, "tasks.md");
  }

  /** Path to tasks directory */
  get tasksDir(): string {
    return join(this.runDir, "tasks");
  }

  /** Path to log.md */
  get log(): string {
    return join(this.runDir, "log.md");
  }

  /** Path to eval.md */
  get evalSpec(): string {
    return join(this.runDir, "eval.md");
  }

  /** Path to eval log */
  get evalLog(): string {
    return join(this.runDir, "tmp", "eval_log.md");
  }

  /** Path to work directory */
  get work(): string {
    return join(this.runDir, "work");
  }

  /** Path to chats directory */
  get chatsDir(): string {
    return join(this.runDir, "chats");
  }

  /** Path to tmp directory */
  get tmpDir(): string {
    return join(this.runDir, "tmp");
  }

  /** Get worker log path */
  workerLog(workerName: string): string {
    return join(this.tmpDir, `${workerName}.log`);
  }

  /** Get chat file path */
  chatFile(name: string): string {
    return join(this.chatsDir, `${name}.md`);
  }

  /** Get task detail file path */
  taskDetail(taskId: string): string {
    return join(this.tasksDir, `${taskId}.md`);
  }

  /** Initialize run directories */
  initDirs(): void {
    mkdirSync(this.runDir, { recursive: true });
    mkdirSync(this.tasksDir, { recursive: true });
    mkdirSync(this.chatsDir, { recursive: true });
    mkdirSync(this.tmpDir, { recursive: true });
  }

  /** Append entry to log.md */
  appendLog(message: string): void {
    const timestamp = new Date().toLocaleTimeString("en-US", {
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    });
    appendFileSync(this.log, `${timestamp} ${message}\n`);
  }

  /** Initialize tasks.md with header */
  initTasksMd(): void {
    if (!existsSync(this.tasksMd)) {
      const header = "# Tasks\n\n| ID | Status | Worker | Name |\n|----|--------|--------|------|\n";
      writeFileSync(this.tasksMd, header);
    }
  }

  /** Render tasks as markdown table */
  renderTasksTable(tasks: Array<{ id: string; status: string; claimed_by?: string | null; name: string }>): string {
    const lines = [
      "# Tasks",
      "",
      "| ID | Status | Worker | Name |",
      "|----|--------|--------|------|",
    ];
    for (const t of tasks) {
      const worker = t.claimed_by || "";
      lines.push(`| ${t.id} | ${t.status.toUpperCase()} | ${worker} | ${t.name} |`);
    }
    lines.push("");
    return lines.join("\n");
  }

  /** Update tasks.md with current tasks */
  updateTasksMd(tasks: Array<{ id: string; status: string; claimed_by?: string | null; name: string }>): void {
    writeFileSync(this.tasksMd, this.renderTasksTable(tasks));
  }

  /** Create task detail file if it doesn't exist */
  createTaskDetail(taskId: string, name: string): void {
    const detailPath = this.taskDetail(taskId);
    if (!existsSync(detailPath)) {
      writeFileSync(detailPath, `# ${name}\n\n`);
    }
  }

  /** Write task detail content */
  writeTaskDetail(taskId: string, content: string): void {
    writeFileSync(this.taskDetail(taskId), content);
  }

  /** Parse tasks.md and return task list */
  parseTasksMd(): Array<{
    id: string;
    status: string;
    claimed_by: string | null;
    name: string;
  }> {
    if (!existsSync(this.tasksMd)) {
      return [];
    }

    const content = readFileSync(this.tasksMd, "utf-8");
    const tasks: Array<{
      id: string;
      status: string;
      claimed_by: string | null;
      name: string;
    }> = [];

    for (const line of content.split("\n")) {
      const match = line.trim().match(TASK_ROW_RE);
      if (match) {
        tasks.push({
          id: match[1],
          status: match[2].toLowerCase(),
          claimed_by: match[3].trim() || null,
          name: match[4].trim(),
        });
      }
    }

    return tasks;
  }

  /** Ensure all tasks have detail files */
  ensureTaskDetails(): void {
    const tasks = this.parseTasksMd();
    for (const task of tasks) {
      const detailPath = this.taskDetail(task.id);
      if (!existsSync(detailPath)) {
        writeFileSync(detailPath, `# ${task.name}\n\n`);
      }
    }
  }

  /** Check if spec file exists */
  hasSpec(): boolean {
    return existsSync(this.spec);
  }

  /** Check if eval spec file exists */
  hasEvalSpec(): boolean {
    return existsSync(this.evalSpec);
  }

  /** Read spec content */
  readSpec(): string {
    if (!existsSync(this.spec)) {
      return "";
    }
    return readFileSync(this.spec, "utf-8");
  }

  /** Read eval spec content */
  readEvalSpec(): string {
    if (!existsSync(this.evalSpec)) {
      return "";
    }
    return readFileSync(this.evalSpec, "utf-8");
  }
}

/** Get Files instance for a run directory */
export function getFiles(runDir: string): Files {
  return new Files(runDir);
}
