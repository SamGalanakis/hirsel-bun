import { apiFetch, parseJson } from "@/lib/api/core";
import type {
  ChatMessage,
  ChatSendResponse,
  MergeResult,
  SpawnThreadRequest,
  SpawnedThread,
  ThreadDetail,
  ThreadInspection,
  ThreadSummary,
} from "@/lib/api/types";

export async function listThreads(projectId: number): Promise<ThreadSummary[]> {
  const res = await apiFetch(`/projects/${projectId}/threads`);
  return parseJson<ThreadSummary[]>(res);
}

export async function getThread(projectId: number, threadId: string): Promise<ThreadDetail> {
  const res = await apiFetch(`/projects/${projectId}/threads/${threadId}`);
  return parseJson<ThreadDetail>(res);
}

export async function getThreadHistory(
  projectId: number,
  threadId: string,
  data: { limit?: number } = {},
): Promise<ChatMessage[]> {
  const params = new URLSearchParams();
  if (Number.isFinite(data.limit) && (data.limit ?? 0) > 0) {
    params.set("limit", String(data.limit));
  }
  const suffix = params.toString();
  const res = await apiFetch(
    suffix
      ? `/projects/${projectId}/threads/${threadId}/history?${suffix}`
      : `/projects/${projectId}/threads/${threadId}/history`,
  );
  return parseJson<ChatMessage[]>(res);
}

export async function sendThreadMessage(
  projectId: number,
  threadId: string,
  content: string,
): Promise<ChatSendResponse> {
  const res = await apiFetch(`/projects/${projectId}/threads/${threadId}/chat/send`, {
    method: "POST",
    body: JSON.stringify({ content }),
  });
  return parseJson<ChatSendResponse>(res);
}

export async function stopThreadChat(projectId: number, threadId: string): Promise<void> {
  const res = await apiFetch(`/projects/${projectId}/threads/${threadId}/chat/stop`, {
    method: "POST",
  });
  await parseJson<{ ok: true }>(res);
}

export async function spawnThread(
  projectId: number,
  request: SpawnThreadRequest,
): Promise<SpawnedThread> {
  const res = await apiFetch(`/projects/${projectId}/spawn-thread`, {
    method: "POST",
    body: JSON.stringify(request),
  });
  return parseJson<SpawnedThread>(res);
}

export async function inspectThread(
  projectId: number,
  threadId: string,
): Promise<ThreadInspection> {
  const res = await apiFetch(`/projects/${projectId}/threads/${threadId}/inspect`);
  return parseJson<ThreadInspection>(res);
}

export async function mergeThread(projectId: number, threadId: string): Promise<MergeResult> {
  const res = await apiFetch(`/projects/${projectId}/threads/${threadId}/merge`, {
    method: "POST",
  });
  return parseJson<MergeResult>(res);
}

export async function mergeThreadRetry(
  projectId: number,
  threadId: string,
): Promise<MergeResult> {
  const res = await apiFetch(`/projects/${projectId}/threads/${threadId}/merge/retry`, {
    method: "POST",
  });
  return parseJson<MergeResult>(res);
}

export async function discardThread(projectId: number, threadId: string): Promise<void> {
  const res = await apiFetch(`/projects/${projectId}/threads/${threadId}/discard`, {
    method: "POST",
  });
  await parseJson<{ ok: true }>(res);
}
