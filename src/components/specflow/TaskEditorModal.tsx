/**
 * TaskEditorModal - CodeMirror-based markdown editor for draft nodes
 *
 * Features:
 * - CodeMirror 6 with markdown syntax highlighting
 * - Minimal toolbar: Bold, Italic, Code, Link
 * - Edit/Preview toggle
 * - Drag-drop files to add as project assets
 */

import { type Component, Show, createSignal, createEffect, onMount, onCleanup } from 'solid-js';
import { useEscapeKey } from '../../hooks';
import { invoke } from '../../lib/invoke';
import { EditorView, keymap } from '@codemirror/view';
import { EditorState } from '@codemirror/state';
import { markdown } from '@codemirror/lang-markdown';
import { defaultKeymap } from '@codemirror/commands';
import { marked } from 'marked';
import DOMPurify from 'dompurify';
import type { BoardNodeTree, NodeKind } from '../../lib/types';
import { Icon } from '../shared';
import { amber, sage } from '../../lib/theme-colors';

interface TaskEditorModalProps {
  node: BoardNodeTree;
  projectId: number;
  onSave: (updates: {
    name: string;
    content: string;
    validates: string[];
    validatedBy: string[];
    blockedBy: string[];
  }) => Promise<void>;
  onClose: () => void;
}

