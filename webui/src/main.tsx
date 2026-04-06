import { render } from "solid-js/web";
import "@/styles/index.css";
import App from "@/App";

const CHUNK_RELOAD_KEY = "hirsel_chunk_reload_once";

window.addEventListener("unhandledrejection", (event) => {
  const reason = event.reason;
  const message =
    reason instanceof Error
      ? reason.message
      : typeof reason === "string"
        ? reason
        : "";

  const chunkLoadFailure =
    message.includes("Failed to fetch dynamically imported module") ||
    message.includes("valid JavaScript MIME type") ||
    message.includes("Importing a module script failed");

  if (!chunkLoadFailure) return;

  if (sessionStorage.getItem(CHUNK_RELOAD_KEY) === "1") return;
  sessionStorage.setItem(CHUNK_RELOAD_KEY, "1");
  window.location.reload();
});

sessionStorage.removeItem(CHUNK_RELOAD_KEY);

const root = document.getElementById("root");
if (!root) throw new Error("Root element not found");

render(() => <App />, root);
