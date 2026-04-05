import { apiFetch, apiUrl, parseJson } from "@/lib/api/core";
import type {
  ChatMessage,
  ChatSendResponse,
  KnowledgeGraph,
  KnowledgeGraphNode,
  LiveUpdateEvent,
  Project,
  ProjectCreateProbe,
  ProjectPreparation,
  ProjectSurface,
  ScopeActivity,
  WorkspaceSnapshot,
} from "@/lib/api/types";

export async function listProjects(): Promise<Project[]> {
  const res = await apiFetch("/projects");
  return parseJson<Project[]>(res);
}

export async function getProject(projectId: number): Promise<Project> {
  const res = await apiFetch(`/projects/${projectId}`);
  return parseJson<Project>(res);
}

export async function probeProjectCreate(data: {
  repo_url: string;
  branch?: string;
}): Promise<ProjectCreateProbe> {
  const params = new URLSearchParams();
  params.set("repo_url", data.repo_url);
  if (data.branch?.trim()) {
    params.set("branch", data.branch.trim());
  }
  const res = await apiFetch(`/projects/probe?${params.toString()}`);
  return parseJson<ProjectCreateProbe>(res);
}

export async function getProjectPreparation(projectId: number): Promise<ProjectPreparation> {
  const res = await apiFetch(`/projects/${projectId}/preparation`);
  return parseJson<ProjectPreparation>(res);
}

export async function retryProjectPreparation(projectId: number): Promise<ProjectPreparation> {
  const res = await apiFetch(`/projects/${projectId}/preparation/retry`, {
    method: "POST",
  });
  return parseJson<ProjectPreparation>(res);
}

export async function getProjectActivity(projectId: number): Promise<ScopeActivity> {
  const res = await apiFetch(`/projects/${projectId}/activity`);
  return parseJson<ScopeActivity>(res);
}

export async function getProjectHistory(
  projectId: number,
  data: { limit?: number } = {},
): Promise<ChatMessage[]> {
  const params = new URLSearchParams();
  if (Number.isFinite(data.limit) && (data.limit ?? 0) > 0) {
    params.set("limit", String(data.limit));
  }
  const suffix = params.toString();
  const res = await apiFetch(
    suffix ? `/projects/${projectId}/history?${suffix}` : `/projects/${projectId}/history`,
  );
  return parseJson<ChatMessage[]>(res);
}

export async function getProjectSurface(projectId: number): Promise<ProjectSurface> {
  const res = await apiFetch(`/projects/${projectId}/surface`);
  return parseJson<ProjectSurface>(res);
}

export async function getWorkspaceSnapshot(
  projectId: number,
  data: { threadId?: string; librarian?: boolean } = {},
): Promise<WorkspaceSnapshot> {
  const params = new URLSearchParams();
  if (data.threadId?.trim()) {
    params.set("thread_id", data.threadId.trim());
  }
  if (data.librarian) {
    params.set("librarian", "true");
  }
  const suffix = params.toString();
  const res = await apiFetch(
    suffix
      ? `/projects/${projectId}/workspace-snapshot?${suffix}`
      : `/projects/${projectId}/workspace-snapshot`,
  );
  return parseJson<WorkspaceSnapshot>(res);
}

export function subscribeProjectEvents(
  projectId: number,
  onEvent: (event: LiveUpdateEvent) => void,
  onOpen?: () => void,
  onError?: () => void,
): () => void {
  const source = new EventSource(apiUrl(`/projects/${projectId}/events`));

  source.addEventListener("open", () => {
    onOpen?.();
  });

  source.addEventListener("update", (event) => {
    if (!(event instanceof MessageEvent)) return;
    try {
      onEvent(JSON.parse(event.data) as LiveUpdateEvent);
    } catch (error) {
      console.warn("Failed to parse live update event", error);
    }
  });

  source.addEventListener("error", () => {
    onError?.();
  });

  return () => {
    source.close();
  };
}

export async function getLibrarianActivity(projectId: number): Promise<ScopeActivity> {
  const res = await apiFetch(`/projects/${projectId}/librarian/activity`);
  return parseJson<ScopeActivity>(res);
}

export async function getLibrarianHistory(
  projectId: number,
  data: { limit?: number } = {},
): Promise<ChatMessage[]> {
  const params = new URLSearchParams();
  if (Number.isFinite(data.limit) && (data.limit ?? 0) > 0) {
    params.set("limit", String(data.limit));
  }
  const suffix = params.toString();
  const res = await apiFetch(
    suffix ? `/projects/${projectId}/librarian/history?${suffix}` : `/projects/${projectId}/librarian/history`,
  );
  return parseJson<ChatMessage[]>(res);
}

export async function sendLibrarianMessage(projectId: number, content: string): Promise<ChatSendResponse> {
  const res = await apiFetch(`/projects/${projectId}/librarian/chat/send`, {
    method: "POST",
    body: JSON.stringify({ content }),
  });
  return parseJson<ChatSendResponse>(res);
}

export async function stopLibrarianChat(projectId: number): Promise<void> {
  const res = await apiFetch(`/projects/${projectId}/librarian/chat/stop`, { method: "POST" });
  await parseJson<{ ok: true }>(res);
}

export async function triggerKnowledgeScan(projectId: number): Promise<void> {
  const res = await apiFetch(`/projects/${projectId}/knowledge-graph/scan`, {
    method: "POST",
  });
  await parseJson<{ ok: true }>(res);
}

export async function getKnowledgeGraph(projectId: number): Promise<KnowledgeGraph> {
  const res = await apiFetch(`/projects/${projectId}/knowledge-graph`);
  return parseJson<KnowledgeGraph>(res);
}

export async function resolveKnowledgeGraphNode(
  projectId: number,
  kind: string,
  nodeId: string,
): Promise<KnowledgeGraphNode | null> {
  const graph = await getKnowledgeGraph(projectId);
  return graph.nodes.find((node) => node.kind === kind && node.node_id === nodeId) ?? null;
}

export async function sendChatMessage(projectId: number, content: string): Promise<ChatSendResponse> {
  const res = await apiFetch(`/projects/${projectId}/chat/send`, {
    method: "POST",
    body: JSON.stringify({ content }),
  });
  return parseJson<ChatSendResponse>(res);
}

export async function stopChat(projectId: number): Promise<void> {
  const res = await apiFetch(`/projects/${projectId}/chat/stop`, { method: "POST" });
  await parseJson<{ ok: true }>(res);
}

export async function createProject(data: {
  name: string;
  repo_url: string;
  branch?: string;
  sandbox_image?: string;
}): Promise<Project> {
  const res = await apiFetch("/projects", {
    method: "POST",
    body: JSON.stringify(data),
  });
  return parseJson<Project>(res);
}

export async function deleteProject(projectId: number): Promise<void> {
  const res = await apiFetch(`/projects/${projectId}`, { method: "DELETE" });
  await parseJson<{ ok: true }>(res);
}

export async function saveProjectSettings(
  projectId: number,
  data: {
    name: string;
    description?: string | null;
    sandbox_image?: string;
  },
): Promise<Project> {
  const res = await apiFetch(`/projects/${projectId}/settings`, {
    method: "POST",
    body: JSON.stringify(data),
  });
  return parseJson<Project>(res);
}
