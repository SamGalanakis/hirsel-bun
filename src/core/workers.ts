/**
 * Worker name allocation for hirsel
 * Port of Python reference implementation (workers.py)
 *
 * Workers are named after Greek heroes from mythology.
 */

import { Database } from "bun:sqlite";
import { existsSync, readdirSync, statSync } from "fs";
import { join } from "path";
import { config } from "./config";

// =============================================================================
// Worker Names - Greek Heroes
// =============================================================================

/** All available worker names */
export const WORKER_NAMES = [
  "achilles",
  "hector",
  "odysseus",
  "ajax",
  "patroclus",
  "agamemnon",
  "menelaus",
  "diomedes",
  "nestor",
  "priam",
  "paris",
  "aeneas",
  "heracles",
  "theseus",
  "perseus",
  "jason",
  "orpheus",
  "bellerophon",
  "cadmus",
  "peleus",
  "telamon",
  "tydeus",
  "amphiaraus",
  "capaneus",
  "hippomedon",
  "parthenopaeus",
  "polynices",
  "eteocles",
  "adrastus",
  "antilochus",
  "philoctetes",
  "idomeneus",
  "meriones",
  "eurypylus",
  "machaon",
  "podalirius",
  "protesilaus",
  "palamedes",
  "calchas",
  "stentor",
  "automedon",
  "phoenix",
  "deiphobus",
  "helenus",
  "polydamas",
  "sarpedon",
  "glaucus",
  "memnon",
  "penthesilea",
  "pyrrhus",
] as const;

export type WorkerName = (typeof WORKER_NAMES)[number];

// =============================================================================
// Name Allocation
// =============================================================================

/**
 * Get all worker names currently in use across all active runs.
 * A run is considered active if its status is not 'done' or 'merged'.
 */
export function getUsedNames(): Set<string> {
  const runsDir = config.runsDir;
  if (!existsSync(runsDir)) {
    return new Set();
  }

  const used = new Set<string>();

  try {
    const entries = readdirSync(runsDir);

    for (const entry of entries) {
      const runDir = join(runsDir, entry);

      // Skip non-directories
      try {
        if (!statSync(runDir).isDirectory()) continue;
      } catch {
        continue;
      }

      const dbPath = join(runDir, "hirsel.db");
      if (!existsSync(dbPath)) continue;

      try {
        const db = new Database(dbPath, { readonly: true });

        // Get workers from this run
        const workers = db
          .query("SELECT name FROM workers")
          .all() as { name: string }[];

        // Check if run is still active
        const state = db
          .query("SELECT status FROM state WHERE id = 1")
          .get() as { status: string } | undefined;

        const isActive = state && !["done", "merged"].includes(state.status);

        if (isActive) {
          for (const worker of workers) {
            used.add(worker.name);
          }
        }

        db.close();
      } catch (e) {
        // Ignore errors reading individual run databases
        console.debug(`Could not read workers from ${dbPath}:`, e);
      }
    }
  } catch (e) {
    console.error("Failed to scan runs directory:", e);
  }

  return used;
}

/**
 * Get a single available worker name.
 *
 * @param exclude Additional names to exclude beyond currently used ones
 * @returns A randomly selected available worker name
 */
export function getAvailableName(exclude?: Set<string>): WorkerName {
  const used = getUsedNames();

  if (exclude) {
    for (const name of exclude) {
      used.add(name);
    }
  }

  const available = WORKER_NAMES.filter((name) => !used.has(name));

  if (available.length === 0) {
    // All names used, fall back to random (shouldn't happen with 50 names)
    console.warn("All worker names in use, recycling a name");
    return WORKER_NAMES[Math.floor(Math.random() * WORKER_NAMES.length)];
  }

  return available[Math.floor(Math.random() * available.length)];
}

/**
 * Get multiple available worker names.
 *
 * @param count Number of names to allocate
 * @returns Array of randomly selected available worker names
 */
export function getAvailableNames(count: number): WorkerName[] {
  const used = getUsedNames();
  const available = WORKER_NAMES.filter((name) => !used.has(name));

  if (available.length < count) {
    // Not enough unique names, use what we have
    console.warn(
      `Only ${available.length} worker names available, requested ${count}`
    );
    return available.length > 0
      ? available.slice(0, count)
      : WORKER_NAMES.slice(0, count);
  }

  // Shuffle and take first 'count' names
  const shuffled = [...available].sort(() => Math.random() - 0.5);
  return shuffled.slice(0, count);
}

/**
 * Check if a name is a valid worker name.
 */
export function isValidWorkerName(name: string): name is WorkerName {
  return (WORKER_NAMES as readonly string[]).includes(name);
}

/**
 * Get all worker names (for display/autocomplete).
 */
export function getAllWorkerNames(): readonly WorkerName[] {
  return WORKER_NAMES;
}
