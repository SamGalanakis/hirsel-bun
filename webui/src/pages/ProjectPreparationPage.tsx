import { type Component, Show, createEffect, createSignal, onCleanup } from "solid-js";
import ProjectPreparationScreen from "@/components/ProjectPreparationScreen";
import {
  getProjectPreparation,
  retryProjectPreparation,
  startProjectPreparation,
  subscribeProjectEvents,
  ApiError,
  type LiveUpdateEvent,
  type ProjectPreparation,
} from "@/lib/api";

interface ProjectPreparationPageProps {
  projectId: number;
  onReady: () => void;
}

const ProjectPreparationPage: Component<ProjectPreparationPageProps> = (props) => {
  const [preparation, setPreparation] = createSignal<ProjectPreparation | null>(null);
  const [retrying, setRetrying] = createSignal(false);
  const [error, setError] = createSignal("");
  let requestSeq = 0;
  let cleanup: (() => void) | undefined;
  const scheduled = new Map<string, number>();

  const clearScheduled = () => {
    for (const timer of scheduled.values()) {
      window.clearTimeout(timer);
    }
    scheduled.clear();
  };

  const clearLiveUpdates = () => {
    cleanup?.();
    cleanup = undefined;
    clearScheduled();
  };

  const schedule = (key: string, work: () => Promise<void>, delay = 60) => {
    if (scheduled.has(key)) return;
    const timer = window.setTimeout(() => {
      scheduled.delete(key);
      void work();
    }, delay);
    scheduled.set(key, timer);
  };

  const loadPreparation = async (): Promise<void> => {
    const seq = ++requestSeq;
    try {
      let state: ProjectPreparation;
      try {
        state = await getProjectPreparation(props.projectId);
      } catch (err) {
        if (err instanceof ApiError && err.status === 404) {
          state = await startProjectPreparation(props.projectId);
        } else {
          throw err;
        }
      }
      if (seq !== requestSeq) return;
      setPreparation(state);
      setError("");
      if (state.status === "done") {
        props.onReady();
      }
    } catch (err) {
      if (seq !== requestSeq) return;
      setError(err instanceof Error ? err.message : "Failed to load project setup");
    }
  };

  const handleLiveUpdate = (event: LiveUpdateEvent) => {
    if (event.projectId !== props.projectId) return;
    if (event.kind === "project_preparation_changed" || event.kind === "project_changed") {
      schedule("preparation", loadPreparation, 0);
    }
  };

  const connect = () => {
    clearLiveUpdates();
    cleanup = subscribeProjectEvents(props.projectId, handleLiveUpdate);
  };

  createEffect(() => {
    requestSeq += 1;
    setPreparation(null);
    setRetrying(false);
    setError("");
    clearLiveUpdates();
    void loadPreparation();
    connect();
    onCleanup(clearLiveUpdates);
  });

  const handleRetry = async () => {
    setRetrying(true);
    try {
      const state = await retryProjectPreparation(props.projectId);
      setPreparation(state);
      setError("");
      if (state.status === "done") {
        props.onReady();
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to retry preparation");
    } finally {
      setRetrying(false);
    }
  };

  return (
    <Show
      when={preparation()}
      fallback={
        <div class="flex min-h-0 flex-1 items-center justify-center bg-background text-foreground">
          <span class="text-xs text-muted-foreground">
            {error() || "Loading project bring-up..."}
          </span>
        </div>
      }
    >
      {(state) => (
        <ProjectPreparationScreen
          preparation={state()}
          retrying={retrying()}
          onRetry={handleRetry}
        />
      )}
    </Show>
  );
};

export default ProjectPreparationPage;
