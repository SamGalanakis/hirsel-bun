const ALLOWED_TAGS = new Set([
  "a",
  "article",
  "aside",
  "b",
  "button",
  "blockquote",
  "br",
  "canvas",
  "circle",
  "code",
  "defs",
  "dd",
  "details",
  "div",
  "dl",
  "dt",
  "em",
  "fieldset",
  "figcaption",
  "figure",
  "footer",
  "form",
  "g",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "header",
  "hr",
  "i",
  "img",
  "input",
  "label",
  "legend",
  "line",
  "li",
  "lineargradient",
  "main",
  "mark",
  "nav",
  "ol",
  "option",
  "p",
  "path",
  "polygon",
  "polyline",
  "pre",
  "radialgradient",
  "rect",
  "section",
  "select",
  "small",
  "script",
  "span",
  "strong",
  "style",
  "summary",
  "svg",
  "table",
  "tbody",
  "td",
  "tfoot",
  "th",
  "thead",
  "textarea",
  "tr",
  "u",
  "ul",
  "hirsel-callout",
  "hirsel-card",
  "hirsel-code",
  "hirsel-codediff",
  "hirsel-coderef",
  "hirsel-dia",
  "hirsel-diagram",
  "hirsel-disclosure",
  "hirsel-fileref",
  "hirsel-filelist",
  "hirsel-patchset",
  "hirsel-progress",
  "hirsel-stat",
  "hirsel-stat-grid",
  "hirsel-tabs",
]);

const DROP_WITH_CONTENT_TAGS = new Set([
  "iframe",
  "object",
  "embed",
  "template",
]);

const GLOBAL_ATTRIBUTES = new Set([
  "active",
  "alt",
  "autocomplete",
  "class",
  "checked",
  "colspan",
  "collapsed",
  "cols",
  "cx",
  "cy",
  "d",
  "detail",
  "disabled",
  "fill",
  "filename",
  "for",
  "heading",
  "height",
  "hidden",
  "href",
  "id",
  "label",
  "language",
  "line-end",
  "line-start",
  "max",
  "maxlength",
  "min",
  "minlength",
  "open",
  "name",
  "placeholder",
  "points",
  "preserveaspectratio",
  "r",
  "rel",
  "role",
  "rowspan",
  "rows",
  "rx",
  "ry",
  "scope",
  "selected",
  "status",
  "src",
  "step",
  "stroke",
  "stroke-linecap",
  "stroke-linejoin",
  "stroke-width",
  "style",
  "target",
  "tabindex",
  "summary",
  "title",
  "tone",
  "transform",
  "type",
  "value",
  "viewbox",
  "workspace",
  "width",
  "eyebrow",
  "path",
  "x",
  "x1",
  "x2",
  "y",
  "y1",
  "y2",
]);

const SAFE_STYLE_PROPERTIES = new Set([
  "align-items",
  "background",
  "background-color",
  "border",
  "border-bottom",
  "border-color",
  "border-left",
  "border-radius",
  "border-right",
  "border-top",
  "box-shadow",
  "column-gap",
  "color",
  "display",
  "flex",
  "flex-direction",
  "flex-wrap",
  "font",
  "font-family",
  "font-size",
  "font-weight",
  "gap",
  "grid-column",
  "grid-row",
  "grid-template-columns",
  "grid-template-rows",
  "height",
  "justify-content",
  "letter-spacing",
  "line-height",
  "margin",
  "margin-bottom",
  "margin-left",
  "margin-right",
  "margin-top",
  "max-height",
  "max-width",
  "min-height",
  "min-width",
  "padding",
  "padding-bottom",
  "padding-left",
  "padding-right",
  "padding-top",
  "place-items",
  "position",
  "row-gap",
  "text-transform",
  "text-align",
  "top",
  "right",
  "bottom",
  "left",
  "opacity",
  "overflow",
  "overflow-x",
  "overflow-y",
  "white-space",
  "width",
]);

function isSafeUrl(value: string, attr: string): boolean {
  const raw = value.trim();
  if (!raw) return true;
  if (raw.startsWith("#") || raw.startsWith("/") || raw.startsWith("./") || raw.startsWith("../")) {
    return true;
  }
  if (attr === "src" && raw.startsWith("data:image/")) {
    return true;
  }

  try {
    const parsed = new URL(raw, window.location.origin);
    const protocol = parsed.protocol.toLowerCase();
    return protocol === "http:" || protocol === "https:" || protocol === "mailto:" || protocol === "tel:";
  } catch {
    return false;
  }
}

