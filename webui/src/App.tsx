import { type Component, Suspense, createSignal, lazy, onCleanup, onMount, Show } from "solid-js";
import { ApiError } from "@/lib/api/core";
import ConnectPage from "@/pages/ConnectPage";
import { listProjects } from "@/lib/api";

const WorkspacePage = lazy(() => import("@/pages/WorkspacePage"));
const SettingsPage = lazy(() => import("@/pages/SettingsPage"));
const NewProjectPage = lazy(() => import("@/pages/NewProjectPage"));

type ScreenState =
  | { page: "connect" }
  | { page: "project"; projectId: number }
  | { page: "librarian"; projectId: number }
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

  // Legacy thread route → redirect to project (thread becomes an overlay)
  const threadMatch = h.match(/^thread\/(\d+)\/(.+)$/);
  if (threadMatch) {
    const pid = parseInt(threadMatch[1], 10);
    window.location.hash = `#project/${pid}`;
    return { page: "project", projectId: pid };
  }

  const librarianMatch = h.match(/^librarian\/(\d+)$/);
  if (librarianMatch)
    return { page: "librarian", projectId: parseInt(librarianMatch[1], 10) };

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

        <Show when={screen().page === "librarian"}>
          <WorkspacePage
            projectId={(screen() as { projectId: number }).projectId}
            librarianView={true}
          />
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
