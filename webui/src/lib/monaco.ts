type MonacoNamespace = typeof import("monaco-editor/esm/vs/editor/editor.api.js");

type MonacoWorkerConstructor = new () => Worker;

type MonacoWorkers = {
  editorWorker: MonacoWorkerConstructor;
  jsonWorker: MonacoWorkerConstructor;
};

const MONACO_LANGUAGE_MODULES = [
  "monaco-editor/esm/vs/basic-languages/cpp/cpp.contribution.js",
  "monaco-editor/esm/vs/basic-languages/csharp/csharp.contribution.js",
  "monaco-editor/esm/vs/basic-languages/css/css.contribution.js",
  "monaco-editor/esm/vs/basic-languages/dockerfile/dockerfile.contribution.js",
  "monaco-editor/esm/vs/basic-languages/go/go.contribution.js",
  "monaco-editor/esm/vs/basic-languages/html/html.contribution.js",
  "monaco-editor/esm/vs/basic-languages/ini/ini.contribution.js",
  "monaco-editor/esm/vs/basic-languages/java/java.contribution.js",
  "monaco-editor/esm/vs/basic-languages/javascript/javascript.contribution.js",
  "monaco-editor/esm/vs/basic-languages/markdown/markdown.contribution.js",
  "monaco-editor/esm/vs/basic-languages/python/python.contribution.js",
  "monaco-editor/esm/vs/basic-languages/ruby/ruby.contribution.js",
  "monaco-editor/esm/vs/basic-languages/rust/rust.contribution.js",
  "monaco-editor/esm/vs/basic-languages/shell/shell.contribution.js",
  "monaco-editor/esm/vs/basic-languages/sql/sql.contribution.js",
  "monaco-editor/esm/vs/basic-languages/typescript/typescript.contribution.js",
  "monaco-editor/esm/vs/basic-languages/xml/xml.contribution.js",
  "monaco-editor/esm/vs/basic-languages/yaml/yaml.contribution.js",
] as const;

let monacoPromise: Promise<MonacoNamespace> | null = null;
let monacoWorkersPromise: Promise<MonacoWorkers> | null = null;

async function loadMonacoWorkers(): Promise<MonacoWorkers> {
  if (!monacoWorkersPromise) {
    monacoWorkersPromise = Promise.all([
      import("monaco-editor/esm/vs/editor/editor.worker?worker"),
      import("monaco-editor/esm/vs/language/json/json.worker?worker"),
    ]).then(([editorWorkerModule, jsonWorkerModule]) => ({
      editorWorker: editorWorkerModule.default,
      jsonWorker: jsonWorkerModule.default,
    }));
  }
  return monacoWorkersPromise;
}

async function ensureMonacoEnvironment(): Promise<void> {
  if (window.MonacoEnvironment) return;

  const { editorWorker, jsonWorker } = await loadMonacoWorkers();
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
    monacoPromise = (async () => {
      await ensureMonacoEnvironment();

      const [monaco] = await Promise.all([
        import("monaco-editor/esm/vs/editor/editor.api.js"),
        import("monaco-editor/esm/vs/editor/edcore.main.js"),
        import("monaco-editor/esm/vs/language/json/monaco.contribution.js"),
        ...MONACO_LANGUAGE_MODULES.map((modulePath) => import(modulePath)),
      ]);

      return monaco;
    })();
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
