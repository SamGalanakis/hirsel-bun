/**
 * hirsel tasks <run> - List tasks
 *
 * Also provides task-add, task-delete, task-done, task-reopen, task-unclaim commands.
 */

import { isJsonOutput, jsonOutput, validateTaskId, ICONS } from "../index";
import { Task, TaskStatus } from "../../shared/types";
import { formatTaskStatus, dim, bold, colorize } from "../../shared/theme";
import { existsSync } from "fs";
import { join } from "path";
import { homedir } from "os";
import { Database } from "bun:sqlite";

// Get hirsel root directory
function getHirselRoot(): string {
  return process.env.HIRSEL_ROOT || join(homedir(), ".hirsel");
}

// Get database for a run
function getDb(runName: string): Database {
  const dbPath = join(getHirselRoot(), "runs", runName, "hirsel.db");
  if (!existsSync(dbPath)) {
    throw new Error(`Run '${runName}' not found`);
  }
  return new Database(dbPath);
}

// Get all tasks for a run
function getTasks(db: Database): Task[] {
  return db.query(`
    SELECT id, name, status, created_at, completed_at, claimed_by, claimed_at, tokens_used, parent_id, blocked_by
    FROM tasks
    ORDER BY created_at ASC
  `).all() as Task[];
}

// Format task for display
function formatTask(task: Task, indent: number = 0): string {
  const statusIcon = ICONS[task.status as keyof typeof ICONS] || ICONS.taskTodo;
  const prefix = "  ".repeat(indent);

  let statusColor: "yellow" | "green" | "brightBlack" = "brightBlack";
  if (task.status === "doing") statusColor = "yellow";
  else if (task.status === "done") statusColor = "green";

  const statusStr = colorize(statusIcon, statusColor);
  const name = task.status === "done" ? dim(task.name) : task.name;
  const id = dim(task.id.padEnd(20));
  const claimedBy = task.claimedBy ? dim(` (${task.claimedBy})`) : "";

  return `${prefix}${statusStr} ${id} ${name}${claimedBy}`;
}

// Main command handler - list tasks
export default async function tasks(args: string[]): Promise<void> {
  if (args.length === 0) {
    console.error(`${ICONS.error} Run name required`);
    console.error("Usage: hirsel tasks <run>");
    process.exit(1);
  }

  const runName = args[0];
  let db: Database;

  try {
    db = getDb(runName);
  } catch (error) {
    console.error(`${ICONS.error} ${(error as Error).message}`);
    process.exit(1);
  }

  const taskList = getTasks(db);
  db.close();

  if (isJsonOutput()) {
    jsonOutput({ tasks: taskList });
    return;
  }

  if (taskList.length === 0) {
    console.log(dim("No tasks"));
    return;
  }

  console.log(bold("\nTasks\n"));

  // Group by status
  const doing = taskList.filter((t) => t.status === "doing");
  const todo = taskList.filter((t) => t.status === "todo");
  const done = taskList.filter((t) => t.status === "done");

  // Print doing first
  for (const task of doing) {
    console.log(formatTask(task));
  }

  // Then todo
  for (const task of todo) {
    console.log(formatTask(task));
  }

  // Then done (at end, dimmed)
  for (const task of done) {
    console.log(formatTask(task));
  }

  console.log();
  console.log(dim(`${done.length}/${taskList.length} completed`));
}

// task-add command
export async function taskAddCommand(args: string[]): Promise<void> {
  if (args.length < 3) {
    console.error(`${ICONS.error} Usage: hirsel task-add <run> <id> <description>`);
    process.exit(1);
  }

  const [runName, taskId, ...descParts] = args;
  const description = descParts.join(" ");

  try {
    validateTaskId(taskId);
  } catch {
    process.exit(1);
  }

  let db: Database;
  try {
    db = getDb(runName);
  } catch (error) {
    console.error(`${ICONS.error} ${(error as Error).message}`);
    process.exit(1);
  }

  // Check if task already exists
  const existing = db.query("SELECT id FROM tasks WHERE id = ?").get(taskId);
  if (existing) {
    console.error(`${ICONS.error} Task '${taskId}' already exists`);
    db.close();
    process.exit(1);
  }

  // Add task
  db.run("INSERT INTO tasks (id, name, status) VALUES (?, ?, 'todo')", [taskId, description]);
  db.run("INSERT INTO history (action, detail) VALUES (?, ?)", ["task added", `${taskId}: ${description}`]);
  db.close();

  console.log(`${ICONS.done} Added task: ${taskId}`);
}