export const TaskEditorModal: Component<TaskEditorModalProps> = (props) => {
  useEscapeKey(() => props.onClose());

  // Form state
  const [name, setName] = createSignal(props.node.name);
  const [content, setContent] = createSignal(props.node.content.replace(/\\n/g, '\n'));
  const [validates, setValidates] = createSignal(props.node.validates.join(', '));
  const [validatedBy, setValidatedBy] = createSignal(props.node.validatedBy.join(', '));
  const [blockedBy, setBlockedBy] = createSignal(props.node.blockedBy.join(', '));

  // UI state
  const [activeTab, setActiveTab] = createSignal<'edit' | 'preview'>('preview');
  const [dragOver, setDragOver] = createSignal(false);
  const [saving, setSaving] = createSignal(false);

  // CodeMirror ref
  let editorContainerRef: HTMLDivElement | undefined;
  let editorView: EditorView | undefined;

  // Dark theme for CodeMirror
  const darkTheme = EditorView.theme({
    '&': {
      backgroundColor: 'var(--pasture-900)',
      color: 'var(--wool-200)',
      fontSize: '13px',
      height: '100%',
    },
    '.cm-content': {
      fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace',
      padding: '12px',
      caretColor: 'var(--amber-500)',
    },
    '.cm-cursor': {
      borderLeftColor: 'var(--amber-500)',
    },
    '&.cm-focused .cm-selectionBackground, .cm-selectionBackground': {
      backgroundColor: 'var(--cm-selection-bg)',
    },
    '.cm-activeLine': {
      backgroundColor: 'var(--cm-active-line-bg)',
    },
    '.cm-gutters': {
      backgroundColor: 'var(--pasture-800)',
      color: 'var(--wool-600)',
      border: 'none',
      borderRight: '1px solid var(--pasture-600)',
    },
    '.cm-lineNumbers .cm-gutterElement': {
      padding: '0 8px 0 12px',
    },
    // Markdown syntax highlighting
    '.cm-header': { color: 'var(--amber-400)' },
    '.cm-strong': { color: 'var(--wool-100)', fontWeight: 'bold' },
    '.cm-emphasis': { color: 'var(--wool-200)', fontStyle: 'italic' },
    '.cm-link': { color: 'var(--sky-400)' },
    '.cm-url': { color: 'var(--wool-500)' },
    '.cm-quote': { color: 'var(--sage)', fontStyle: 'italic' },
    '.cm-list': { color: 'var(--amber-500)' },
  }, { dark: true });

  // Initialize CodeMirror directly (no solid-codemirror wrapper)
  onMount(() => {
    if (!editorContainerRef) return;
    const state = EditorState.create({
      doc: content(),
      extensions: [
        markdown(),
        keymap.of(defaultKeymap),
        darkTheme,
        EditorView.lineWrapping,
        EditorView.updateListener.of((update) => {
          if (update.docChanged) {
            setContent(update.state.doc.toString());
          }
        }),
      ],
    });
    editorView = new EditorView({ state, parent: editorContainerRef });
  });

  onCleanup(() => editorView?.destroy());

  // Keyboard shortcuts (escape handled by useEscapeKey)
  createEffect(() => {
    const handler = (e: KeyboardEvent) => {
      // Cmd/Ctrl+S to save
      if ((e.metaKey || e.ctrlKey) && e.key === 's') {
        e.preventDefault();
        handleSave();
      }
      // Cmd/Ctrl+B for bold (when in editor)
      if ((e.metaKey || e.ctrlKey) && e.key === 'b') {
        e.preventDefault();
        insertFormatting('**', '**');
      }
      // Cmd/Ctrl+I for italic
      if ((e.metaKey || e.ctrlKey) && e.key === 'i') {
        e.preventDefault();
        insertFormatting('*', '*');
      }
    };

    document.addEventListener('keydown', handler);
    onCleanup(() => document.removeEventListener('keydown', handler));
  });

  // Insert formatting around selection or at cursor
  const insertFormatting = (before: string, after: string) => {
    if (!editorView) return;

    const { state, dispatch } = editorView;
    const selection = state.selection.main;
    const selectedText = state.sliceDoc(selection.from, selection.to);

    const transaction = state.update({
      changes: {
        from: selection.from,
        to: selection.to,
        insert: `${before}${selectedText}${after}`,
      },
      selection: {
        anchor: selection.from + before.length,
        head: selection.to + before.length,
      },
    });

    dispatch(transaction);
    editorView.focus();
  };

  // Insert link at cursor
  const insertLink = () => {
    if (!editorView) return;

    const { state, dispatch } = editorView;
    const selection = state.selection.main;
    const selectedText = state.sliceDoc(selection.from, selection.to);

    const linkText = selectedText || 'text';
    const insert = `[${linkText}](url)`;

    const transaction = state.update({
      changes: {
        from: selection.from,
        to: selection.to,
        insert,
      },
      // Select "url" for easy replacement
      selection: {
        anchor: selection.from + linkText.length + 3,
        head: selection.from + linkText.length + 6,
      },
    });

    dispatch(transaction);
    editorView.focus();
  };

  // Handle file drop
  const handleDrop = async (e: DragEvent) => {
    e.preventDefault();
    setDragOver(false);

    if (!e.dataTransfer?.files.length) return;

    const file = e.dataTransfer.files[0];
    const reader = new FileReader();

    reader.onload = async () => {
      try {
        const data = new Uint8Array(reader.result as ArrayBuffer);
        const savedFilename = await invoke<string>('save_project_asset', {
          projectId: props.projectId,
          filename: file.name,
          data: Array.from(data),
        });

        // Insert markdown reference
        const isImage = /\.(png|jpg|jpeg|gif|webp|svg)$/i.test(savedFilename);
        const ref = isImage
          ? `![${savedFilename}](assets/${savedFilename})`
          : `[${savedFilename}](assets/${savedFilename})`;

        if (editorView) {
          const { state, dispatch } = editorView;
          const cursor = state.selection.main.head;
          const transaction = state.update({
            changes: { from: cursor, insert: ref },
            selection: { anchor: cursor + ref.length },
          });
          dispatch(transaction);
          editorView.focus();
        } else {
          // Fallback: append to content
          setContent(prev => prev + '\n' + ref);
        }

        window.toast?.success(`Asset "${savedFilename}" saved`);
      } catch (e) {
        console.error('Failed to save asset:', e);
        window.toast?.error(`Failed to save asset: ${e}`);
      }
    };

    reader.readAsArrayBuffer(file);
  };

  // Open assets folder
  const openAssets = async () => {
    try {
      await invoke('open_project_assets_folder', { projectId: props.projectId });
    } catch (e) {
      console.error('Failed to open assets folder:', e);
    }
  };

  // Save handler
  const handleSave = async () => {
    setSaving(true);
    try {
      await props.onSave({
        name: name(),
        content: content(),
        validates: validates()
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean),
        validatedBy: validatedBy()
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean),
        blockedBy: blockedBy()
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean),
      });
      props.onClose();
    } catch (e) {
      console.error('Failed to save:', e);
      window.toast?.error(`Failed to save: ${e}`);
    } finally {
      setSaving(false);
    }
  };

  // Render markdown preview
  const renderPreview = () => {
    const html = marked.parse(content(), { async: false }) as string;
    return DOMPurify.sanitize(html);
  };

  const isCheck = () => props.node.kind === 'check';
  const typeLabel = () => (isCheck() ? 'Check' : 'Issue');

  return (
    <div
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      onClick={(e) => { if (e.target === e.currentTarget) props.onClose(); }}
    >
      <div
        class="flex flex-col rounded-lg shadow-xl overflow-hidden"
        style={{
          width: 'min(640px, 90vw)',
          height: 'min(70vh, 700px)',
          background: 'var(--pasture-800)',
          border: '1px solid var(--pasture-600)',
        }}
      >
        {/* Header */}
        <div class="p-3 border-b border-pasture-600 flex items-center justify-between flex-shrink-0">
          <div class="flex items-center gap-2.5">
            <div
              class="w-8 h-8 rounded-lg flex items-center justify-center flex-shrink-0"
              style={{
                background: isCheck() ? sage(0.15) : amber(0.12),
                border: `1px solid ${isCheck() ? sage(0.25) : amber(0.2)}`,
              }}
            >
              <Show when={isCheck()} fallback={
                <Icon name="clipboard-list" size={16} class="text-amber-500" />
              }>
                <Icon name="check-circle" size={16} class="text-sage" />
              </Show>
            </div>
            <div>
              <span
                class="text-[9px] font-medium uppercase tracking-wider"
                style={{ color: isCheck() ? 'var(--sage)' : 'var(--amber-500)', opacity: 0.8 }}
              >
                Edit {typeLabel()}
              </span>
              <h2 class="text-sm font-semibold text-wool-100 -mt-0.5 truncate max-w-[300px]">
                {name() || 'Untitled'}
              </h2>
            </div>
          </div>
          <button
            onClick={props.onClose}
            class="p-1.5 rounded text-wool-500 hover:text-wool-300 hover:bg-white/5"
          >
            <Icon name="x" size={16} />
          </button>
        </div>

        {/* Name field */}
        <div class="px-3 py-2 border-b border-pasture-600 flex-shrink-0">
          <div class="flex items-center gap-2">
            <label for="edit-name" class="text-xs font-medium text-wool-400 w-12">Name</label>
            <input
              id="edit-name"
              type="text"
              value={name()}
              onInput={(e) => setName(e.currentTarget.value)}
              placeholder={`${typeLabel()} name...`}
              class="flex-1 px-2.5 py-1.5 rounded text-sm bg-pasture-900 border border-pasture-600 text-wool-100 placeholder-wool-600 focus:outline-none focus:ring-1 focus:ring-amber-500/30"
            />
          </div>
        </div>

        {/* Toolbar */}
        <div class="px-3 py-1.5 border-b border-pasture-600 flex items-center justify-between flex-shrink-0">
          {/* Formatting buttons */}
          <div class="flex items-center gap-0.5">
            <button
              onClick={() => insertFormatting('**', '**')}
              class="p-1.5 rounded text-wool-400 hover:text-wool-200 hover:bg-white/5"
              title="Bold (Cmd+B)"
            >
              <Icon name="bold" size={14} />
            </button>
            <button
              onClick={() => insertFormatting('*', '*')}
              class="p-1.5 rounded text-wool-400 hover:text-wool-200 hover:bg-white/5"
              title="Italic (Cmd+I)"
            >
              <Icon name="italic" size={14} />
            </button>
            <button
              onClick={() => insertFormatting('`', '`')}
              class="p-1.5 rounded text-wool-400 hover:text-wool-200 hover:bg-white/5"
              title="Code"
            >
              <Icon name="code" size={14} />
            </button>
            <button
              onClick={insertLink}
              class="p-1.5 rounded text-wool-400 hover:text-wool-200 hover:bg-white/5"
              title="Link"
            >
              <Icon name="link" size={14} />
            </button>
            <div class="w-px h-4 bg-pasture-600 mx-1" />
            <button
              onClick={openAssets}
              class="p-1.5 rounded text-wool-400 hover:text-wool-200 hover:bg-white/5"
              title="Open assets folder"
            >
              <Icon name="folder-open" size={14} />
            </button>
          </div>

          {/* Edit/Preview toggle */}
          <div class="flex items-center gap-0.5 p-0.5 rounded-md bg-pasture-900">
            <button
              onClick={() => setActiveTab('edit')}
              class={`px-2.5 py-1 rounded text-xs font-medium transition-colors ${
                activeTab() === 'edit'
                  ? 'bg-pasture-700 text-wool-200'
                  : 'text-wool-500 hover:text-wool-300'
              }`}
            >
              Edit
            </button>
            <button
              onClick={() => setActiveTab('preview')}
              class={`px-2.5 py-1 rounded text-xs font-medium transition-colors ${
                activeTab() === 'preview'
                  ? 'bg-pasture-700 text-wool-200'
                  : 'text-wool-500 hover:text-wool-300'
              }`}
            >
              Preview
            </button>
          </div>
        </div>

        {/* Editor / Preview */}
        <div
          class={`flex-1 overflow-hidden relative ${dragOver() ? 'ring-2 ring-amber-500 ring-inset' : ''}`}
          onDrop={handleDrop}
          onDragOver={(e) => { e.preventDefault(); setDragOver(true); }}
          onDragLeave={(e) => { e.preventDefault(); setDragOver(false); }}
        >
          {/* Use CSS display instead of Show to preserve CodeMirror DOM state */}
          <div
            ref={editorContainerRef}
            class="h-full overflow-auto"
            style={{ display: activeTab() === 'edit' ? 'block' : 'none' }}
          />

          <div
            class="h-full overflow-auto p-4 prose prose-invert prose-sm max-w-none"
            innerHTML={renderPreview()}
            style={{
              display: activeTab() === 'preview' ? 'block' : 'none',
              '--tw-prose-body': 'var(--wool-200)',
              '--tw-prose-headings': 'var(--wool-100)',
              '--tw-prose-links': 'var(--sky-400)',
              '--tw-prose-code': 'var(--amber-400)',
            }}
          />

          {/* Drop overlay */}
          <Show when={dragOver()}>
            <div class="absolute inset-0 bg-amber-500/10 flex items-center justify-center pointer-events-none">
              <div class="text-center">
                <Icon name="upload" size={32} class="text-amber-500 mx-auto mb-2" />
                <p class="text-sm text-amber-400">Drop file to add as asset</p>
              </div>
            </div>
          </Show>
        </div>

        {/* Blocked By / Validates field */}
        <div class="px-3 py-2 border-t border-pasture-600 flex-shrink-0">
          <Show when={isCheck()}>
            <div class="flex items-center gap-2">
              <label for="edit-validates" class="text-xs font-medium w-20" style={{ color: 'var(--sage)' }}>
                Validates
              </label>
              <input
                id="edit-validates"
                type="text"
                value={validates()}
                onInput={(e) => setValidates(e.currentTarget.value)}
                placeholder="issue-id-1, issue-id-2"
                class="flex-1 px-2.5 py-1.5 rounded text-xs font-mono bg-pasture-900 text-wool-200 placeholder-wool-600 focus:outline-none focus:ring-1 focus:ring-sage/30"
                style={{ border: `1px solid ${sage(0.4)}` }}
              />
            </div>
          </Show>
          <Show when={!isCheck()}>
            <div class="flex flex-col gap-1.5">
              <div class="flex items-center gap-2">
                <label for="edit-blocked-by" class="text-xs font-medium w-20" style={{ color: 'var(--amber-500)' }}>
                  Blocked by
                </label>
                <input
                  id="edit-blocked-by"
                  type="text"
                  value={blockedBy()}
                  onInput={(e) => setBlockedBy(e.currentTarget.value)}
                  placeholder="issue-id-1, issue-id-2"
                  class="flex-1 px-2.5 py-1.5 rounded text-xs font-mono bg-pasture-900 text-wool-200 placeholder-wool-600 focus:outline-none focus:ring-1 focus:ring-amber-500/30"
                  style={{ border: `1px solid ${amber(0.4)}` }}
                />
              </div>
              <div class="flex items-center gap-2">
                <label for="edit-validated-by" class="text-xs font-medium w-20" style={{ color: 'var(--sage)' }}>
                  Validated by
                </label>
                <input
                  id="edit-validated-by"
                  type="text"
                  value={validatedBy()}
                  onInput={(e) => setValidatedBy(e.currentTarget.value)}
                  placeholder="eval-id-1, eval-id-2"
                  class="flex-1 px-2.5 py-1.5 rounded text-xs font-mono bg-pasture-900 text-wool-200 placeholder-wool-600 focus:outline-none focus:ring-1 focus:ring-sage/30"
                  style={{ border: `1px solid ${sage(0.4)}` }}
                />
              </div>
            </div>
          </Show>
        </div>

        {/* Footer */}
        <div class="px-3 py-2.5 border-t border-pasture-600 flex justify-between items-center flex-shrink-0">
          <p class="text-[10px] text-wool-600">
            Drag files to add assets. Cmd+S to save.
          </p>
          <div class="flex items-center gap-2">
            <button
              onClick={props.onClose}
              class="px-3 py-1.5 rounded text-xs font-medium text-wool-400 hover:text-wool-200 hover:bg-white/5"
            >
              Cancel
            </button>
            <button
              onClick={handleSave}
              disabled={saving()}
              class="px-3 py-1.5 rounded text-xs font-medium disabled:opacity-40"
              style={{
                background: isCheck() ? sage(0.2) : 'var(--amber-500)',
                color: isCheck() ? 'var(--sage)' : 'var(--pasture-900)',
                border: isCheck() ? `1px solid ${sage(0.3)}` : 'none',
              }}
            >
              {saving() ? 'Saving...' : 'Save'}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
