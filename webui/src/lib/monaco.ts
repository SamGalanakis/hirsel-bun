import editorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import jsonWorker from "monaco-editor/esm/vs/language/json/json.worker?worker";
import cssWorker from "monaco-editor/esm/vs/language/css/css.worker?worker";
import htmlWorker from "monaco-editor/esm/vs/language/html/html.worker?worker";
import tsWorker from "monaco-editor/esm/vs/language/typescript/ts.worker?worker";

type MonacoNamespace = typeof import("monaco-editor");

declare global {
  interface Window {
    MonacoEnvironment?: {
      getWorker(_: string, label: string): Worker;
    };
  }
}

let monacoPromise: Promise<MonacoNamespace> | null = null;

function ensureMonacoEnvironment(): void {
  if (window.MonacoEnvironment) return;
  window.MonacoEnvironment = {
    getWorker(_: string, label: string): Worker {
      switch (label) {
        case "json":
          return new jsonWorker();
        case "css":
        case "scss":
        case "less":
          return new cssWorker();
        case "html":
        case "handlebars":
        case "razor":
          return new htmlWorker();
        case "typescript":
        case "javascript":
          return new tsWorker();
        default:
          return new editorWorker();
      }
    },
  };
}

export async function loadMonaco(): Promise<MonacoNamespace> {
  if (!monacoPromise) {
    ensureMonacoEnvironment();
    monacoPromise = import("monaco-editor");
  }
  return monacoPromise;
}

export function monacoThemeName(): string {
  const theme = document.documentElement.getAttribute("data-theme") || "hirsel";
  return theme.includes("dark") || theme === "midnight" ? "vs-dark" : "vs";
}

export function monacoLanguageForPath(path: string): string {
  const lower = path.toLowerCase();
  const ext = lower.includes(".") ? lower.split(".").pop() ?? lower : lower;
  const aliases: Record<string, string> = {
    cjs: "javascript",
    cmake: "cmake",
    coffee: "coffeescript",
    cpp: "cpp",
    cxx: "cpp",
    cs: "csharp",
    css: "css",
    dockerfile: "dockerfile",
    go: "go",
    htm: "html",
    html: "html",
    java: "java",
    js: "javascript",
    json: "json",
    jsx: "javascript",
    md: "markdown",
    mjs: "javascript",
    mts: "typescript",
    py: "python",
    rb: "ruby",
    rs: "rust",
    scss: "scss",
    sh: "shell",
    sql: "sql",
    svg: "xml",
    toml: "ini",
    ts: "typescript",
    tsx: "typescript",
    txt: "plaintext",
    xml: "xml",
    yaml: "yaml",
    yml: "yaml",
  };

  if (lower.endsWith("/dockerfile") || lower === "dockerfile") {
    return "dockerfile";
  }

  return aliases[ext] ?? "plaintext";
}
