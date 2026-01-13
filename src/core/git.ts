import { $ } from "bun";
import { existsSync, cpSync, rmSync, mkdirSync } from "fs";
import { join } from "path";
import type { BranchInfo, GitInfo } from "../shared/types";

export class GitError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "GitError";
  }
}

async function git(args: string[], cwd: string): Promise<string> {
  const result = await $`git ${args}`.cwd(cwd).quiet().nothrow();
  if (result.exitCode !== 0) {
    throw new GitError(result.stderr.toString());
  }
  return result.stdout.toString().trim();
}

export async function getRepoRoot(path: string): Promise<string> {
  return git(["rev-parse", "--show-toplevel"], path);
}

export async function isGitRepo(path: string): Promise<boolean> {
  try {
    await git(["rev-parse", "--git-dir"], path);
    return true;
  } catch {
    return false;
  }
}

export async function getCurrentBranch(path: string): Promise<string> {
  return git(["rev-parse", "--abbrev-ref", "HEAD"], path);
}

export async function branchExists(path: string, branchName: string): Promise<boolean> {
  try {
    await git(["rev-parse", "--verify", branchName], path);
    return true;
  } catch {
    return false;
  }
}

export async function createWorktree(projectPath: string, workDir: string): Promise<void> {
  if (existsSync(workDir)) {
    rmSync(workDir, { recursive: true });
  }
  cpSync(projectPath, workDir, {
    recursive: true,
    filter: (src) => !src.includes(".git"),
  });
  await git(["init"], workDir);
  await git(["checkout", "-b", "staging"], workDir);
  await git(["add", "."], workDir);
  await git(["commit", "-m", "Initial commit"], workDir);
}

export async function createWorkspace(projectPath: string, workDir: string): Promise<void> {
  const stagingDir = join(workDir, "staging");
  if (existsSync(workDir)) {
    rmSync(workDir, { recursive: true });
  }
  mkdirSync(workDir, { recursive: true });
  cpSync(projectPath, stagingDir, { recursive: true });

  try {
    await git(["add", "."], stagingDir);
    await git(["commit", "-m", "hirsel: snapshot uncommitted changes"], stagingDir);
  } catch {
    // No changes
  }

  const currentBranch = await getCurrentBranch(stagingDir);
  if (currentBranch !== "staging") {
    await git(["checkout", "-b", "staging"], stagingDir);
  }

  const branches = (await git(["branch", "--format=%(refname:short)"], stagingDir))
    .split("\n")
    .filter((b) => b && b !== "staging");
  
  for (const branch of branches) {
    try {
      await git(["branch", "-D", branch], stagingDir);
    } catch {
      // Ignore
    }
  }
  await git(["config", "receive.denyCurrentBranch", "updateInstead"], stagingDir);
}

export async function createWorkerClone(workDir: string, workerName: string): Promise<string> {
  const stagingDir = join(workDir, "staging");
  const workerDir = join(workDir, workerName);
  if (existsSync(workerDir)) {
    rmSync(workerDir, { recursive: true });
  }
  cpSync(stagingDir, workerDir, {
    recursive: true,
    filter: (src) => !src.includes(".git"),
  });
  await git(["init"], workerDir);
  await git(["remote", "add", "origin", stagingDir], workerDir);
  await git(["fetch", "origin"], workerDir);
  await git(["checkout", "-b", "staging", "origin/staging"], workerDir);
  return workerDir;
}

export async function createTaskBranch(path: string, taskId: string): Promise<void> {
  const branchName = "task/" + taskId;
  await git(["checkout", "-b", branchName], path);
}

export async function mergeTaskToStaging(path: string, taskId: string): Promise<void> {
  const branchName = "task/" + taskId;

  try {
    await git(["add", "."], path);
    await git(["commit", "-m", "WIP: " + taskId], path);
  } catch {
    // No changes
  }

  await git(["checkout", "staging"], path);

  try {
    await git(["merge", "--no-edit", branchName], path);
  } catch (e) {
    const status = await git(["status", "--porcelain"], path);
    const conflicts = status
      .split("\n")
      .filter((line) => line.startsWith("UU") || line.startsWith("AA"))
      .map((line) => line.slice(3));
    if (conflicts.length > 0) {
      throw new GitError("Merge conflict in files: " + conflicts.join(", "));
    }
    throw e;
  }
  await git(["branch", "-d", branchName], path);
}

