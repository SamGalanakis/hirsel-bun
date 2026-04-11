type MermaidInstance = typeof import("mermaid").default;

type MermaidGlobal = typeof globalThis & {
  mermaid?: MermaidInstance;
  __hirselMermaid?: MermaidInstance;
};

let nextDiagramId = 0;
let mermaidPromise: Promise<MermaidInstance> | null = null;

function loadMermaid(): Promise<MermaidInstance> {
  if (!mermaidPromise) {
    mermaidPromise = import("mermaid").then((module) => module.default);
  }
  return mermaidPromise;
}

// Resolve a CSS token to a concrete rgb() string via a 1×1 canvas.
// This works for any color space the browser understands (oklch, hsl, rgb, named).
function tokenToRgb(tokenName: string, fallback: string): string {
  try {
    const el = document.createElement("div");
    el.style.background = `var(${tokenName})`;
    document.body.appendChild(el);
    const computed = getComputedStyle(el).backgroundColor;
    el.remove();
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = 1;
    const ctx = canvas.getContext("2d");
    if (!ctx) return fallback;
    ctx.fillStyle = computed;
    ctx.fillRect(0, 0, 1, 1);
    const [r, g, b] = ctx.getImageData(0, 0, 1, 1).data;
    return `rgb(${r}, ${g}, ${b})`;
  } catch {
    return fallback;
  }
}

function applyMermaidTheme(_target: Element, mermaid: MermaidInstance): void {
  // Fallbacks are warm neutrals so we never leak pure black/white if resolution fails.
  const bg = tokenToRgb("--color-background", "rgb(26, 25, 20)");
  const card = tokenToRgb("--color-card", "rgb(33, 32, 26)");
  const fg = tokenToRgb("--color-foreground", "rgb(218, 210, 192)");
  const border = tokenToRgb("--color-border", "rgb(68, 63, 55)");
  const secondary = tokenToRgb("--color-secondary", "rgb(43, 41, 34)");
  const mutedFg = tokenToRgb("--color-muted-foreground", "rgb(138, 132, 118)");

  mermaid.initialize({
    startOnLoad: false,
    securityLevel: "loose",
    theme: "base",
    fontFamily: `"Karla", system-ui, sans-serif`,
    themeVariables: {
      background: bg,
      primaryColor: card,
      primaryTextColor: fg,
      primaryBorderColor: border,
      secondaryColor: secondary,
      secondaryTextColor: fg,
      secondaryBorderColor: border,
      tertiaryColor: secondary,
      tertiaryBorderColor: border,
      tertiaryTextColor: mutedFg,
      lineColor: fg,
      textColor: fg,
      mainBkg: card,
      clusterBkg: secondary,
      clusterBorder: border,
      nodeBorder: border,
      edgeLabelBackground: bg,
      fontSize: "14px",
    },
    flowchart: {
      curve: "basis",
      nodeSpacing: 34,
      rankSpacing: 42,
      padding: 18,
      useMaxWidth: true,
      htmlLabels: true,
    },
    sequence: {
      useMaxWidth: true,
      wrap: true,
    },
    gantt: {
      useMaxWidth: true,
    },
  });
}

export async function preloadCanvasMermaid(): Promise<void> {
  const mermaid = await loadMermaid();
  applyMermaidTheme(document.documentElement, mermaid);
  const global = globalThis as MermaidGlobal;
  global.mermaid = mermaid;
  global.__hirselMermaid = mermaid;
}

export async function renderCanvasMermaid(
  source: string,
  target: Element,
): Promise<string> {
  const mermaid = await loadMermaid();
  applyMermaidTheme(target, mermaid);
  const global = globalThis as MermaidGlobal;
  global.mermaid = mermaid;
  global.__hirselMermaid = mermaid;
  const diagramId = `hirsel-mermaid-${++nextDiagramId}`;
  const { svg } = await mermaid.render(diagramId, source);
  return svg;
}
