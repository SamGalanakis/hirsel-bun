export interface ComposerTokenMatch {
  kind: "slash" | "file";
  rangeStart: number;
  rangeEnd: number;
  raw: string;
  query: string;
}

export interface RecognizedSkillMention {
  name: string;
  label: string;
}

export interface RecognizedFileMention {
  path: string;
  lineStart?: number;
  lineEnd?: number;
  label: string;
}

const TOKEN_BOUNDARY = new Set(["(", "[", "{", "<", '"', "'", "`"]);

function isWhitespace(value: string): boolean {
  return /\s/.test(value);
}

function isSkillNameChar(value: string): boolean {
  return /^[a-z0-9-]$/.test(value);
}

function isTokenBoundary(value: string | undefined): boolean {
  return !value || isWhitespace(value) || TOKEN_BOUNDARY.has(value);
}

function clampCursor(text: string, cursor: number): number {
  if (!Number.isFinite(cursor)) return text.length;
  return Math.max(0, Math.min(text.length, cursor));
}

function parseLineSuffix(raw: string): {
  path: string;
  lineStart?: number;
  lineEnd?: number;
} {
  const index = raw.lastIndexOf(":");
  if (index <= 0) {
    return { path: raw };
  }
  const suffix = raw.slice(index + 1).trim();
  if (!suffix || !/^[0-9-]+$/.test(suffix)) {
    return { path: raw };
  }
  if (suffix.includes("-")) {
    const [startRaw, endRaw] = suffix.split("-", 2);
    const start = Number.parseInt(startRaw, 10);
    const end = Number.parseInt(endRaw, 10);
    if (!Number.isFinite(start) || !Number.isFinite(end)) {
      return { path: raw };
    }
    return {
      path: raw.slice(0, index),
      lineStart: start,
      lineEnd: end,
    };
  }
  const line = Number.parseInt(suffix, 10);
  if (!Number.isFinite(line)) {
    return { path: raw };
  }
  return {
    path: raw.slice(0, index),
    lineStart: line,
    lineEnd: line,
  };
}

export function formatFileMentionLabel(mention: {
  path: string;
  lineStart?: number;
  lineEnd?: number;
}): string {
  if (!mention.lineStart) {
    return `@${mention.path}`;
  }
  if (mention.lineEnd && mention.lineEnd !== mention.lineStart) {
    return `@${mention.path}:${mention.lineStart}-${mention.lineEnd}`;
  }
  return `@${mention.path}:${mention.lineStart}`;
}

export function findActiveComposerToken(
  text: string,
  cursor: number,
): ComposerTokenMatch | null {
  const position = clampCursor(text, cursor);
  let tokenStart = -1;
  let trigger: "slash" | "file" | null = null;

  for (let index = position - 1; index >= 0; index -= 1) {
    const char = text[index];
    if (isWhitespace(char)) {
      break;
    }
    if (char === "/" || char === "@") {
      const previous = index > 0 ? text[index - 1] : undefined;
      if (!isTokenBoundary(previous)) {
        continue;
      }
      tokenStart = index;
      trigger = char === "/" ? "slash" : "file";
      break;
    }
  }

  if (tokenStart < 0 || !trigger) {
    return null;
  }

  let tokenEnd = position;
  while (tokenEnd < text.length && !isWhitespace(text[tokenEnd])) {
    tokenEnd += 1;
  }

  const raw = text.slice(tokenStart, tokenEnd);
  if (!raw) {
    return null;
  }

  if (trigger === "slash") {
    const query = raw.slice(1);
    if (query && [...query].some((char) => !isSkillNameChar(char))) {
      return null;
    }
    return {
      kind: "slash",
      rangeStart: tokenStart,
      rangeEnd: tokenEnd,
      raw,
      query,
    };
  }

  if (raw.length === 1) {
    return {
      kind: "file",
      rangeStart: tokenStart,
      rangeEnd: tokenEnd,
      raw,
      query: "",
    };
  }

  return {
    kind: "file",
    rangeStart: tokenStart,
    rangeEnd: tokenEnd,
    raw,
    query: raw.slice(1),
  };
}

export function collectRecognizedSkillMentions(
  text: string,
  skillNames: Iterable<string>,
): RecognizedSkillMention[] {
  const known = new Set(skillNames);
  const seen = new Set<string>();
  const mentions: RecognizedSkillMention[] = [];
  const bytes = text;

  for (let index = 0; index < bytes.length; index += 1) {
    if (bytes[index] !== "/") {
      continue;
    }
    const previous = index > 0 ? bytes[index - 1] : undefined;
    if (!isTokenBoundary(previous)) {
      continue;
    }
    let end = index + 1;
    while (end < bytes.length && isSkillNameChar(bytes[end])) {
      end += 1;
    }
    if (end <= index + 1) {
      continue;
    }
    if (bytes[end] === "/") {
      continue;
    }
    const name = bytes.slice(index + 1, end);
    if (!known.has(name) || seen.has(name)) {
      continue;
    }
    seen.add(name);
    mentions.push({
      name,
      label: `/${name}`,
    });
  }

  return mentions;
}

export function collectRecognizedFileMentions(text: string): RecognizedFileMention[] {
  const mentions: RecognizedFileMention[] = [];
  const seen = new Set<string>();

  for (let index = 0; index < text.length; index += 1) {
    if (text[index] !== "@") {
      continue;
    }
    const previous = index > 0 ? text[index - 1] : undefined;
    if (!isTokenBoundary(previous)) {
      continue;
    }
    let end = index + 1;
    while (end < text.length && !isWhitespace(text[end])) {
      end += 1;
    }
    if (end <= index + 1) {
      continue;
    }
    const trimmed = text
      .slice(index + 1, end)
      .replace(/[,\.\)\]\}]+$/g, "")
      .trim();
    if (!trimmed) {
      continue;
    }
    const parsed = parseLineSuffix(trimmed);
    if (!parsed.path) {
      continue;
    }
    const label = formatFileMentionLabel(parsed);
    if (seen.has(label)) {
      continue;
    }
    seen.add(label);
    mentions.push({
      path: parsed.path,
      lineStart: parsed.lineStart,
      lineEnd: parsed.lineEnd,
      label,
    });
  }

  return mentions;
}
