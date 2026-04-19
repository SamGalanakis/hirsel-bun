import { type Component, For, Show, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { Portal } from "solid-js/web";
import ChatMessage from "@/components/ChatMessage";
import { listLibrarianJobs } from "@/lib/api";
import type { LibrarianJobSummary } from "@/lib/api/types";

interface JobsPanelProps {
  projectId: number;
}

const JobsPanel: Component<JobsPanelProps> = (props) => {
  const [open, setOpen] = createSignal(false);
  const [jobs, setJobs] = createSignal<LibrarianJobSummary[]>([]);
  const [loading, setLoading] = createSignal(false);
  const [selected, setSelected] = createSignal<LibrarianJobSummary | null>(null);
  let panelRef: HTMLDivElement | undefined;
  let buttonRef: HTMLButtonElement | undefined;

  const refresh = async () => {
    setLoading(true);
    try {
      const next = await listLibrarianJobs(props.projectId, 50);
      setJobs(next);
      // Keep the selected job in sync with any status/chunks update.
      const sel = selected();
      if (sel) {
        const replacement = next.find((j) => j.id === sel.id);
        if (replacement) setSelected(replacement);
      }
    } catch {
      setJobs([]);
    } finally {
      setLoading(false);
    }
  };

  const toggle = () => {
    const opening = !open();
    setOpen(opening);
    if (opening) void refresh();
  };

  // Outside-click dismiss the dropdown (not the modal — modal has its
  // own escape/backdrop handling).
  const onDocPointer = (event: PointerEvent) => {
    if (!open() || selected()) return;
    const target = event.target as Node;
    if (panelRef && (panelRef.contains(target) || buttonRef?.contains(target))) return;
    setOpen(false);
  };

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      if (selected()) setSelected(null);
      else if (open()) setOpen(false);
    }
  };

  onMount(() => {
    document.addEventListener("pointerdown", onDocPointer);
    document.addEventListener("keydown", onKeyDown);
  });
  onCleanup(() => {
    document.removeEventListener("pointerdown", onDocPointer);
    document.removeEventListener("keydown", onKeyDown);
  });

  const runningCount = () => jobs().filter((j) => j.status === "running").length;
  const failedCount = () => jobs().filter((j) => j.status === "failed").length;

  return (
    <div class="jobs-panel">
      <button
        ref={buttonRef}
        type="button"
        class="jobs-panel-btn"
        data-open={open()}
        title="Recent librarian jobs"
        onClick={toggle}
      >
        <span class="jobs-panel-btn-label">Jobs</span>
        <Show when={loading()}>
          <span class="jobs-panel-btn-dot jobs-panel-btn-dot-loading" />
        </Show>
        <Show when={!loading() && runningCount() > 0}>
          <span class="jobs-panel-btn-dot jobs-panel-btn-dot-running" />
          <span class="jobs-panel-btn-count">{runningCount()}</span>
        </Show>
        <Show when={!loading() && failedCount() > 0 && runningCount() === 0}>
          <span class="jobs-panel-btn-dot jobs-panel-btn-dot-failed" />
          <span class="jobs-panel-btn-count">{failedCount()}</span>
        </Show>
      </button>
      <Show when={open()}>
        <div
          ref={panelRef}
          class="jobs-panel-dropdown"
          onPointerDown={(e) => e.stopPropagation()}
        >
          <div class="jobs-panel-header">
            <span>Librarian jobs ({jobs().length})</span>
            <button
              type="button"
              class="jobs-panel-refresh"
              title="Refresh"
              onClick={() => void refresh()}
            >
              ↻
            </button>
          </div>
          <Show when={loading() && jobs().length === 0}>
            <div class="jobs-panel-empty">Loading…</div>
          </Show>
          <Show when={!loading() && jobs().length === 0}>
            <div class="jobs-panel-empty">No jobs yet.</div>
          </Show>
          <div class="jobs-panel-scroll">
            <For each={jobs()}>
              {(job) => <JobsListRow job={job} onOpen={setSelected} />}
            </For>
          </div>
        </div>
      </Show>
      <Show when={selected()}>
        <Portal>
          <JobInspectorModal
            job={selected()!}
            onClose={() => setSelected(null)}
            onRefresh={() => void refresh()}
          />
        </Portal>
      </Show>
    </div>
  );
};

