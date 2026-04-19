import {
  type Component,
  Show,
  createSignal,
  onCleanup,
  onMount,
} from "solid-js";
import { listLibrarianJobs } from "@/lib/api";
import type { LibrarianJobSummary } from "@/lib/api/types";
import { cn } from "@/lib/cn";

interface JobsPanelProps {
  projectId: number;
  active: boolean;
  onOpen: () => void;
}

/**
 * Sidebar-bottom chip. Clicking it switches the main pane to the
 * JobsBrowser. A lightweight poll feeds the status dot — blue while any
 * job is running, red when there are failures and nothing running — so
 * the user sees a signal without having to open the browser.
 */
const JobsPanel: Component<JobsPanelProps> = (props) => {
  const [summary, setSummary] = createSignal<{ running: number; failed: number; total: number }>(
    { running: 0, failed: 0, total: 0 },
  );

  const refresh = async () => {
    try {
      const jobs: LibrarianJobSummary[] = await listLibrarianJobs(props.projectId, 50);
      let running = 0;
      let failed = 0;
      for (const j of jobs) {
        if (j.status === "running") running++;
        else if (j.status === "failed") failed++;
      }
      setSummary({ running, failed, total: jobs.length });
    } catch {
      // Keep previous summary on transient errors.
    }
  };

  let interval: number | undefined;
  onMount(() => {
    void refresh();
    interval = window.setInterval(() => void refresh(), 10000);
  });
  onCleanup(() => {
    if (interval) window.clearInterval(interval);
  });

  return (
    <button
      type="button"
      class={cn("jobs-panel-btn", props.active && "is-active")}
      title="Open librarian jobs"
      onClick={props.onOpen}
    >
      <span class="jobs-panel-btn-label">Jobs</span>
      <Show when={summary().running > 0}>
        <span class="jobs-panel-btn-dot jobs-panel-btn-dot-running" />
        <span class="jobs-panel-btn-count">{summary().running}</span>
      </Show>
      <Show when={summary().running === 0 && summary().failed > 0}>
        <span class="jobs-panel-btn-dot jobs-panel-btn-dot-failed" />
        <span class="jobs-panel-btn-count">{summary().failed}</span>
      </Show>
      <Show when={summary().running === 0 && summary().failed === 0 && summary().total > 0}>
        <span class="jobs-panel-btn-count jobs-panel-btn-count-muted">{summary().total}</span>
      </Show>
    </button>
  );
};

export default JobsPanel;
