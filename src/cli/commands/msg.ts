/**
 * hirsel msg <run> <message> - Send message to run
 *
 * Sends a message to the user thread that workers can see.
 */

import { ICONS } from "../index";
import { dim, bold } from "../../shared/theme";
import { existsSync } from "fs";
import { join } from "path";
import { homedir } from "os";
import { Database } from "bun:sqlite";

// Get hirsel root directory
function getHirselRoot(): string {
  return process.env.HIRSEL_ROOT || join(homedir(), ".hirsel");
}

// Main command handler
export default async function msg(args: string[]): Promise<void> {
  if (args.length < 2) {
    console.error(`${ICONS.error} Run name and message required`);
    console.error("Usage: hirsel msg <run> <message>");
    process.exit(1);
  }

  const runName = args[0];
  const message = args.slice(1).join(" ");

  const dbPath = join(getHirselRoot(), "runs", runName, "hirsel.db");

  if (!existsSync(dbPath)) {
    console.error(`${ICONS.error} Run '${runName}' not found`);
    process.exit(1);
  }

  const db = new Database(dbPath);

  // Add message to user thread
  const now = new Date().toISOString();
  db.run(`
    INSERT INTO messages (thread, sender, content, timestamp)
    VALUES ('user', 'user', ?, ?)
  `, [message, now]);

  // Add history entry
  db.run(`INSERT INTO history (action, detail) VALUES (?, ?)`, [
    "message sent",
    message.substring(0, 50) + (message.length > 50 ? "..." : ""),
  ]);

  db.close();

  console.log(`${ICONS.done} Message sent to ${bold(runName)}`);
}
