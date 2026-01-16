/**
 * Shared Chunk Renderer - Common rendering logic for AI output chunks
 *
 * Used by both Gyp chat (direct-chat.ts) and Worker output viewer (worker-output-viewer.ts)
 * to ensure consistent look and behavior for text, thinking, and tool chunks.
 */

import type { ChatToolCall } from '../types';
import { getIcon, getToolKindIcon, getToolStatusIcon as getToolStatusIconSvg } from '../icons';

/** A chunk of content - text, thinking, or tool */
export interface OutputChunk {
  id: string;
  type: 'text' | 'thinking' | 'tool';
  content?: string;
  tool?: ChatToolCall;
}

/**
 * Shared helper functions for chunk rendering.
 * These can be spread into an Alpine component's return object.
 */
export function chunkRendererHelpers() {
  return {
    /**
     * Get icon for a tool kind
     */
    getToolIcon(kind: string | null): string {
      return getToolKindIcon(kind, 14);
    },

    /**
     * Get status icon for a tool
     */
    getToolStatusIcon(status: string): string {
      return getToolStatusIconSvg(status, 12);
    },

    /**
     * Get CSS class for tool status
     */
    getToolStatusClass(status: string | null | undefined): string {
      const classes: Record<string, string> = {
        pending: 'text-wool-500',
        in_progress: 'text-amber-400',
        completed: 'text-sage',
        failed: 'text-terra',
      };
      return classes[status || ''] || 'text-wool-500';
    },

    /**
     * Check if this is a terminal/bash tool by kind or title
     */
    isTerminalTool(tool: ChatToolCall | null | undefined): boolean {
      if (!tool) return false;
      const isTerminalByKind = tool.kind === 'execute' || tool.kind === 'bash' || tool.kind === 'terminal';
      const isTerminalByTitle = tool.title?.toLowerCase().includes('terminal') ||
                                tool.title?.toLowerCase().includes('bash') ||
                                tool.title?.toLowerCase() === 'bash';
      return isTerminalByKind || isTerminalByTitle;
    },

    /**
     * Extract command from tool input JSON for terminal/bash tools
     */
    getToolCommand(tool: ChatToolCall | null | undefined): string | null {
      if (!tool?.input) return null;
      if (!this.isTerminalTool(tool)) return null;

      try {
        const parsed = JSON.parse(tool.input);
        return parsed.command || parsed.cmd || null;
      } catch {
        return null;
      }
    },

    /**
     * Get truncated command preview (for inline display)
     * Returns null if title already contains the command (e.g., "`df -h`")
     */
    getCommandPreview(tool: ChatToolCall | null | undefined, maxLen = 60): string | null {
      const cmd = this.getToolCommand(tool);
      if (!cmd) return null;

      // Don't show preview if title already contains the command
      if (tool?.title?.includes('`') && tool.title.includes(cmd.split(' ')[0])) {
        return null;
      }

      if (cmd.length <= maxLen) return cmd;
      return cmd.substring(0, maxLen) + '…';
    },

    /**
     * Check if a tool has output to display
     */
    hasToolOutput(tool: ChatToolCall | null | undefined): boolean {
      if (!tool?.output) return false;
      const output = tool.output.trim();
      return output.length > 0 && output !== 'null' && output !== '""';
    },

    /**
     * Get formatted tool output (parse JSON if needed)
     */
    getToolOutput(tool: ChatToolCall | null | undefined): string {
      if (!tool?.output) return '';
      try {
        // Try to parse as JSON and format nicely
        const parsed = JSON.parse(tool.output);
        if (typeof parsed === 'string') {
          return parsed;
        }
        return JSON.stringify(parsed, null, 2);
      } catch {
        // Return as-is if not JSON
        return tool.output;
      }
    },

    /**
     * Check if a tool has expandable content (command or output)
     */
    hasExpandableContent(tool: ChatToolCall | null | undefined): boolean {
      if (!tool) return false;
      return this.hasToolOutput(tool) || this.getToolCommand(tool) !== null;
    },

    /**
     * Get expand/collapse chevron icon
     */
    getExpandIcon(expanded: boolean | undefined): string {
      return getIcon(expanded ? 'chevron-up' : 'chevron-down', 12);
    },
  };
}

/**
 * Create a toggle function that searches through a chunks array
 */
export function createToggleToolExpanded(
  getChunks: () => OutputChunk[],
  setChunks: (chunks: OutputChunk[]) => void
) {
  return function toggleToolExpanded(toolId: string) {
    const chunks = getChunks();
    for (const chunk of chunks) {
      if (chunk.type === 'tool' && chunk.tool?.id === toolId) {
        chunk.tool.expanded = !chunk.tool.expanded;
        setChunks([...chunks]);
        return;
      }
    }
  };
}
