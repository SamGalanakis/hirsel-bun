import { apiFetch, apiUrl, parseJson } from "@/lib/api/core";
import type {
  WorkspaceCompletionEntry,
  WorkspaceDiffFile,
  WorkspaceDiffSummary,
  WorkspaceFile,
  WorkspaceRoot,
  WorkspaceSearchResponse,
  WorkspaceTree,
} from "@/lib/api/types";

export async function listWorkspaceRoots(projectId: number): Promise<WorkspaceRoot[]> {
  const res = await apiFetch(`/projects/${projectId}/workspace/roots`);
  return parseJson<WorkspaceRoot[]>(res);
}

export async function listWorkspaceTree(
  projectId: number,
  data: {
    rootId: string;
    path?: string;
    signal?: AbortSignal;
  },
): Promise<WorkspaceTree> {
  const params = new URLSearchParams();
  params.set("root_id", data.rootId);
  if (data.path?.trim()) {
    params.set("path", data.path.trim());
  }
  const res = await apiFetch(`/projects/${projectId}/workspace/tree?${params.toString()}`, {
    signal: data.signal,
  });
  return parseJson<WorkspaceTree>(res);
}

export async function getWorkspaceFile(
  projectId: number,
  data: {
    rootId: string;
    path: string;
    lineStart?: number;
    lineEnd?: number;
    signal?: AbortSignal;
  },
): Promise<WorkspaceFile> {
  const params = new URLSearchParams();
  params.set("root_id", data.rootId);
  params.set("path", data.path);
  if (Number.isFinite(data.lineStart) && (data.lineStart ?? 0) > 0) {
    params.set("line_start", String(data.lineStart));
  }
  if (Number.isFinite(data.lineEnd) && (data.lineEnd ?? 0) > 0) {
    params.set("line_end", String(data.lineEnd));
  }
  const res = await apiFetch(`/projects/${projectId}/workspace/file?${params.toString()}`, {
    signal: data.signal,
  });
  return parseJson<WorkspaceFile>(res);
}

export async function saveWorkspaceFile(
  projectId: number,
  data: {
    rootId: string;
    path: string;
    content: string;
  },
): Promise<void> {
  const res = await apiFetch(`/projects/${projectId}/workspace/file`, {
    method: "PUT",
    body: JSON.stringify({
      rootId: data.rootId,
      path: data.path,
      content: data.content,
    }),
  });
  await parseJson<{ ok: true }>(res);
}

export async function uploadWorkspaceFiles(
  projectId: number,
  data: {
    rootId: string;
    path?: string;
    files: File[];
  },
): Promise<void> {
  const body = new FormData();
  body.set("root_id", data.rootId);
  if (data.path?.trim()) {
    body.set("path", data.path.trim());
  }
  for (const file of data.files) {
    body.append("files", file, file.name);
  }
  const res = await apiFetch(`/projects/${projectId}/workspace/upload`, {
    method: "POST",
    body,
  });
  await parseJson<{ ok: true }>(res);
}

export async function searchWorkspace(
  projectId: number,
  data: {
    query: string;
    rootId?: string;
    signal?: AbortSignal;
  },
): Promise<WorkspaceSearchResponse> {
  const params = new URLSearchParams();
  params.set("q", data.query);
  if (data.rootId?.trim()) {
    params.set("root_id", data.rootId.trim());
  }
  const res = await apiFetch(`/projects/${projectId}/workspace/search?${params.toString()}`, {
    signal: data.signal,
  });
  return parseJson<WorkspaceSearchResponse>(res);
}

export async function completeWorkspacePath(
  projectId: number,
  data: {
    rootId: string;
    prefix?: string;
    signal?: AbortSignal;
  },
): Promise<WorkspaceCompletionEntry[]> {
  const params = new URLSearchParams();
  params.set("root_id", data.rootId);
  if (data.prefix?.trim()) {
    params.set("prefix", data.prefix.trim());
  }
  const res = await apiFetch(`/projects/${projectId}/workspace/complete?${params.toString()}`, {
    signal: data.signal,
  });
  return parseJson<WorkspaceCompletionEntry[]>(res);
}

export async function getWorkspaceDiff(
  projectId: number,
  data: {
    leftRootId: string;
    rightRootId: string;
    signal?: AbortSignal;
  },
): Promise<WorkspaceDiffSummary> {
  const params = new URLSearchParams();
  params.set("left_root_id", data.leftRootId);
  params.set("right_root_id", data.rightRootId);
  const res = await apiFetch(`/projects/${projectId}/workspace/diff?${params.toString()}`, {
    signal: data.signal,
  });
  return parseJson<WorkspaceDiffSummary>(res);
}

export async function getWorkspaceDiffFile(
  projectId: number,
  data: {
    leftRootId: string;
    rightRootId: string;
    path: string;
    signal?: AbortSignal;
  },
): Promise<WorkspaceDiffFile> {
  const params = new URLSearchParams();
  params.set("left_root_id", data.leftRootId);
  params.set("right_root_id", data.rightRootId);
  params.set("path", data.path);
  const res = await apiFetch(`/projects/${projectId}/workspace/diff/file?${params.toString()}`, {
    signal: data.signal,
  });
  return parseJson<WorkspaceDiffFile>(res);
}

export function buildWorkspaceDownloadUrl(
  projectId: number,
  data: {
    rootId: string;
    path: string;
  },
): string {
  const params = new URLSearchParams();
  params.set("root_id", data.rootId);
  params.set("path", data.path);
  return apiUrl(`/projects/${projectId}/workspace/download?${params.toString()}`);
}
