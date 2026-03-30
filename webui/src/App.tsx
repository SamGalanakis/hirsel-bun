import { type Component, createEffect, createSignal, on, onCleanup, Show } from "solid-js";
import ConnectPage from "@/pages/ConnectPage";
import ProjectPage from "@/pages/ProjectPage";
import ThreadDetailPage from "@/pages/ThreadDetailPage";
import SettingsPage from "@/pages/SettingsPage";
import NewProjectPage from "@/pages/NewProjectPage";
import { listProjects } from "@/lib/api";

type Route =
  | { page: "connect" }
  | { page: "project"; projectId: number }
  | { page: "thread"; projectId: number; threadId: string }
  | { page: "settings" }
  | { page: "new" }
  | { page: "loading" };

function parseHash(hash: string): Route | null {
  const h = hash.replace(/^#\/?/, "");

  if (h === "connect") return { page: "connect" };
  if (h === "settings") return { page: "settings" };
  if (h === "new") return { page: "new" };

  const projectMatch = h.match(/^project\/(\d+)$/);
  if (projectMatch) return { page: "project", projectId: parseInt(projectMatch[1], 10) };

  const threadMatch = h.match(/^thread\/(\d+)\/(.+)$/);
  if (threadMatch)
    return {
      page: "thread",
      projectId: parseInt(threadMatch[1], 10),
      threadId: threadMatch[2],
    };

  return null;
}

const App: Component = () => {
  const [route, setRoute] = createSignal<Route>({ page: "loading" });

  const navigate = () => {
    const parsed = parseHash(window.location.hash);
    if (parsed) {
      setRoute(parsed);
    } else {
      // Default: try to redirect to first project
      listProjects()
        .then((projects) => {
          if (projects.length > 0) {
            window.location.hash = `#project/${projects[0].id}`;
          } else {
            window.location.hash = "#new";
          }
        })
        .catch(() => {
          window.location.hash = "#connect";
        });
    }
  };

  createEffect(() => {
    navigate();
    window.addEventListener("hashchange", navigate);
    onCleanup(() => window.removeEventListener("hashchange", navigate));
  });

  return (
    <>
      <Show when={route().page === "connect"}>
        <ConnectPage />
      </Show>

      <Show when={route().page === "project"}>
        <ProjectPage projectId={(route() as { projectId: number }).projectId} />
      </Show>

      <Show when={route().page === "thread"}>
        <ThreadDetailPage
          projectId={(route() as { projectId: number }).projectId}
          threadId={(route() as { threadId: string }).threadId}
        />
      </Show>

      <Show when={route().page === "settings"}>
        <SettingsPage />
      </Show>

      <Show when={route().page === "new"}>
        <NewProjectPage />
      </Show>

      <Show when={route().page === "loading"}>
        <div class="flex items-center justify-center h-screen bg-background">
          <span class="text-muted-foreground text-xs font-mono">loading...</span>
        </div>
      </Show>
    </>
  );
};

export default App;
