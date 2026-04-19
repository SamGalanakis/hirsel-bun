import { type Component, For, Show, createSignal, onCleanup, onMount } from "solid-js";
import { listLibrarianJobs } from "@/lib/api";
import type { LibrarianJobSummary } from "@/lib/api/types";

interface JobsPanelProps {
  projectId: number;
}

const JobsPanel: Component<JobsPanelProps> = (props) => {
  const [open, setOpen] = createSignal(false);
  const [jobs, setJobs] = createSignal<LibrarianJobSummary[]>([]);
  const [loading, setLoading] = createSignal(false);
  const [expandedId, setExpandedId] = createSignal<string | null>(null);
  let panelRef: HTMLDivElement | undefined;
  let buttonRef: HTMLButtonElement | undefined;

  const refresh = async () => {
    setLoading(true);
    try {
      setJobs(await listLibrarianJobs(props.projectId, 50));
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

  // Dismiss on outside click
  const onDocPointer = (event: PointerEvent) => {
    if (!open()) return;
    const target = event.target as Node;
    if (panelRef && (panelRef.contains(target) || buttonRef?.contains(target))) return;
    setOpen(false);
  };

  onMount(() => document.addEventListener("pointerdown", onDocPointer));
  onCleanup(() => document.removeEventListener("pointerdown", onDocPointer));

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
        <div ref={panelRef} class="jobs-panel-dropdown" onPointerDown={(e) => e.stopPropagation()}>
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
              {(job) => (
                <div
                  class="jobs-panel-item"
                  data-status={job.status}
                  onClick={() =>
                    setExpandedId((prev) => (prev === job.id ? null : job.id))
                  }
                >
                  <div class="jobs-panel-item-head">
                    <span class="jobs-panel-item-kind">{job.kind}</span>
                    <span class="jobs-panel-item-status">{job.status}</span>
                    <span class="jobs-panel-item-time">
                      {job.created_at
                        ? new Date(job.created_at).toLocaleTimeString()
                        : "—"}
                    </span>
                  </div>
                  <Show when={job.last_error}>
                    <div class="jobs-panel-item-error">{job.last_error}</div>
                  </Show>
                  <Show when={expandedId() === job.id}>
                    <pre class="jobs-panel-item-prompt">
                      {job.prompt}
                      {job.prompt_truncated ? "\n…(truncated)" : ""}
                    </pre>
                  </Show>
                </div>
              )}
            </For>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default JobsPanel;
