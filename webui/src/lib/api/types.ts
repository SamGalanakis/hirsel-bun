export interface Project {
  id: number;
  name: string;
  description: string | null;
  sandbox_image: string | null;
  created_at: string;
}

export type ProjectPreparationStatus = "pending" | "working" | "done" | "failed";

export interface ProjectPreparationStep {
  id: string;
  label: string;
  status: ProjectPreparationStatus;
  detail: string | null;
  progress: number | null;
}

export interface ProjectPreparation {
  project: Project;
  worker_image: string;
  status: ProjectPreparationStatus;
  headline: string;
  detail: string | null;
  progress: number;
  steps: ProjectPreparationStep[];
  current_step_id: string | null;
  started_at: string;
  updated_at: string;
}

export interface ProjectCreateProbe {
  normalized_repo_url: string;
  suggested_name: string;
  selected_branch: string;
  branch_source: "explicit" | "url" | "detected";
  branches: string[];
  worker_image: string;
}

export interface ProjectSurface {
  canvas_node_id: string | null;
  canvas_label: string | null;
  canvas_html: string | null;
  canvas_source: string | null;
}

export interface WorkspaceSnapshot {
  project: Project;
  project_activity: ScopeActivity;
  project_history: ChatMessage[];
  surface: ProjectSurface;
  threads: ThreadSummary[];
  thread_detail: ThreadDetail | null;
  thread_history: ChatMessage[];
  librarian_activity: ScopeActivity;
  librarian_history: ChatMessage[];
}

export type LiveUpdateKind =
  | "project_changed"
  | "project_preparation_changed"
  | "project_surface_changed"
  | "project_history_changed"
  | "project_activity_changed"
  | "librarian_history_changed"
  | "librarian_activity_changed"
  | "knowledge_graph_changed"
  | "threads_changed"
  | "thread_changed"
  | "thread_history_changed"
  | "thread_activity_changed";

export interface LiveUpdateEvent {
  projectId: number;
  threadId: string | null;
  kind: LiveUpdateKind;
  timestamp: string;
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

export interface ChatSendResponse {
  started: boolean;
  thread_id?: string | null;
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

export interface ThreadSummary {
  thread: ShepherdThread;
  activity: ScopeActivity;
  plan_progress: PlanProgress | null;
}

export interface ThreadDetail {
  thread: ShepherdThread;
  activity: ScopeActivity;
  plan: PlanSnapshot | null;
}

export interface SkillSummary {
  name: string;
  description: string;
}

export interface WorkspaceRoot {
  id: string;
  kind: "main" | "thread" | "remote";
  label: string;
  status: string;
  summary: string | null;
  threadId: string | null;
  branch: string | null;
  readOnly: boolean;
}

export interface WorkspaceTreeEntry {
  rootId: string;
  path: string;
  name: string;
  kind: "directory" | "file";
  size: number | null;
  modifiedAt: string | null;
  mime: string | null;
  isText: boolean | null;
}

export interface WorkspaceTree {
  rootId: string;
  path: string;
  entries: WorkspaceTreeEntry[];
}

export interface WorkspaceCompletionEntry {
  path: string;
  kind: "directory" | "file";
}

export interface WorkspaceFile {
  rootId: string;
  path: string;
  name: string;
  size: number;
  modifiedAt: string | null;
  mime: string | null;
  isText: boolean;
  lineStart: number;
  lineEnd: number;
  totalLines: number;
  truncated: boolean;
  content: string | null;
}

export interface WorkspaceSearchResult {
  rootId: string;
  path: string;
  line: number;
  column: number;
  preview: string;
}

export interface WorkspaceSearchResponse {
  query: string;
  results: WorkspaceSearchResult[];
  truncated: boolean;
}

export interface WorkspaceDiffEntry {
  path: string;
  status: "modified" | "added" | "deleted";
  isText: boolean;
  leftMime: string | null;
  rightMime: string | null;
  leftSize: number | null;
  rightSize: number | null;
}

export interface WorkspaceDiffSummary {
  leftRootId: string;
  rightRootId: string;
  entries: WorkspaceDiffEntry[];
}

export interface WorkspaceDiffFileSide {
  rootId: string;
  path: string;
  name: string;
  size: number;
  modifiedAt: string | null;
  mime: string | null;
  isText: boolean;
  truncated: boolean;
  content: string | null;
}

export interface WorkspaceDiffFile {
  leftRootId: string;
  rightRootId: string;
  path: string;
  status: "modified" | "added" | "deleted";
  isText: boolean;
  additions: number | null;
  deletions: number | null;
  left: WorkspaceDiffFileSide | null;
  right: WorkspaceDiffFileSide | null;
}

export interface KnowledgeGraphNode {
  id: unknown;
  project_id: number;
  kind: string;
  node_id: string;
  label: string;
  summary?: string;
  content?: string;
  confidence?: string;
  source: string;
  metadata: Record<string, unknown>;
  updated_at: string;
}

export interface KnowledgeGraphEdge {
  id: unknown;
  project_id: number;
  relation: string;
  in: unknown;
  out: unknown;
  metadata: Record<string, unknown>;
  created_at: string;
}

export interface KnowledgeGraph {
  nodes: KnowledgeGraphNode[];
  edges: KnowledgeGraphEdge[];
}

export interface SettingsResponse {
  provider: "codex" | "openrouter";
  openrouter_key_masked: string | null;
  openrouter_base_url: string | null;
  role_models: {
    shepherd: RoleModelSettings;
    librarian: RoleModelSettings;
    thread: RoleModelSettings;
  };
  model_catalog: ModelCatalogSettings;
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

export interface RoleModelSettings {
  configured_model: string | null;
  configured_model_variant: string | null;
  effective_model: string;
  effective_model_variant: string | null;
}

export interface ModelCatalogOption {
  value: string;
  label: string;
  description: string | null;
}

export interface ModelCatalogSettings {
  model_options: ModelCatalogOption[];
  variant_options: Record<string, string[]>;
  default_variants: Record<string, string | null>;
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