// task-delete command
export async function taskDeleteCommand(args: string[]): Promise<void> {
  if (args.length < 2) {
    console.error(`${ICONS.error} Usage: hirsel task-delete <run> <id>`);
    process.exit(1);
  }

  const [runName, taskId] = args;

  let db: Database;
  try {
    db = getDb(runName);
  } catch (error) {
    console.error(`${ICONS.error} ${(error as Error).message}`);
    process.exit(1);
  }

  // Check if task exists
  const existing = db.query("SELECT id, status, claimed_by FROM tasks WHERE id = ?").get(taskId) as {
    id: string;
    status: string;
    claimed_by: string | null;
  } | null;

  if (!existing) {
    console.error(`${ICONS.error} Task '${taskId}' not found`);
    db.close();
    process.exit(1);
  }

  if (existing.claimed_by && existing.status === "doing") {
    console.error(`${ICONS.error} Cannot delete task that is in progress`);
    db.close();
    process.exit(1);
  }

  // Delete task
  db.run("DELETE FROM tasks WHERE id = ?", [taskId]);
  db.run("INSERT INTO history (action, detail) VALUES (?, ?)", ["task deleted", taskId]);
  db.close();

  console.log(`${ICONS.done} Deleted task: ${taskId}`);
}

// task-done command
export async function taskDoneCommand(args: string[]): Promise<void> {
  if (args.length < 2) {
    console.error(`${ICONS.error} Usage: hirsel task-done <run> <id>`);
    process.exit(1);
  }

  const [runName, taskId] = args;

  let db: Database;
  try {
    db = getDb(runName);
  } catch (error) {
    console.error(`${ICONS.error} ${(error as Error).message}`);
    process.exit(1);
  }

  // Check if task exists
  const existing = db.query("SELECT id, status FROM tasks WHERE id = ?").get(taskId) as {
    id: string;
    status: string;
  } | null;

  if (!existing) {
    console.error(`${ICONS.error} Task '${taskId}' not found`);
    db.close();
    process.exit(1);
  }

  if (existing.status === "done") {
    console.log(dim(`Task '${taskId}' is already done`));
    db.close();
    return;
  }

  // Mark done
  const now = new Date().toISOString();
  db.run("UPDATE tasks SET status = 'done', completed_at = ? WHERE id = ?", [now, taskId]);
  db.run("INSERT INTO history (action, detail) VALUES (?, ?)", ["task completed", taskId]);
  db.close();

  console.log(`${ICONS.done} Completed task: ${taskId}`);
}

// task-reopen command
export async function taskReopenCommand(args: string[]): Promise<void> {
  if (args.length < 2) {
    console.error(`${ICONS.error} Usage: hirsel task-reopen <run> <id>`);
    process.exit(1);
  }

  const [runName, taskId] = args;

  let db: Database;
  try {
    db = getDb(runName);
  } catch (error) {
    console.error(`${ICONS.error} ${(error as Error).message}`);
    process.exit(1);
  }

  // Check if task exists
  const existing = db.query("SELECT id, status FROM tasks WHERE id = ?").get(taskId) as {
    id: string;
    status: string;
  } | null;

  if (!existing) {
    console.error(`${ICONS.error} Task '${taskId}' not found`);
    db.close();
    process.exit(1);
  }

  if (existing.status !== "done") {
    console.log(dim(`Task '${taskId}' is not completed`));
    db.close();
    return;
  }

  // Reopen
  db.run("UPDATE tasks SET status = 'todo', completed_at = NULL, claimed_by = NULL, claimed_at = NULL WHERE id = ?", [taskId]);
  db.run("INSERT INTO history (action, detail) VALUES (?, ?)", ["task reopened", taskId]);
  db.close();

  console.log(`${ICONS.done} Reopened task: ${taskId}`);
}

// task-unclaim command
export async function taskUnclaimCommand(args: string[]): Promise<void> {
  if (args.length < 2) {
    console.error(`${ICONS.error} Usage: hirsel task-unclaim <run> <id>`);
    process.exit(1);
  }

  const [runName, taskId] = args;

  let db: Database;
  try {
    db = getDb(runName);
  } catch (error) {
    console.error(`${ICONS.error} ${(error as Error).message}`);
    process.exit(1);
  }

  // Check if task exists
  const existing = db.query("SELECT id, status, claimed_by FROM tasks WHERE id = ?").get(taskId) as {
    id: string;
    status: string;
    claimed_by: string | null;
  } | null;

  if (!existing) {
    console.error(`${ICONS.error} Task '${taskId}' not found`);
    db.close();
    process.exit(1);
  }

  if (!existing.claimed_by) {
    console.log(dim(`Task '${taskId}' is not claimed`));
    db.close();
    return;
  }

  // Unclaim
  db.run("UPDATE tasks SET status = 'todo', claimed_by = NULL, claimed_at = NULL WHERE id = ?", [taskId]);
  db.run("INSERT INTO history (action, detail) VALUES (?, ?)", ["task unclaimed", taskId]);
  db.close();

  console.log(`${ICONS.done} Unclaimed task: ${taskId}`);
}
