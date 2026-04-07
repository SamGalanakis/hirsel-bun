import { type Component, createEffect, onCleanup, onMount } from "solid-js";
import { DiffHunksRenderer, parseDiffFromFile } from "@pierre/diffs";

const PIERRE_DIFFS_TAG = "diffs-container";

function ensurePierreDiffElement() {
  if (typeof HTMLElement === "undefined" || customElements.get(PIERRE_DIFFS_TAG)) {
    return;
  }

  class PierreDiffContainer extends HTMLElement {
    constructor() {
      super();
      if (this.shadowRoot) return;
      this.attachShadow({ mode: "open" });
    }
  }

  customElements.define(PIERRE_DIFFS_TAG, PierreDiffContainer);
}

interface PierreDiffViewerProps {
  path: string;
  originalValue: string;
  modifiedValue: string;
}

function currentThemeType(): "dark" | "light" {
  const theme = document.documentElement.getAttribute("data-theme") ?? "";
  return theme === "hirsel" || theme === "bone" ? "light" : "dark";
}

const PierreDiffViewer: Component<PierreDiffViewerProps> = (props) => {
  let containerRef: HTMLElement | undefined;
  let renderer: DiffHunksRenderer | null = null;
  let themeObserver: MutationObserver | undefined;

  const renderDiff = () => {
    if (!containerRef || !renderer) return;

    const oldFile = { name: props.path, contents: props.originalValue };
    const newFile = { name: props.path, contents: props.modifiedValue };
    const diff = parseDiffFromFile(oldFile, newFile);

    const result = renderer.renderDiff(diff);
    if (!result) {
      containerRef.shadowRoot!.innerHTML = "";
      return;
    }
    const html = renderer.renderFullHTML(result);
    // Pierre's web component creates a shadow root — inject there
    if (containerRef.shadowRoot) {
      containerRef.shadowRoot.innerHTML = html;
    }
  };

  const applyTheme = () => {
    if (!renderer) return;
    const themeType = currentThemeType();
    renderer.setOptions({
      theme: {
        dark: "pierre-dark",
        light: "pierre-light",
      },
      themeType,
      diffStyle: "unified",
      headerRenderMode: "none" as never,
    });
    renderDiff();
  };

  onMount(() => {
    ensurePierreDiffElement();

    renderer = new DiffHunksRenderer({
      theme: {
        dark: "pierre-dark",
        light: "pierre-light",
      },
      themeType: currentThemeType(),
      diffStyle: "unified",
      headerRenderMode: "none" as never,
    });

    renderDiff();

    themeObserver = new MutationObserver(() => applyTheme());
    themeObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme"],
    });
  });

  createEffect(() => {
    // Track reactive props
    void props.path;
    void props.originalValue;
    void props.modifiedValue;
    renderDiff();
  });

  onCleanup(() => {
    themeObserver?.disconnect();
    renderer?.cleanUp();
    renderer = null;
  });

  return <diffs-container ref={containerRef} class="block h-full w-full overflow-auto" />;
};

export default PierreDiffViewer;

// Extend JSX for the web component
declare module "solid-js" {
  namespace JSX {
    interface IntrinsicElements {
      "diffs-container": JSX.HTMLAttributes<HTMLElement>;
    }
  }
}
