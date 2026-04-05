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
type MonacoDiffEditorInstance = import("monaco-editor").editor.IStandaloneDiffEditor;
type MonacoModel = import("monaco-editor").editor.ITextModel;

interface MonacoDiffEditorProps {
  path: string;
  originalValue: string;
  modifiedValue: string;
}

const MonacoDiffEditor: Component<MonacoDiffEditorProps> = (props) => {
  let hostRef: HTMLDivElement | undefined;
  let editor: MonacoDiffEditorInstance | null = null;
  let originalModel: MonacoModel | null = null;
  let modifiedModel: MonacoModel | null = null;
  let monaco: MonacoNamespace | null = null;
  let resizeObserver: ResizeObserver | undefined;
  let themeObserver: MutationObserver | undefined;

  const applyTheme = () => {
    if (!monaco) return;
    monaco.editor.setTheme(monacoThemeName());
  };

  const ensureModels = () => {
    if (!monaco || !editor) return;
    const nextLanguage = monacoLanguageForPath(props.path);
    const baseUri = encodeURIComponent(props.path);
    const originalUri = monaco.Uri.parse(`inmemory://hirsel-diff/original/${baseUri}`);
    const modifiedUri = monaco.Uri.parse(`inmemory://hirsel-diff/modified/${baseUri}`);

    if (!originalModel || originalModel.uri.toString() !== originalUri.toString()) {
      originalModel?.dispose();
      originalModel = monaco.editor.createModel(props.originalValue, nextLanguage, originalUri);
    } else {
      monaco.editor.setModelLanguage(originalModel, nextLanguage);
      if (originalModel.getValue() !== props.originalValue) {
        originalModel.setValue(props.originalValue);
      }
    }

    if (!modifiedModel || modifiedModel.uri.toString() !== modifiedUri.toString()) {
      modifiedModel?.dispose();
      modifiedModel = monaco.editor.createModel(props.modifiedValue, nextLanguage, modifiedUri);
    } else {
      monaco.editor.setModelLanguage(modifiedModel, nextLanguage);
      if (modifiedModel.getValue() !== props.modifiedValue) {
        modifiedModel.setValue(props.modifiedValue);
      }
    }

    editor.setModel({
      original: originalModel,
      modified: modifiedModel,
    });
  };

  onMount(() => {
    let disposed = false;

    void (async () => {
      const loaded = await loadMonaco();
      if (disposed || !hostRef) return;
      monaco = loaded;
      applyTheme();
      editor = monaco.editor.createDiffEditor(hostRef, {
        automaticLayout: false,
        diffCodeLens: true,
        enableSplitViewResizing: true,
        fontFamily: "Martian Mono, ui-monospace, monospace",
        fontLigatures: false,
        fontSize: 12,
        glyphMargin: false,
        lineNumbersMinChars: 4,
        minimap: { enabled: false },
        originalEditable: false,
        padding: { top: 12, bottom: 12 },
        readOnly: true,
        renderIndicators: true,
        renderMarginRevertIcon: false,
        renderOverviewRuler: false,
        renderSideBySide: true,
        roundedSelection: false,
        scrollBeyondLastLine: false,
        smoothScrolling: true,
        tabSize: 2,
      });
      ensureModels();

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
      originalModel?.dispose();
      modifiedModel?.dispose();
      editor = null;
      originalModel = null;
      modifiedModel = null;
      monaco = null;
    });
  });

  createEffect(() => {
    ensureModels();
  });

  return <div ref={hostRef} class="h-full w-full" />;
};

export default MonacoDiffEditor;
