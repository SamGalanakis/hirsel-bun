import { render } from "solid-js/web";
import "@/styles/index.css";
import App from "@/App";
import { registerCanvasComponents } from "@/lib/canvas-components";
import { preloadCanvasMermaid } from "@/lib/canvas-mermaid";

const root = document.getElementById("root");
if (!root) throw new Error("Root element not found");

preloadCanvasMermaid();
registerCanvasComponents();

render(() => <App />, root);
