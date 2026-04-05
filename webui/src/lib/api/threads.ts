import { apiFetch, parseJson } from "@/lib/api/core";
import type { ChatMessage, ChatSendResponse, ThreadDetail, ThreadSummary } from "@/lib/api/types";

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
