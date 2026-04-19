import {
  type Component,
  For,
  Show,
  createMemo,
  createSignal,
  onCleanup,
  onMount,
} from "solid-js";
import ChatMessage from "@/components/ChatMessage";
import { listLibrarianJobs } from "@/lib/api";
import type { LibrarianJobSummary } from "@/lib/api/types";
import { cn } from "@/lib/cn";

interface JobsBrowserProps {
  projectId: number;
}

/**
 * Full main-pane view of librarian jobs. Left column is a dense
 * scrollable list; right column is a chat-style inspector showing the
 * exact prompt that fired the job and the transcript the librarian
 * produced while running. Triggers (reasons) render as coloured chips
 * so the "what caused this" is readable at a glance.
 */
const JobsBrowser: Component<JobsBrowserProps> = (props) => {
  const [jobs, setJobs] = createSignal<LibrarianJobSummary[]>([]);
  const [selectedId, setSelectedId] = createSignal<string | null>(null);
  const [loading, setLoading] = createSignal(false);
  const [lastFetched, setLastFetched] = createSignal<Date | null>(null);
  const [filter, setFilter] = createSignal<"all" | "running" | "queued" | "completed" | "failed">(
    "all",
  );

  const refresh = async () => {
    setLoading(true);
    try {
      const next = await listLibrarianJobs(props.projectId, 100);
      setJobs(next);
      setLastFetched(new Date());
      // Keep selection stable across refreshes.
      if (!selectedId() && next.length > 0) setSelectedId(next[0].id);
    } catch {
      setJobs([]);
    } finally {
      setLoading(false);
    }
  };

  // Auto-refresh every 5s while any job is running/queued so status
  // updates land without the user hammering the refresh button.
  let interval: number | undefined;
  onMount(() => {
    void refresh();
    interval = window.setInterval(() => {
      const busy = jobs().some((j) => j.status === "running" || j.status === "queued");
      if (busy) void refresh();
    }, 5000);
  });
  onCleanup(() => {
    if (interval) window.clearInterval(interval);
  });

  const visibleJobs = createMemo(() => {
    const f = filter();
    if (f === "all") return jobs();
    return jobs().filter((j) => j.status === f);
  });

  const selectedJob = createMemo(() => jobs().find((j) => j.id === selectedId()) ?? null);

  const counts = createMemo(() => {
    const c = { all: jobs().length, running: 0, queued: 0, completed: 0, failed: 0 };
    for (const j of jobs()) {
      if (j.status === "running") c.running++;
      else if (j.status === "queued") c.queued++;
      else if (j.status === "completed") c.completed++;
      else if (j.status === "failed") c.failed++;
    }
    return c;
  });

  return (
    <div class="jobs-browser">
      {/* Toolbar: filter chips + refresh. */}
      <div class="jobs-browser-toolbar">
        <div class="jobs-browser-filters" role="tablist" aria-label="Filter jobs by status">
          <FilterChip label="All" count={counts().all} active={filter() === "all"} onClick={() => setFilter("all")} />
          <FilterChip
            label="Running"
            count={counts().running}
            tone="running"
            active={filter() === "running"}
            onClick={() => setFilter("running")}
          />
          <FilterChip
            label="Queued"
            count={counts().queued}
            active={filter() === "queued"}
            onClick={() => setFilter("queued")}
          />
          <FilterChip
            label="Completed"
            count={counts().completed}
            tone="completed"
            active={filter() === "completed"}
            onClick={() => setFilter("completed")}
          />
          <FilterChip
            label="Failed"
            count={counts().failed}
            tone="failed"
            active={filter() === "failed"}
            onClick={() => setFilter("failed")}
          />
        </div>
        <div class="jobs-browser-toolbar-meta">
          <Show when={lastFetched()}>
            <span class="jobs-browser-fetched">
              Updated {lastFetched()!.toLocaleTimeString()}
            </span>
          </Show>
          <button
            type="button"
            class="jobs-browser-refresh"
            onClick={() => void refresh()}
            title="Refresh"
            disabled={loading()}
          >
            {loading() ? "…" : "↻"}
          </button>
        </div>
      </div>

      {/* Split: list on the left, detail on the right. */}
      <div class="jobs-browser-split">
        <div class="jobs-browser-list">
          <Show when={loading() && jobs().length === 0}>
            <div class="jobs-browser-empty">Loading…</div>
          </Show>
          <Show when={!loading() && visibleJobs().length === 0}>
            <div class="jobs-browser-empty">No jobs match this filter.</div>
          </Show>
          <For each={visibleJobs()}>
            {(job) => (
              <button
                type="button"
                class={cn("jobs-browser-row", selectedId() === job.id && "is-selected")}
                data-status={job.status}
                onClick={() => setSelectedId(job.id)}
              >
                <div class="jobs-browser-row-head">
                  <span class="jobs-browser-row-kind">{job.kind}</span>
                  <span class="jobs-browser-row-status">{job.status}</span>
                  <span class="jobs-browser-row-time">
                    {job.created_at ? relativeOrTime(job.created_at) : "—"}
                  </span>
                </div>
                <div class="jobs-browser-row-title">
                  <Show when={job.target_node_kind && job.target_node_id} fallback={<span>—</span>}>
                    <span class="jobs-browser-row-target">
                      {job.target_node_kind}:{job.target_node_id}
                    </span>
                  </Show>
                </div>
                <Show when={job.reasons.length > 0}>
                  <div class="jobs-browser-row-reasons">
                    <For each={job.reasons.slice(0, 3)}>
                      {(reason) => (
                        <span class="jobs-browser-row-reason" data-tone={reasonTone(reason)}>
                          {reason}
                        </span>
                      )}
                    </For>
                    <Show when={job.reasons.length > 3}>
                      <span class="jobs-browser-row-reason-more">
                        +{job.reasons.length - 3}
                      </span>
                    </Show>
                  </div>
                </Show>
              </button>
            )}
          </For>
        </div>

        <div class="jobs-browser-detail">
          <Show
            when={selectedJob()}
            fallback={
              <div class="jobs-browser-empty jobs-browser-empty-center">
                Select a job to view its trace.
              </div>
            }
          >
            <JobDetail job={selectedJob()!} />
          </Show>
        </div>
      </div>
    </div>
  );
};