function sanitizeStyle(value: string): string {
  return value
    .split(";")
    .map((declaration) => declaration.trim())
    .filter(Boolean)
    .map((declaration) => {
      const separator = declaration.indexOf(":");
      if (separator <= 0) return null;
      const property = declaration.slice(0, separator).trim().toLowerCase();
      const propertyValue = declaration.slice(separator + 1).trim();
      if (!SAFE_STYLE_PROPERTIES.has(property)) return null;
      if (/url\s*\(|expression\s*\(|javascript:/i.test(propertyValue)) return null;
      return `${property}: ${propertyValue}`;
    })
    .filter((declaration): declaration is string => declaration !== null)
    .join("; ");
}

function stripDocumentWrapper(raw: string): string {
  const bodyMatch = raw.match(/<body[^>]*>([\s\S]*)<\/body>/i);
  return bodyMatch ? bodyMatch[1] : raw;
}

function sanitizeAttributes(element: Element): void {
  const tag = element.tagName.toLowerCase();
  for (const attribute of Array.from(element.attributes)) {
    const name = attribute.name.toLowerCase();
    const value = attribute.value;

    if (name.startsWith("on")) {
      element.removeAttribute(attribute.name);
      continue;
    }

    if (tag === "script") {
      if (name === "src") {
        element.removeAttribute(attribute.name);
        continue;
      }
      if (name !== "type" && !name.startsWith("data-") && !name.startsWith("aria-")) {
        element.removeAttribute(attribute.name);
      }
      continue;
    }

    if (tag === "style") {
      if (name !== "media" && !name.startsWith("data-") && !name.startsWith("aria-")) {
        element.removeAttribute(attribute.name);
      }
      continue;
    }

    if (
      !GLOBAL_ATTRIBUTES.has(name) &&
      !name.startsWith("aria-") &&
      !name.startsWith("data-")
    ) {
      element.removeAttribute(attribute.name);
      continue;
    }

    if ((name === "href" || name === "src") && !isSafeUrl(value, name)) {
      element.removeAttribute(attribute.name);
      continue;
    }

    if (name === "style") {
      const sanitizedStyle = sanitizeStyle(value);
      if (sanitizedStyle) {
        element.setAttribute("style", sanitizedStyle);
      } else {
        element.removeAttribute("style");
      }
      continue;
    }

    if (name === "target" && value === "_blank") {
      const rel = new Set(
        (element.getAttribute("rel") ?? "")
          .split(/\s+/)
          .map((token) => token.trim())
          .filter(Boolean),
      );
      rel.add("noopener");
      rel.add("noreferrer");
      element.setAttribute("rel", Array.from(rel).join(" "));
    }
  }
}

function sanitizeNode(node: Node): void {
  if (node.nodeType === Node.COMMENT_NODE) {
    node.remove();
    return;
  }

  if (node.nodeType !== Node.ELEMENT_NODE) {
    return;
  }

  const element = node as Element;
  const tag = element.tagName.toLowerCase();

  if (DROP_WITH_CONTENT_TAGS.has(tag)) {
    element.remove();
    return;
  }

  if (!ALLOWED_TAGS.has(tag)) {
    const children = Array.from(element.childNodes);
    children.forEach((child) => sanitizeNode(child));
    element.replaceWith(...children);
    return;
  }

  sanitizeAttributes(element);
  Array.from(element.childNodes).forEach((child) => sanitizeNode(child));
}

export function sanitizeCanvasHtml(raw: string): string {
  const fragment = stripDocumentWrapper(raw);
  const parser = new DOMParser();
  const document = parser.parseFromString(`<body>${fragment}</body>`, "text/html");
  const body = document.body;
  Array.from(body.childNodes).forEach((node) => sanitizeNode(node));
  return body.innerHTML;
}

function scopeSimpleSelector(selector: string, scopeSelector: string): string {
  const trimmed = selector.trim();
  if (!trimmed) return "";
  if (trimmed === ":root" || trimmed === "html" || trimmed === "body") {
    return scopeSelector;
  }

  const replaced = trimmed
    .replaceAll(":root", scopeSelector)
    .replace(/\bhtml\b/g, scopeSelector)
    .replace(/\bbody\b/g, scopeSelector);

  if (replaced.includes(scopeSelector)) {
    return replaced;
  }
  return `${scopeSelector} ${replaced}`;
}

function scopeSelectorList(selectorText: string, scopeSelector: string): string {
  return selectorText
    .split(",")
    .map((selector) => scopeSimpleSelector(selector, scopeSelector))
    .filter(Boolean)
    .join(", ");
}

function serializeScopedRule(rule: CSSRule, scopeSelector: string): string {
  if (rule instanceof CSSStyleRule) {
    return `${scopeSelectorList(rule.selectorText, scopeSelector)} { ${rule.style.cssText} }`;
  }

  if (rule instanceof CSSMediaRule) {
    const children = Array.from(rule.cssRules)
      .map((child) => serializeScopedRule(child, scopeSelector))
      .filter(Boolean)
      .join("\n");
    return children ? `@media ${rule.conditionText} {\n${children}\n}` : "";
  }

  if (rule instanceof CSSSupportsRule) {
    const children = Array.from(rule.cssRules)
      .map((child) => serializeScopedRule(child, scopeSelector))
      .filter(Boolean)
      .join("\n");
    return children ? `@supports ${rule.conditionText} {\n${children}\n}` : "";
  }

  if (rule instanceof CSSKeyframesRule || rule instanceof CSSFontFaceRule) {
    return rule.cssText;
  }

  if ("cssRules" in rule) {
    const nested = Array.from((rule as CSSGroupingRule).cssRules)
      .map((child) => serializeScopedRule(child, scopeSelector))
      .filter(Boolean)
      .join("\n");
    return nested ? `${rule.cssText.split("{")[0]}{\n${nested}\n}` : "";
  }

  return "";
}

function basicScopeCss(cssText: string, scopeSelector: string): string {
  return cssText.replace(/(^|})\s*([^@}{][^{]+)\{/g, (_match, boundary, selectors) => {
    return `${boundary}\n${scopeSelectorList(selectors, scopeSelector)} {`;
  });
}

export function scopeCanvasCss(cssText: string, scopeSelector: string): string {
  try {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(cssText);
    return Array.from(sheet.cssRules)
      .map((rule) => serializeScopedRule(rule, scopeSelector))
      .filter(Boolean)
      .join("\n");
  } catch {
    return basicScopeCss(cssText, scopeSelector);
  }
}
