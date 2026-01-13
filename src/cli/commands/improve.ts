/**
 * hirsel improve - Update project memory from learnings
 *
 * Processes learnings from completed runs to improve project context.
 */

import { ICONS } from "../index";
import { dim, bold } from "../../shared/theme";

// Main command handler
export default async function improve(args: string[]): Promise<void> {
  console.log(bold("\nImprove Project Memory\n"));
  console.log(dim("This command processes learnings from runs to improve"));
  console.log(dim("the project's CLAUDE.md or similar context files."));
  console.log();
  console.log(dim("Usage:"));
  console.log(dim("  hirsel improve <run>       Process learnings from a run"));
  console.log(dim("  hirsel improve --all       Process all unprocessed learnings"));
  console.log();
  console.log(dim("Note: This command requires the worker system (Phase 3)."));
}
