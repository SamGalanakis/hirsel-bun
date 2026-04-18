import { type Component, For, Show, createSignal } from "solid-js";
import { cn } from "@/lib/cn";

export function normalizeTags(tags: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const tag of tags) {
    const normalized = tag.trim().toLowerCase();
    if (!normalized) continue;
    if (seen.has(normalized)) continue;
    seen.add(normalized);
    out.push(normalized);
  }
  return out;
}

interface TagEditorProps {
  value: string[];
  onChange: (tags: string[]) => void | Promise<void>;
  placeholder?: string;
  disabled?: boolean;
  class?: string;
}

/**
 * Inline tag chip editor. Enter, comma, or Tab commits the current buffer.
 * Backspace on an empty buffer removes the last chip.
 */
const TagEditor: Component<TagEditorProps> = (props) => {
  const [buffer, setBuffer] = createSignal("");
  let inputRef: HTMLInputElement | undefined;

  const commit = async (raw: string) => {
    const merged = normalizeTags([...props.value, raw]);
    if (merged.length === props.value.length) return;
    await props.onChange(merged);
  };

  const removeAt = async (index: number) => {
    const next = props.value.filter((_, i) => i !== index);
    await props.onChange(next);
  };

  const flushBuffer = async () => {
    const raw = buffer().trim();
    setBuffer("");
    if (raw) await commit(raw);
  };

  const handleKeyDown = async (e: KeyboardEvent) => {
    if (props.disabled) return;
    if (e.key === "Enter" || e.key === "," || e.key === "Tab") {
      if (buffer().trim()) {
        e.preventDefault();
        await flushBuffer();
      }
    } else if (e.key === "Backspace" && !buffer() && props.value.length > 0) {
      e.preventDefault();
      await removeAt(props.value.length - 1);
    }
  };

  return (
    <div
      class={cn("tag-editor", props.disabled && "is-disabled", props.class)}
      onClick={() => inputRef?.focus()}
    >
      <For each={props.value}>
        {(tag, index) => (
          <span class="tag-chip">
            <span class="tag-chip-label">{tag}</span>
            <Show when={!props.disabled}>
              <button
                type="button"
                class="tag-chip-remove"
                aria-label={`Remove ${tag}`}
                onClick={(e) => {
                  e.stopPropagation();
                  void removeAt(index());
                }}
              >
                <svg viewBox="0 0 10 10" class="h-2.5 w-2.5" fill="none" stroke="currentColor" stroke-width="1.6">
                  <path d="M2 2l6 6M8 2L2 8" />
                </svg>
              </button>
            </Show>
          </span>
        )}
      </For>
      <input
        ref={inputRef}
        type="text"
        class="tag-editor-input"
        value={buffer()}
        placeholder={props.value.length === 0 ? (props.placeholder ?? "add tags…") : ""}
        disabled={props.disabled}
        onInput={(e) => setBuffer(e.currentTarget.value)}
        onKeyDown={handleKeyDown}
        onBlur={() => void flushBuffer()}
      />
    </div>
  );
};

export default TagEditor;
