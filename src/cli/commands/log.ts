/**
 * hirsel log <run> - View activity log
 *
 * Options:
 *   -f, --follow     Follow log updates (like tail -f)
 */

import { isJsonOutput, jsonOutput, ICONS } from "../index";
import { dim, bold, colorize } from "../../shared/theme";
import { existsSync } from "fs";
import { join } from "path";
import { homedir } from "os";
import { Database } from "bun:sqlite";

// Get hirsel root directory
function getHirselRoot(): string {
  return process.env.HIRSEL_ROOT || join(homedir(), ".hirsel");
}

// Parse command arguments
interface LogOptions {
  runName: string;
  follow: boolean;
}

function parseLogArgs(args: string[]): LogOptions {
  const options: LogOptions = {
    runName: "",
    follow: false,
  };

  for (const arg of args) {
    if (arg === "-f" || arg === "--follow") {
      options.follow = true;
    } else if (!arg.startsWith("-") && !options.runName) {
      options.runName = arg;
    }
  }

  return options;
}

// Get history entries
interface HistoryEntry {
  id: number;
  timestamp: string;
  action: string;
  detail: string | null;
}

function getHistory(db: Database, limit: number = 50, afterId: number = 0): HistoryEntry[] {
  return db.query(`
    SELECT id, timestamp, action, detail
    FROM history
    WHERE id > ?
    ORDER BY timestamp DESC
    LIMIT ?
  `).all(afterId, limit) as HistoryEntry[];
}

// Format history entry
function formatEntry(entry: HistoryEntry): string {
  const time = entry.timestamp.substring(11, 19);
  const detail = entry.detail ? ` ${entry.detail}` : "";

  // Color based on action type
  let actionColor: "yellow" | "green" | "red" | "cyan" | "brightBlack" = "brightBlack";
  if (entry.action.includes("started") || entry.action.includes("resumed")) {
    actionColor = "green";
  } else if (entry.action.includes("paused") || entry.action.includes("stopped")) {
    actionColor = "yellow";
  } else if (entry.action.includes("error") || entry.action.includes("failed")) {
    actionColor = "red";
  } else if (entry.action.includes("task")) {
    actionColor = "cyan";
  }

  return `${dim(time)} ${colorize(entry.action, actionColor)}${dim(detail)}`;
}

// Main command handler
export default async function log(args: string[]): Promise<void> {
  const options = parseLogArgs(args);

  if (!options.runName) {
    console.error(`${ICONS.error} Run name required`);
    console.error("Usage: hirsel log <run> [-f]");
    process.exit(1);
  }

  const runName = options.runName;
  const dbPath = join(getHirselRoot(), "runs", runName, "hirsel.db");

  if (!existsSync(dbPath)) {
    console.error(`${ICONS.error} Run '${runName}' not found`);
    process.exit(1);
  }

  const db = new Database(dbPath, { readonly: true });

  if (isJsonOutput() && !options.follow) {
    const history = getHistory(db, 100);
    db.close();
    jsonOutput({ history: history.reverse() });
    return;
  }

  console.log(bold(`\nActivity Log: ${runName}\n`));

  if (options.follow) {
    // Follow mode
    let lastId = 0;
    const history = getHistory(db, 20);
    for (const entry of history.reverse()) {
      console.log(formatEntry(entry));
      lastId = Math.max(lastId, entry.id);
    }

    console.log(dim("\n--- Following (Ctrl+C to exit) ---\n"));

    // Poll for new entries
    while (true) {
      await Bun.sleep(1000);
      const newEntries = getHistory(db, 10, lastId);
      for (const entry of newEntries.reverse()) {
        console.log(formatEntry(entry));
        lastId = Math.max(lastId, entry.id);
      }
    }
  } else {
    // One-shot mode
    const history = getHistory(db, 50);
    db.close();

    if (history.length === 0) {
      console.log(dim("No activity yet"));
      return;
    }

    for (const entry of history.reverse()) {
      console.log(formatEntry(entry));
    }
  }
}
