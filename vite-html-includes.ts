import { Plugin } from 'vite';
import * as fs from 'fs';
import * as path from 'path';

/**
 * Simple Vite plugin for HTML file includes.
 *
 * Usage in HTML:
 *   <!--#include file="./templates/section.html" -->
 *
 * The file path is relative to the HTML file containing the include.
 */
export function htmlIncludes(): Plugin {
  const includePattern = /<!--#include\s+file="([^"]+)"\s*-->/g;

  function processIncludes(html: string, basePath: string, seen: Set<string> = new Set()): string {
    return html.replace(includePattern, (match, filePath) => {
      const absolutePath = path.resolve(basePath, filePath);

      // Prevent infinite recursion
      if (seen.has(absolutePath)) {
        console.warn(`[html-includes] Circular include detected: ${absolutePath}`);
        return `<!-- Error: Circular include of ${filePath} -->`;
      }

      if (!fs.existsSync(absolutePath)) {
        console.error(`[html-includes] File not found: ${absolutePath}`);
        return `<!-- Error: File not found ${filePath} -->`;
      }

      seen.add(absolutePath);
      const content = fs.readFileSync(absolutePath, 'utf-8');
      const fileDir = path.dirname(absolutePath);

      // Recursively process nested includes
      return processIncludes(content, fileDir, seen);
    });
  }

  return {
    name: 'html-includes',
    enforce: 'pre',

    transformIndexHtml: {
      order: 'pre',
      handler(html, ctx) {
        const basePath = path.dirname(ctx.filename);
        return processIncludes(html, basePath);
      }
    },

    // Watch included files for HMR
    handleHotUpdate({ file, server }) {
      if (file.includes('/templates/') && file.endsWith('.html')) {
        // Trigger full page reload when template files change
        server.ws.send({ type: 'full-reload' });
        return [];
      }
    }
  };
}
