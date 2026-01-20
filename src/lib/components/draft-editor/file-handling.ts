/**
 * File handling utilities for draft editor
 */

import { importAssetFromPath, saveAsset } from '../../api';

declare const window: Window & {
  toast?: {
    success: (message: string, title?: string) => void;
    error: (message: string, title?: string) => void;
  };
};

/**
 * Determine if a file is an image based on type or extension
 */
export function isImageFile(filename: string, mimeType?: string): boolean {
  if (mimeType?.startsWith('image/')) return true;
  return /\.(png|jpg|jpeg|gif|webp|svg|bmp|ico)$/i.test(filename);
}

/**
 * Create markdown reference for a file
 */
export function createMarkdownRef(filename: string, isImage: boolean): string {
  return isImage ? `![${filename}](assets/${filename})` : `[${filename}](assets/${filename})`;
}

/**
 * Process and save a dropped web file (from browser drag-drop)
 * Returns the markdown reference string or null if failed
 */
export async function processDroppedFile(
  runName: string,
  file: File,
): Promise<{ filename: string; markdownRef: string } | null> {
  try {
    const arrayBuffer = await file.arrayBuffer();
    const data = Array.from(new Uint8Array(arrayBuffer));
    const savedFilename = await saveAsset(runName, file.name, data);

    const isImage = isImageFile(file.name, file.type);
    const markdownRef = createMarkdownRef(savedFilename, isImage);

    return { filename: savedFilename, markdownRef };
  } catch (err) {
    console.error('Failed to save file:', err);
    window.toast?.error(`Failed to save: ${file.name}`);
    return null;
  }
}

/**
 * Process and import a native file path (from Tauri drag-drop)
 * Returns the markdown reference string or null if failed
 */
export async function processNativeFilePath(
  runName: string,
  filePath: string,
): Promise<{ filename: string; markdownRef: string } | null> {
  try {
    const savedFilename = await importAssetFromPath(runName, filePath);
    const isImage = isImageFile(savedFilename);
    const markdownRef = createMarkdownRef(savedFilename, isImage);

    return { filename: savedFilename, markdownRef };
  } catch (err) {
    console.error('Failed to import file:', err);
    const filename = filePath.split('/').pop() || filePath;
    window.toast?.error(`Failed to import: ${filename}`);
    return null;
  }
}

/**
 * Calculate insertion position in text based on drop coordinates
 */
export function calculateInsertPosition(
  content: string,
  textarea: HTMLTextAreaElement | null,
  dropY: number,
): number {
  if (!textarea) return content.length;

  const rect = textarea.getBoundingClientRect();
  const relativeY = dropY - rect.top;

  // Get computed style for line height
  const style = window.getComputedStyle(textarea);
  const lineHeight = Number.parseFloat(style.lineHeight) || Number.parseFloat(style.fontSize) * 1.2;
  const paddingTop = Number.parseFloat(style.paddingTop) || 0;

  // Calculate which line was dropped on (accounting for scroll)
  const scrollTop = textarea.scrollTop;
  const adjustedY = relativeY + scrollTop - paddingTop;
  const targetLine = Math.max(0, Math.floor(adjustedY / lineHeight));

  // Find the character position at the start of that line
  const lines = content.split('\n');
  let charPos = 0;
  for (let i = 0; i < Math.min(targetLine, lines.length); i++) {
    charPos += lines[i].length + 1; // +1 for newline
  }

  return Math.min(charPos, content.length);
}

/**
 * Insert content at a position with proper newline handling
 */
export function insertAtPosition(content: string, insertion: string, insertPos: number): string {
  const before = content.slice(0, insertPos);
  const after = content.slice(insertPos);

  // Check if we're at the start of a line
  const atLineStart = insertPos === 0 || content[insertPos - 1] === '\n';

  // Find end of current line
  const nextNewline = after.indexOf('\n');
  const currentLineContent = nextNewline === -1 ? after : after.slice(0, nextNewline);
  const isLineEmpty = currentLineContent.trim() === '';

  if (atLineStart && isLineEmpty) {
    // At start of empty line - just insert
    return before + insertion + after;
  }
  if (atLineStart) {
    // At start of non-empty line - insert before with newline after
    return `${before + insertion}\n${after}`;
  }
  // In middle of content - insert on new line
  return `${before}\n${insertion}${after}`;
}