const JobsListRow: Component<{
  job: LibrarianJobSummary;
  onOpen: (job: LibrarianJobSummary) => void;
}> = (props) => {
  const title = createMemo(() => {
    if (props.job.target_node_kind && props.job.target_node_id) {
      return `${props.job.target_node_kind}:${props.job.target_node_id}`;
    }
    return props.job.kind;
  });
  const primaryReason = createMemo(() => props.job.reasons[0] ?? null);
  return (
    <button
      type="button"
      class="jobs-panel-item"
      data-status={props.job.status}
      onClick={() => props.onOpen(props.job)}
    >
      <div class="jobs-panel-item-head">
        <span class="jobs-panel-item-kind">{props.job.kind}</span>
        <span class="jobs-panel-item-status">{props.job.status}</span>
        <span class="jobs-panel-item-time">
          {props.job.created_at
            ? new Date(props.job.created_at).toLocaleTimeString()
            : "—"}
        </span>
      </div>
      <div class="jobs-panel-item-sub">
        <span class="jobs-panel-item-title">{title()}</span>
        <Show when={primaryReason()}>
          <span class="jobs-panel-item-reason">{primaryReason()}</span>
        </Show>
      </div>
      <Show when={props.job.last_error}>
        <div class="jobs-panel-item-error">{props.job.last_error}</div>
      </Show>
    </button>
  );
};

const JobInspectorModal: Component<{
  job: LibrarianJobSummary;
  onClose: () => void;
  onRefresh: () => void;
}> = (props) => {
  const target = createMemo(() => {
    if (props.job.target_node_kind && props.job.target_node_id) {
      return `${props.job.target_node_kind}:${props.job.target_node_id}`;
    }
    return null;
  });

  const created = createMemo(() =>
    props.job.created_at ? new Date(props.job.created_at) : null,
  );
  const updated = createMemo(() =>
    props.job.updated_at ? new Date(props.job.updated_at) : null,
  );
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

  // The prompt always exists — render it as the "user" turn so the
  // librarian's reply appears opposite. ChatMessage takes chunks_json,
  // so wrap the prompt text in a single Text chunk.
  const userChunksJson = createMemo(() =>
    JSON.stringify([{ type: "text", content: props.job.prompt }]),
  );

  const responseChunksJson = createMemo(() => props.job.response_chunks_json);

  const reasonTone = (reason: string): string => {
    if (reason.startsWith("contradiction_with:")) return "warning";
    if (reason.startsWith("upstream_superseded:")) return "warning";
    if (reason.startsWith("comment_pileup:")) return "info";
    if (reason.startsWith("workspace_merge:")) return "info";
    if (reason.startsWith("A2_hot_aging")) return "warning";
    if (reason.startsWith("A4_user_forgot")) return "warning";
    if (reason === "user_request") return "user";
    return "default";
  };

  return (
    <div
      class="job-inspector-backdrop"
      onClick={(e) => {
        if (e.target === e.currentTarget) props.onClose();
      }}
    >
      <div
        class="job-inspector"
        role="dialog"
        aria-modal="true"
        aria-label="Librarian job inspector"
        onClick={(e) => e.stopPropagation()}
      >
        <header class="job-inspector-header">
          <div class="job-inspector-head-top">
            <span class="job-inspector-kind">{props.job.kind}</span>
            <span
              class="job-inspector-status"
              data-status={props.job.status}
            >
              {props.job.status}
            </span>
            <Show when={target()}>
              <span class="job-inspector-target" title="Target node">
                {target()}
              </span>
            </Show>
            <button
              type="button"
              class="job-inspector-refresh"
              title="Refresh"
              onClick={props.onRefresh}
            >
              ↻
            </button>
            <button
              type="button"
              class="job-inspector-close"
              aria-label="Close"
              onClick={props.onClose}
            >
              ×
            </button>
          </div>
          <div class="job-inspector-head-meta">
            <Show when={created()}>
              <span>Started {created()!.toLocaleString()}</span>
            </Show>
            <Show when={updated() && duration()}>
              <span>· duration {duration()}</span>
            </Show>
          </div>
          <Show when={props.job.reasons.length > 0}>
            <div class="job-inspector-reasons">
              <span class="job-inspector-reasons-label">Triggers</span>
              <For each={props.job.reasons}>
                {(reason) => (
                  <span
                    class="job-inspector-reason"
                    data-tone={reasonTone(reason)}
                    title={reason}
                  >
                    {reason}
                  </span>
                )}
              </For>
            </div>
          </Show>
          <Show when={props.job.last_error}>
            <div class="job-inspector-error">
              <span class="job-inspector-error-label">Error</span>
              <span>{props.job.last_error}</span>
            </div>
          </Show>
        </header>
        <div class="job-inspector-body">
          <ChatMessage
            messageId={`${props.job.id}-prompt`}
            role="user"
            chunksJson={userChunksJson()}
            timestamp={props.job.created_at ?? ""}
            collapsedByDefault={true}
          />
          <Show
            when={responseChunksJson()}
            fallback={
              <div class="job-inspector-empty">
                <Show
                  when={props.job.status === "running" || props.job.status === "queued"}
                  fallback={<>No transcript captured for this job.</>}
                >
                  Still {props.job.status}. The transcript appears when the run completes.
                </Show>
              </div>
            }
          >
            <ChatMessage
              messageId={`${props.job.id}-response`}
              role="assistant"
              chunksJson={responseChunksJson()!}
              timestamp={props.job.updated_at ?? props.job.created_at ?? ""}
            />
          </Show>
        </div>
      </div>
    </div>
  );
};

export default JobsPanel;
