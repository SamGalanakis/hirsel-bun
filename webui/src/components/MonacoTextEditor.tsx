import {
  type Component,
  createEffect,
  onCleanup,
  onMount,
} from "solid-js";
import {
  loadMonaco,
  monacoLanguageForPath,
  monacoThemeName,
} from "@/lib/monaco";

type MonacoNamespace = typeof import("monaco-editor");
type MonacoEditor = import("monaco-editor").editor.IStandaloneCodeEditor;
type MonacoModel = import("monaco-editor").editor.ITextModel;

interface MonacoTextEditorProps {
  path: string;
  value: string;
  readOnly?: boolean;
  revealLine?: number | null;
  onChange?: (value: string) => void;
}

const MonacoTextEditor: Component<MonacoTextEditorProps> = (props) => {
  let hostRef: HTMLDivElement | undefined;
  let editor: MonacoEditor | null = null;
  let model: MonacoModel | null = null;
  let monaco: MonacoNamespace | null = null;
  let syncSilenced = false;
  let resizeObserver: ResizeObserver | undefined;
  let themeObserver: MutationObserver | undefined;

  const applyTheme = () => {
    if (!monaco) return;
    monaco.editor.setTheme(monacoThemeName());
  };

  const ensureModel = () => {
    if (!monaco || !editor) return;
    const uri = monaco.Uri.parse(
      `inmemory://hirsel/${encodeURIComponent(props.path)}`,
    );
    const nextLanguage = monacoLanguageForPath(props.path);
    if (!model || model.uri.toString() !== uri.toString()) {
      model?.dispose();
      model = monaco.editor.createModel(props.value, nextLanguage, uri);
      model.onDidChangeContent(() => {
        if (syncSilenced) return;
        props.onChange?.(model?.getValue() ?? "");
      });
      editor.setModel(model);
      return;
    }

    monaco.editor.setModelLanguage(model, nextLanguage);
    if (model.getValue() !== props.value) {
      syncSilenced = true;
      model.setValue(props.value);
      syncSilenced = false;
    }
  };

  onMount(() => {
    let disposed = false;

    void (async () => {
      const loaded = await loadMonaco();
      if (disposed || !hostRef) return;
      monaco = loaded;
      applyTheme();
      editor = monaco.editor.create(hostRef, {
        automaticLayout: false,
        fontFamily: "Red Hat Mono, ui-monospace, monospace",
        fontLigatures: false,
        fontSize: 12,
        glyphMargin: false,
        lineNumbersMinChars: 4,
        minimap: { enabled: false },
        padding: { top: 12, bottom: 12 },
        readOnly: props.readOnly ?? false,
        renderLineHighlight: "gutter",
        roundedSelection: false,
        scrollBeyondLastLine: false,
        smoothScrolling: true,
        tabSize: 2,
        wordWrap: "off",
      });
      ensureModel();

      resizeObserver = new ResizeObserver(() => {
        editor?.layout();
      });
      resizeObserver.observe(hostRef);

      themeObserver = new MutationObserver(() => {
        applyTheme();
      });
      themeObserver.observe(document.documentElement, {
        attributes: true,
        attributeFilter: ["data-theme"],
      });
    })();

    onCleanup(() => {
      disposed = true;
      themeObserver?.disconnect();
      resizeObserver?.disconnect();
      editor?.dispose();
      model?.dispose();
      editor = null;
      model = null;
      monaco = null;
    });
  });

  createEffect(() => {
    ensureModel();
    if (!editor) return;
    editor.updateOptions({ readOnly: props.readOnly ?? false });
  });

  createEffect(() => {
    const line = props.revealLine;
    if (!editor || !line || line <= 0) return;
    editor.setSelection({
      startLineNumber: line,
      startColumn: 1,
      endLineNumber: line,
      endColumn: 1,
    });
    editor.revealLineInCenter(line);
    editor.focus();
  });

  return <div ref={hostRef} class="h-full w-full" />;
};

export default MonacoTextEditor;