const FilterChip: Component<{
  label: string;
  count: number;
  tone?: "running" | "completed" | "failed";
  active: boolean;
  onClick: () => void;
}> = (props) => (
  <button
    type="button"
    class={cn("jobs-browser-filter", props.active && "is-active")}
    data-tone={props.tone}
    onClick={props.onClick}
    role="tab"
    aria-selected={props.active}
  >
    <span>{props.label}</span>
    <span class="jobs-browser-filter-count">{props.count}</span>
  </button>
);

const JobDetail: Component<{ job: LibrarianJobSummary }> = (props) => {
  const target = createMemo(() => {
    if (props.job.target_node_kind && props.job.target_node_id) {
      return `${props.job.target_node_kind}:${props.job.target_node_id}`;
    }
    return null;
  });

  const created = createMemo(() => (props.job.created_at ? new Date(props.job.created_at) : null));
  const updated = createMemo(() => (props.job.updated_at ? new Date(props.job.updated_at) : null));
  const duration = createMemo(() => {
    const c = created();
    const u = updated();
    if (!c || !u) return null;
    const ms = u.getTime() - c.getTime();
    if (ms < 1000) return `${ms}ms`;
    const s = Math.round(ms / 1000);
    if (s < 60) return `${s}s`;
    const m = Math.floor(s / 60);
    const rs = s % 60;
    return `${m}m ${rs}s`;
  });

  const userChunksJson = createMemo(() =>
    JSON.stringify([{ type: "text", content: props.job.prompt }]),
  );

  return (
    <div class="jobs-browser-detail-inner">
      <header class="jobs-browser-detail-header">
        <div class="jobs-browser-detail-head-top">
          <span class="jobs-browser-detail-kind">{props.job.kind}</span>
          <span class="jobs-browser-detail-status" data-status={props.job.status}>
            {props.job.status}
          </span>
          <Show when={target()}>
            <span class="jobs-browser-detail-target">{target()}</span>
          </Show>
        </div>
        <div class="jobs-browser-detail-meta">
          <Show when={created()}>
            <span>Started {created()!.toLocaleString()}</span>
          </Show>
          <Show when={duration()}>
            <span>· Duration {duration()}</span>
          </Show>
        </div>
        <Show when={props.job.reasons.length > 0}>
          <div class="jobs-browser-detail-reasons">
            <span class="jobs-browser-detail-reasons-label">Triggers</span>
            <For each={props.job.reasons}>
              {(reason) => (
                <span class="jobs-browser-detail-reason" data-tone={reasonTone(reason)} title={reason}>
                  {reason}
                </span>
              )}
            </For>
          </div>
        </Show>
        <Show when={props.job.last_error}>
          <div class="jobs-browser-detail-error">
            <span class="jobs-browser-detail-error-label">Error</span>
            <span>{props.job.last_error}</span>
          </div>
        </Show>
      </header>
      <div class="jobs-browser-detail-body">
        <ChatMessage
          messageId={`${props.job.id}-prompt`}
          role="user"
          chunksJson={userChunksJson()}
          timestamp={props.job.created_at ?? ""}
          collapsedByDefault={true}
        />
        <Show
          when={props.job.response_chunks_json}
          fallback={
            <div class="jobs-browser-detail-placeholder">
              <Show
                when={props.job.status === "running" || props.job.status === "queued"}
                fallback={<>No transcript captured for this job.</>}
              >
                Still {props.job.status}. The transcript will appear when the run completes.
              </Show>
            </div>
          }
        >
          <ChatMessage
            messageId={`${props.job.id}-response`}
            role="assistant"
            chunksJson={props.job.response_chunks_json!}
            timestamp={props.job.updated_at ?? props.job.created_at ?? ""}
          />
        </Show>
      </div>
    </div>
  );
};

function reasonTone(reason: string): string {
  if (reason.startsWith("contradiction_with:")) return "warning";
  if (reason.startsWith("upstream_superseded:")) return "warning";
  if (reason.startsWith("comment_pileup:")) return "info";
  if (reason.startsWith("workspace_merge:")) return "info";
  if (reason.startsWith("A2_hot_aging")) return "warning";
  if (reason.startsWith("A4_user_forgot")) return "warning";
  if (reason === "user_request") return "user";
  return "default";
}

function relativeOrTime(iso: string): string {
  const date = new Date(iso);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  if (diffMs < 0) return date.toLocaleTimeString();
  const s = Math.floor(diffMs / 1000);
  if (s < 60) return `${s}s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  return date.toLocaleString();
}

export default JobsBrowser;
