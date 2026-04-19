export interface ProjectWorkspaceEntry {
  id: string;
  kind: "local" | "git";
  label: string;
  path: string | null;
  url: string | null;
  branch: string | null;
}

export interface Project {
  id: number;
  name: string;
  workspaces: ProjectWorkspaceEntry[];
  shepherd_cwd: string | null;
  created_at: string;
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
  focused_task: Task | null;
  tasks: Task[];
}

export type LiveUpdateKind =
  | "project_changed"
  | "project_surface_changed"
  | "project_history_changed"
  | "project_activity_changed"
  | "knowledge_graph_changed"
  | "threads_changed"
  | "thread_changed"
  | "thread_history_changed"
  | "thread_activity_changed"
  | "tasks_changed"
  | "task_changed"
  | "canvas_layout_changed"
  | "companion_action";

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
  cwd: string | null;
  created_at: string;
  updated_at: string;
  last_activity_at: string;
  highlight: string | null;
  focused_task_id: string | null;
  parent_id?: string | null;
  binding_kind?: string;
  binding_data?: string | null;
  capabilities?: string[];
  merge_status?: string;
  workspace_path?: string | null;
  final_output?: string | null;
}

export interface SpawnedThread {
  thread_id: string;
  title: string;
  status: string;
  workspace_path: string | null;
}

export interface ThreadInspection {
  thread_id: string;
  status: string;
  merge_status: string;
  workspace_path: string | null;
  final_output: string | null;
  parent_id: string | null;
  diff: {
    files: Array<{
      path: string;
      status: string;
      additions: number;
      deletions: number;
      from: string | null;
    }>;
    total_additions: number;
    total_deletions: number;
  } | null;
}

export type MergeResult =
  | { state: "merged"; thread_id: string }
  | { state: "conflict"; thread_id: string; files: string[] };

export interface SpawnThreadRequest {
  objective: string;
  title?: string;
  parent_id?: string | null;
  capabilities?: string[];
  binding_kind?: string;
  binding_data?: string | null;
}

export interface CommentTarget {
  property?: string;
  line_start?: number;
  line_end?: number;
}

export interface Comment {
  id: string;
  project_id: number;
  node_kind: string;
  node_id: string;
  target: CommentTarget | null;
  body: string;
  author: string;
  posted_at: string;
  resolved_at: string | null;
}

export interface Task {
  id: string;
  project_id: number;
  title: string;
  status: string;
  content: string | null;
  review_json: string | null;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

export interface CanvasPosition {
  x: number;
  y: number;
}

export type DocumentSubtype = "markdown" | "html";

export interface CanvasNode {
  kind: string;
  id: string;
  label: string;
  content?: string | null;
  /** Only meaningful when `kind === "document"`. "markdown" | "html". */
  subtype?: DocumentSubtype | string | null;
  status?: string | null;
  tags?: string[] | null;
  focused_task_id?: string | null;
  highlight?: string | null;
  updated_at: string;
  /** Derived staleness tier for KG-backed nodes; `null`/absent for
   *  task/thread nodes which don't live in `kg_node`. */
  staleness?: StalenessTier | null;
}

export interface CanvasEdge {
  from: string;
  to: string;
  relation: string;
}

export interface CanvasView {
  nodes: CanvasNode[];
  edges: CanvasEdge[];
  layout: Record<string, CanvasPosition>;
}

export interface ChatMessage {
  id: number;
  role: string;
  message_kind: string;
  preview_text: string | null;
  collapsed_by_default: boolean;
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
  path: string | null;
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

export type StalenessTier =
  | "fresh"
  | "stable"
  | "stale"
  | "hot_aging"
  | "unread";

export interface KnowledgeGraphNode {
  id: unknown;
  project_id: number;
  kind: string;
  node_id: string;
  label: string;
  summary?: string;
  content?: string;
  /** Only meaningful when `kind === "document"`. "markdown" | "html". */
  subtype?: DocumentSubtype | string | null;
  tags?: string[] | null;
  confidence?: string;
  source: string;
  metadata: Record<string, unknown>;
  updated_at: string;
  /** Derived staleness tier (populated by `get_knowledge_graph`). */
  staleness?: StalenessTier;
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
    search: RoleModelSettings;
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
  /** True iff an OpenRouter key is configured. Embeddings + hybrid
   *  retrieval require it; the UI shows a boot banner when false. */
  embeddings_ready: boolean;
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
