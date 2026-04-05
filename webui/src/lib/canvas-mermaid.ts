import mermaid from "mermaid";

type MermaidGlobal = typeof globalThis & {
  mermaid?: typeof mermaid;
  __hirselMermaid?: typeof mermaid;
};

let nextDiagramId = 0;
let preloaded = false;

function themeColor(style: CSSStyleDeclaration, variable: string, fallback: string): string {
  const raw = style.getPropertyValue(variable).trim();
  return raw ? `hsl(${raw})` : fallback;
}

function applyMermaidTheme(target: Element): void {
  const style = getComputedStyle(target);
  mermaid.initialize({
    startOnLoad: false,
    securityLevel: "loose",
    theme: "base",
    fontFamily: 'system-ui, -apple-system, "Segoe UI", Roboto, Helvetica, Arial, sans-serif',
    themeVariables: {
      background: themeColor(style, "--background", "#f6f5f1"),
      primaryColor: themeColor(style, "--card", "#ffffff"),
      primaryTextColor: themeColor(style, "--foreground", "#111111"),
      primaryBorderColor: themeColor(style, "--border", "#d2d1cb"),
      secondaryColor: themeColor(style, "--secondary", "#efeeea"),
      secondaryTextColor: themeColor(style, "--foreground", "#111111"),
      secondaryBorderColor: themeColor(style, "--border", "#d2d1cb"),
      tertiaryColor: themeColor(style, "--secondary", "#efeeea"),
      tertiaryBorderColor: themeColor(style, "--border", "#d2d1cb"),
      tertiaryTextColor: themeColor(style, "--muted-foreground", "#666666"),
      lineColor: themeColor(style, "--foreground", "#111111"),
      textColor: themeColor(style, "--foreground", "#111111"),
      mainBkg: themeColor(style, "--card", "#ffffff"),
      clusterBkg: themeColor(style, "--secondary", "#efeeea"),
      clusterBorder: themeColor(style, "--border", "#d2d1cb"),
      nodeBorder: themeColor(style, "--border", "#d2d1cb"),
      edgeLabelBackground: themeColor(style, "--background", "#f6f5f1"),
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

export function preloadCanvasMermaid(): void {
  if (preloaded) return;
  applyMermaidTheme(document.documentElement);
  const global = globalThis as MermaidGlobal;
  global.mermaid = mermaid;
  global.__hirselMermaid = mermaid;
  preloaded = true;
}

export async function renderCanvasMermaid(
  source: string,
  target: Element,
): Promise<string> {
  preloadCanvasMermaid();
  applyMermaidTheme(target);
  const diagramId = `hirsel-mermaid-${++nextDiagramId}`;
  const { svg } = await mermaid.render(diagramId, source);
  return svg;
}
