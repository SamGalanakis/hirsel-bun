// RunList Component - Display runs with status icons, selection, progress
import type { RunSummary } from "../../../shared/types";
import { formatElapsed, STATUS_ICON, STATUS_COLOR, COLORS, ditherBar } from "../../../shared/theme";

export interface RunListCallbacks {
  onSelect: (runName: string) => void;
  onContextMenu: (runName: string, x: number, y: number) => void;
  onDoubleClick: (runName: string) => void;
}

export class RunList {
  private container: HTMLElement;
  private runs: RunSummary[] = [];
  private selectedRunName: string | null = null;
  private callbacks: RunListCallbacks;

  constructor(containerId: string, callbacks: RunListCallbacks) {
    const container = document.getElementById(containerId);
    if (!container) throw new Error(`Container #${containerId} not found`);
    this.container = container;
    this.callbacks = callbacks;

    // Add keyboard navigation
    this.container.tabIndex = 0;
    this.container.addEventListener("keydown", this.handleKeydown.bind(this));
  }

  /**
   * Update the run list with new data
   */
  update(runs: RunSummary[], selectedRunName: string | null = null): void {
    this.runs = runs;
    this.selectedRunName = selectedRunName;
    this.render();
  }

  /**
   * Get currently selected run name
   */
  getSelectedRun(): string | null {
    return this.selectedRunName;
  }

  /**
   * Select a run by name
   */
  selectRun(runName: string | null): void {
    this.selectedRunName = runName;
    this.render();
  }

  /**
   * Move selection up/down
   */
  private moveSelection(direction: "up" | "down"): void {
    if (this.runs.length === 0) return;

    const currentIndex = this.selectedRunName
      ? this.runs.findIndex((r) => r.name === this.selectedRunName)
      : -1;

    let newIndex: number;
    if (direction === "up") {
      newIndex = currentIndex <= 0 ? this.runs.length - 1 : currentIndex - 1;
    } else {
      newIndex = currentIndex >= this.runs.length - 1 ? 0 : currentIndex + 1;
    }

    const newRun = this.runs[newIndex];
    if (newRun) {
      this.selectedRunName = newRun.name;
      this.render();
      this.callbacks.onSelect(newRun.name);
    }
  }

  private handleKeydown(e: KeyboardEvent): void {
    switch (e.key) {
      case "ArrowUp":
      case "k":
        e.preventDefault();
        this.moveSelection("up");
        break;
      case "ArrowDown":
      case "j":
        e.preventDefault();
        this.moveSelection("down");
        break;
      case "Enter":
        if (this.selectedRunName) {
          this.callbacks.onDoubleClick(this.selectedRunName);
        }
        break;
    }
  }

  private render(): void {
    if (this.runs.length === 0) {
      this.container.innerHTML = `
        <div class="run-list-empty">
          <span class="icon">○</span>
          <span>No runs yet</span>
        </div>
      `;
      return;
    }

    this.container.innerHTML = this.runs
      .map((run) => this.renderRunItem(run))
      .join("");

    // Attach event listeners
    this.container.querySelectorAll(".run-item").forEach((el) => {
      const runName = el.getAttribute("data-run");
      if (!runName) return;

      el.addEventListener("click", () => {
        this.selectedRunName = runName;
        this.render();
        this.callbacks.onSelect(runName);
      });

      el.addEventListener("dblclick", () => {
        this.callbacks.onDoubleClick(runName);
      });

      el.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        const event = e as MouseEvent;
        this.callbacks.onContextMenu(runName, event.clientX, event.clientY);
      });
    });

    // Scroll selected item into view
    if (this.selectedRunName) {
      const selectedEl = this.container.querySelector(
        `[data-run="${this.selectedRunName}"]`
      );
      selectedEl?.scrollIntoView({ block: "nearest" });
    }
  }

  private renderRunItem(run: RunSummary): string {
    const isSelected = run.name === this.selectedRunName;
    const statusIcon = STATUS_ICON[run.status] || "○";
    const statusColor = STATUS_COLOR[run.status] || COLORS.idle;
    const progress = run.tasksTotal > 0
      ? Math.round((run.tasksDone / run.tasksTotal) * 100)
      : 0;
    const elapsed = formatElapsed(run.elapsedMinutes);

    // Build progress indicator
    const progressBar = run.tasksTotal > 0
      ? ditherBar(run.tasksDone, run.tasksTotal, 8)
      : "";

    return `
      <div class="run-item ${run.status} ${isSelected ? "selected" : ""}" data-run="${run.name}">
        <span class="run-item-icon" style="color: ${isSelected ? "inherit" : statusColor}">${statusIcon}</span>
        <div class="run-item-content">
          <span class="run-item-name">${escapeHtml(run.name)}</span>
          <span class="run-item-meta">
            ${run.tasksDone}/${run.tasksTotal} tasks
            ${run.workersActive > 0 ? ` · ${run.workersActive}w` : ""}
            · ${elapsed}
          </span>
        </div>
        ${run.hasUnread ? '<span class="run-item-unread">●</span>' : ""}
      </div>
    `;
  }
}

// Helper to escape HTML
function escapeHtml(str: string): string {
  const div = document.createElement("div");
  div.textContent = str;
  return div.innerHTML;
}
