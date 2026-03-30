// Hirsel API client — all calls to the backend

export class ApiError extends Error {
  status: number;
  constructor(message: string, status: number) {
    super(message);
    this.status = status;
  }
}

async function apiFetch(path: string, init: RequestInit = {}): Promise<Response> {
  const headers = new Headers(init.headers || {});
  if (init.body && !headers.has("content-type")) {
    headers.set("content-type", "application/json");
  }
  // Session cookie handles auth
  return fetch(`/api${path}`, { ...init, headers, credentials: "same-origin" });
}

async function parseJson<T>(res: Response): Promise<T> {
  const body = await res.json().catch(() => ({}));
  if (!res.ok) {
    const message = typeof body?.message === "string" ? body.message : res.statusText;
    throw new ApiError(message || `Request failed (${res.status})`, res.status);
  }
  return body as T;
}

// ── Types ──

export interface Project {
  id: number;
  name: string;
  description: string | null;
  repo_url: string;
  branch: string;
  sandbox_image: string | null;
  created_at: string;
}

export interface ShepherdThread {
  id: string;
  project_id: number;
  title: string;
  objective: string;
  summary: string;
  status: string;
  workspace_path: string | null;
  checkout_name: string | null;
  created_at: string;
  updated_at: string;
  last_activity_at: string;
}

export interface ChatMessage {
  id: number;
  role: string;
  chunks_json: string;
  timestamp: string;
}

export interface LiveTurn {
  chunks_json: string;
  status: string;
  updated_at: string;
}

export interface ScopeActivity {
  session: { status: string; last_error: string | null } | null;
  live_turn: LiveTurn | null;
  has_active_turn: boolean;
}

export interface ThreadPanelState {
  thread: ShepherdThread;
  history: ChatMessage[];
  activity: ScopeActivity;
}

export interface ProjectPageData {
  project: Project;
  projects: Project[];
  threads: ThreadPanelState[];
  history: ChatMessage[];
  activity: ScopeActivity;
  has_queued: boolean;
  focus_html: string | null;
  focus_source: string | null;
}

export interface ThreadPageData {
  project: Project;
  thread: ShepherdThread;
  history: ChatMessage[];
  activity: ScopeActivity;
  has_queued: boolean;
}

// ── API Functions ──

export async function listProjects(): Promise<Project[]> {
  const res = await apiFetch("/projects");
  return parseJson<Project[]>(res);
}

export async function getProjectPage(projectId: number): Promise<ProjectPageData> {
  const res = await apiFetch(`/projects/${projectId}/page`);
  return parseJson<ProjectPageData>(res);
}

export async function getThreadPage(
  projectId: number,
  threadId: string,
): Promise<ThreadPageData> {
  const res = await apiFetch(`/projects/${projectId}/threads/${threadId}/page`);
  return parseJson<ThreadPageData>(res);
}

export async function sendChatMessage(
  projectId: number,
  content: string,
): Promise<void> {
  const res = await apiFetch(`/projects/${projectId}/chat/send`, {
    method: "POST",
    body: JSON.stringify({ content }),
  });
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new ApiError(body.message || "Failed to send message", res.status);
  }
}

export async function sendThreadMessage(
  projectId: number,
  threadId: string,
  content: string,
): Promise<void> {
  const res = await apiFetch(`/projects/${projectId}/threads/${threadId}/chat/send`, {
    method: "POST",
    body: JSON.stringify({ content }),
  });
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new ApiError(body.message || "Failed to send message", res.status);
  }
}

export async function stopChat(projectId: number): Promise<void> {
  await apiFetch(`/projects/${projectId}/chat/stop`, { method: "POST" });
}

export async function stopThreadChat(
  projectId: number,
  threadId: string,
): Promise<void> {
  await apiFetch(`/projects/${projectId}/threads/${threadId}/chat/stop`, {
    method: "POST",
  });
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
  await apiFetch(`/projects/${projectId}`, { method: "DELETE" });
}
