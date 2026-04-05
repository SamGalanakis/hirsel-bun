import editorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import jsonWorker from "monaco-editor/esm/vs/language/json/json.worker?worker";

import "monaco-editor/esm/vs/editor/edcore.main.js";

import "monaco-editor/esm/vs/basic-languages/cpp/cpp.contribution.js";
import "monaco-editor/esm/vs/basic-languages/csharp/csharp.contribution.js";
import "monaco-editor/esm/vs/basic-languages/css/css.contribution.js";
import "monaco-editor/esm/vs/basic-languages/dockerfile/dockerfile.contribution.js";
import "monaco-editor/esm/vs/basic-languages/go/go.contribution.js";
import "monaco-editor/esm/vs/basic-languages/html/html.contribution.js";
import "monaco-editor/esm/vs/basic-languages/ini/ini.contribution.js";
import "monaco-editor/esm/vs/basic-languages/java/java.contribution.js";
import "monaco-editor/esm/vs/basic-languages/javascript/javascript.contribution.js";
import "monaco-editor/esm/vs/basic-languages/markdown/markdown.contribution.js";
import "monaco-editor/esm/vs/basic-languages/python/python.contribution.js";
import "monaco-editor/esm/vs/basic-languages/ruby/ruby.contribution.js";
import "monaco-editor/esm/vs/basic-languages/rust/rust.contribution.js";
import "monaco-editor/esm/vs/basic-languages/shell/shell.contribution.js";
import "monaco-editor/esm/vs/basic-languages/sql/sql.contribution.js";
import "monaco-editor/esm/vs/basic-languages/typescript/typescript.contribution.js";
import "monaco-editor/esm/vs/basic-languages/xml/xml.contribution.js";
import "monaco-editor/esm/vs/basic-languages/yaml/yaml.contribution.js";

import "monaco-editor/esm/vs/language/json/monaco.contribution.js";

import * as monaco from "monaco-editor/esm/vs/editor/editor.api.js";

type MonacoNamespace = typeof monaco;

let monacoPromise: Promise<MonacoNamespace> | null = null;

function ensureMonacoEnvironment(): void {
  if (window.MonacoEnvironment) return;
  window.MonacoEnvironment = {
    getWorker(_: string, label: string): Worker {
      switch (label) {
        case "json":
          return new jsonWorker();
        default:
          return new editorWorker();
      }
    },
  };
}

export async function loadMonaco(): Promise<MonacoNamespace> {
  if (!monacoPromise) {
    ensureMonacoEnvironment();
    monacoPromise = Promise.resolve(monaco);
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
    cmake: "plaintext",
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