export async function listUnmergedBranches(path: string): Promise<string[]> {
  try {
    const branches = (await git(["branch", "--format=%(refname:short)"], path)).split("\n");
    const unmerged: string[] = [];
    for (const branch of branches) {
      if (!branch.startsWith("task/")) continue;
      try {
        const mergeBase = await git(["merge-base", "staging", branch], path);
        const branchTip = await git(["rev-parse", branch], path);
        if (mergeBase !== branchTip) {
          unmerged.push(branch);
        }
      } catch {
        unmerged.push(branch);
      }
    }
    return unmerged;
  } catch {
    return [];
  }
}

export async function getDiff(projectPath: string, workDir: string): Promise<string> {
  const remoteName = "hirsel_work";
  try {
    await git(["remote", "add", remoteName, workDir], projectPath);
    await git(["fetch", remoteName], projectPath);
    const diff = await git(["diff", "HEAD", remoteName + "/staging"], projectPath);
    return diff;
  } finally {
    try {
      await git(["remote", "remove", remoteName], projectPath);
    } catch {
      // Ignore
    }
  }
}

export async function getDiffStat(projectPath: string, workDir: string): Promise<string> {
  const remoteName = "hirsel_work";
  try {
    await git(["remote", "add", remoteName, workDir], projectPath);
    await git(["fetch", remoteName], projectPath);
    const stat = await git(["diff", "--stat", "HEAD", remoteName + "/staging"], projectPath);
    return stat;
  } finally {
    try {
      await git(["remote", "remove", remoteName], projectPath);
    } catch {
      // Ignore
    }
  }
}

export async function pushStagingAsBranch(workDir: string, projectPath: string, branchName: string): Promise<void> {
  const remoteName = "hirsel_project";
  try {
    await git(["remote", "add", remoteName, projectPath], workDir);
    await git(["push", remoteName, "staging:" + branchName], workDir);
  } finally {
    try {
      await git(["remote", "remove", remoteName], workDir);
    } catch {
      // Ignore
    }
  }
}

export async function getBranchHistory(path: string): Promise<BranchInfo[]> {
  try {
    const branches = (await git(["branch", "--format=%(refname:short)"], path)).split("\n");
    const currentBranch = await getCurrentBranch(path);
    const result: BranchInfo[] = [];

    for (const branch of branches) {
      if (!branch) continue;
      let isMerged = false;
      try {
        const mergeBase = await git(["merge-base", "staging", branch], path);
        const branchTip = await git(["rev-parse", branch], path);
        isMerged = mergeBase === branchTip;
      } catch {
        // Not merged
      }

      let commitCount = 0;
      try {
        const count = await git(["rev-list", "--count", branch], path);
        commitCount = parseInt(count, 10);
      } catch {
        // Ignore
      }

      result.push({
        name: branch,
        isCurrent: branch === currentBranch,
        isMerged,
        commitCount,
      });
    }
    return result;
  } catch {
    return [];
  }
}

export async function getBranchGraph(path: string): Promise<string> {
  try {
    return git(["log", "--graph", "--simplify-by-decoration", "--oneline", "--all", "-20"], path);
  } catch {
    return "";
  }
}

export async function getGitInfo(workDir: string): Promise<GitInfo> {
  const [currentBranch, unmergedBranches, branchHistory, branchGraph] = await Promise.all([
    getCurrentBranch(workDir).catch(() => "staging"),
    listUnmergedBranches(workDir).catch(() => []),
    getBranchHistory(workDir).catch(() => []),
    getBranchGraph(workDir).catch(() => ""),
  ]);
  return { currentBranch, unmergedBranches, branchHistory, branchGraph };
}
