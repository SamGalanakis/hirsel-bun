import { type Component, Suspense, createSignal, lazy, onCleanup, onMount, Show } from "solid-js";
import { ApiError } from "@/lib/api/core";
import ConnectPage from "@/pages/ConnectPage";
import { listProjects } from "@/lib/api";

const WorkspacePage = lazy(() => import("@/pages/WorkspacePage"));
const SettingsPage = lazy(() => import("@/pages/SettingsPage"));
const NewProjectPage = lazy(() => import("@/pages/NewProjectPage"));
const ThreadTreePanel = lazy(() => import("@/components/ThreadTreePanel"));

type ScreenState =
  | { page: "connect" }
  | { page: "project"; projectId: number }
  | { page: "threads"; projectId: number }
  | { page: "settings" }
  | { page: "new" }
  | { page: "loading" };

function parseHash(hash: string): ScreenState | null {
  const h = hash.replace(/^#\/?/, "");

  if (h === "connect") return { page: "connect" };
  if (h === "settings") return { page: "settings" };
  if (h === "new") return { page: "new" };

  const projectMatch = h.match(/^project\/(\d+)$/);
  if (projectMatch) return { page: "project", projectId: parseInt(projectMatch[1], 10) };

  const threadsMatch = h.match(/^threads\/(\d+)$/);
  if (threadsMatch) return { page: "threads", projectId: parseInt(threadsMatch[1], 10) };

  // Legacy thread route → redirect to project (thread becomes an overlay)
  const threadMatch = h.match(/^thread\/(\d+)\/(.+)$/);
  if (threadMatch) {
    const pid = parseInt(threadMatch[1], 10);
    window.location.hash = `#project/${pid}`;
    return { page: "project", projectId: pid };
  }

  // Legacy librarian route → redirect to project (librarian is now background-only)
  const librarianMatch = h.match(/^librarian\/(\d+)$/);
  if (librarianMatch) {
    const pid = parseInt(librarianMatch[1], 10);
    window.location.hash = `#project/${pid}`;
    return { page: "project", projectId: pid };
  }

  return null;
}

const App: Component = () => {
  const [screen, setScreen] = createSignal<ScreenState>({ page: "loading" });

  const navigate = () => {
    const parsed = parseHash(window.location.hash);
    if (parsed) {
      setScreen(parsed);
    } else {
      listProjects()
        .then((projects) => {
          if (projects.length > 0) {
            window.location.hash = `#project/${projects[0].id}`;
          } else {
            window.location.hash = "#new";
          }
        })
        .catch((error) => {
          if (error instanceof ApiError && (error.status === 401 || error.status === 403)) {
            window.location.hash = "#connect";
            return;
          }
          console.error("Failed to load projects", error);
          window.location.hash = "#new";
        });
    }
  };

  onMount(() => {
    navigate();
    window.addEventListener("hashchange", navigate);
    onCleanup(() => window.removeEventListener("hashchange", navigate));
  });

  return (
    <div class="fixed inset-0 flex min-h-0 flex-col overflow-hidden">
      <Suspense
        fallback={
          <div class="flex flex-1 items-center justify-center bg-background">
            <span class="text-muted-foreground text-xs font-mono">loading...</span>
          </div>
        }
      >
        <Show when={screen().page === "connect"}>
          <ConnectPage />
        </Show>

        <Show when={screen().page === "project"}>
          <WorkspacePage projectId={(screen() as { projectId: number }).projectId} />
        </Show>

        <Show when={screen().page === "threads"}>
          <div class="flex h-screen w-screen flex-col bg-background">
            <div class="flex items-center justify-between border-b border-border px-3 py-2">
              <div class="font-mono text-xs uppercase tracking-wider text-muted-foreground">
                Thread tree · project {(screen() as { projectId: number }).projectId}
              </div>
              <button
                type="button"
                class="text-xs text-muted-foreground hover:text-foreground font-mono"
                onClick={() => {
                  window.location.hash = `#project/${(screen() as { projectId: number }).projectId}`;
                }}
              >
                ← back
              </button>
            </div>
            <div class="flex-1 overflow-auto">
              <ThreadTreePanel
                projectId={(screen() as { projectId: number }).projectId}
                onOpenThread={() => {
                  // Could wire into WorkspacePage to open a specific thread.
                }}
              />
            </div>
          </div>
        </Show>

        <Show when={screen().page === "settings"}>
          <SettingsPage />
        </Show>

        <Show when={screen().page === "new"}>
          <NewProjectPage />
        </Show>

        <Show when={screen().page === "loading"}>
          <div class="flex flex-1 items-center justify-center bg-background">
            <span class="text-muted-foreground text-xs font-mono">loading...</span>
          </div>
        </Show>
      </Suspense>
    </div>
  );
};

export default App;
