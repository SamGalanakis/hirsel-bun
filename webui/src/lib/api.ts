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
  sandbox_image: string | null;
  created_at: string;
}

export interface ProjectPreparationStep {
  id: string;
  label: string;
  status: string;
  detail: string | null;
  progress: number | null;
}

export interface ProjectPreparation {
  project: Project;
  worker_image: string;
  status: string;
  headline: string;
  detail: string | null;
  progress: number;
  steps: ProjectPreparationStep[];
  started_at: string;
  updated_at: string;
}

export interface ProjectCreateProbe {
  normalized_repo_url: string;
  suggested_name: string;
  selected_branch: string;
  branch_source: "explicit" | "url" | "detected";
  has_root_flake: boolean;
  worker_image: string;
}

export interface ShepherdThread {
  id: string;
  project_id: number;
  title: string;
  objective: string;
  summary: string;
  status: string;
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

export interface PlanProgress {
  completed: number;
  total: number;
}

export interface PlanStep {
  step: string;
  status: string;
}

export interface PlanSnapshot {
  explanation?: string;
  plan: PlanStep[];
}

export interface ThreadPanelState {
  thread: ShepherdThread;
  history: ChatMessage[];
  activity: ScopeActivity;
  plan_progress: PlanProgress | null;
}

export interface ProjectPageData {
  project: Project;
  projects: Project[];
  threads: ThreadPanelState[];
  history: ChatMessage[];
  activity: ScopeActivity;
  focus_html: string | null;
  focus_source: string | null;
}

export interface ThreadPageData {
  project: Project;
  thread: ShepherdThread;
  history: ChatMessage[];
  activity: ScopeActivity;
  plan: PlanSnapshot | null;
}

export interface WorkspaceFileSlice {
  workspace: string;
  path: string;
  line_start: number;
  line_end: number;
  total_lines: number;
  truncated: boolean;
  content: string;
}

export interface SettingsResponse {
  provider: "codex" | "openrouter";
  openrouter_key_masked: string | null;
  openrouter_base_url: string | null;
  codex_configured: boolean;
  codex_source: "env" | "store" | null;
  github_configured: boolean;
  github_token_masked: string | null;
  github_source: "env" | "store" | null;
  tavily_required: boolean;
  tavily_configured: boolean;
  tavily_key_masked: string | null;
  tavily_source: "env" | "store" | null;
}

export interface CodexDeviceStartResponse {
  status: "pending";
  deviceAuthId: string;
  userCode: string;
  verifyUrl: string;
  interval: number;
}

export interface CodexDevicePollResponse {
  status: "pending" | "connected";
  expiresAt: number | null;
}

// ── API Functions ──

export async function listProjects(): Promise<Project[]> {
  const res = await apiFetch("/projects");
  return parseJson<Project[]>(res);
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

export async function getProjectPage(projectId: number): Promise<ProjectPageData> {
  const res = await apiFetch(`/projects/${projectId}/page`);
  return parseJson<ProjectPageData>(res);
}

export async function getProjectPreparation(
  projectId: number,
): Promise<ProjectPreparation> {
  const res = await apiFetch(`/projects/${projectId}/preparation`);
  return parseJson<ProjectPreparation>(res);
}

export async function retryProjectPreparation(
  projectId: number,
): Promise<ProjectPreparation> {
  const res = await apiFetch(`/projects/${projectId}/preparation/retry`, {
    method: "POST",
  });
  return parseJson<ProjectPreparation>(res);
}

export async function getThreadPage(
  projectId: number,
  threadId: string,
): Promise<ThreadPageData> {
  const res = await apiFetch(`/projects/${projectId}/threads/${threadId}/page`);
  return parseJson<ThreadPageData>(res);
}

export async function getWorkspaceFileSlice(
  projectId: number,
  data: {
    path: string;
    workspace?: string;
    lineStart?: number;
    lineEnd?: number;
    signal?: AbortSignal;
  },
): Promise<WorkspaceFileSlice> {
  const params = new URLSearchParams();
  params.set("path", data.path);
  if (data.workspace?.trim()) {
    params.set("workspace", data.workspace.trim());
  }
  if (Number.isFinite(data.lineStart) && (data.lineStart ?? 0) > 0) {
    params.set("line_start", String(data.lineStart));
  }
  if (Number.isFinite(data.lineEnd) && (data.lineEnd ?? 0) > 0) {
    params.set("line_end", String(data.lineEnd));
  }
  const res = await apiFetch(`/projects/${projectId}/workspace/file?${params.toString()}`, {
    signal: data.signal,
  });
  return parseJson<WorkspaceFileSlice>(res);
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

export async function getSettings(): Promise<SettingsResponse> {
  const res = await apiFetch("/settings");
  return parseJson<SettingsResponse>(res);
}

export async function saveSettingsProvider(
  provider: "codex" | "openrouter",
): Promise<void> {
  const res = await apiFetch("/settings/provider", {
    method: "POST",
    body: JSON.stringify({ provider }),
  });
  await parseJson<{ ok: true }>(res);
}

export async function saveOpenRouterSettings(data: {
  api_key?: string;
  base_url?: string;
}): Promise<void> {
  const res = await apiFetch("/settings/openrouter", {
    method: "POST",
    body: JSON.stringify(data),
  });
  await parseJson<{ ok: true }>(res);
}

export async function startCodexDeviceFlow(): Promise<CodexDeviceStartResponse> {
  const res = await apiFetch("/settings/provider/codex/device/start", {
    method: "POST",
  });
  return parseJson<CodexDeviceStartResponse>(res);
}

export async function pollCodexDeviceFlow(data: {
  deviceAuthId: string;
  userCode: string;
}): Promise<CodexDevicePollResponse> {
  const res = await apiFetch("/settings/provider/codex/device/poll", {
    method: "POST",
    body: JSON.stringify(data),
  });
  return parseJson<CodexDevicePollResponse>(res);
}

export async function saveTavilyKey(api_key: string): Promise<void> {
  const res = await apiFetch("/settings/tavily", {
    method: "POST",
    body: JSON.stringify({ api_key }),
  });
  await parseJson<{ ok: true }>(res);
}

export async function saveGithubToken(token: string): Promise<void> {
  const res = await apiFetch("/settings/github", {
    method: "POST",
    body: JSON.stringify({ token }),
  });
  await parseJson<{ ok: true }>(res);
}
