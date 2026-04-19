import {
  type Component,
  For,
  Show,
  createResource,
  createSignal,
} from "solid-js";
import {
  addNodeComment,
  listNodeComments,
  resolveComment,
} from "@/lib/api/comments";
import type { Comment, CommentTarget } from "@/lib/api/types";
import { cn } from "@/lib/cn";

interface CommentsPanelProps {
  projectId: number;
  kind: string;
  nodeId: string;
  /** Show resolved comments as well. Default: false. */
  includeResolved?: boolean;
}

const CommentsPanel: Component<CommentsPanelProps> = (props) => {
  const [comments, { refetch }] = createResource(
    () =>
      [props.projectId, props.kind, props.nodeId, props.includeResolved] as [
        number,
        string,
        string,
        boolean | undefined,
      ],
    async ([pid, kind, nid, includeResolved]) =>
      listNodeComments(pid, kind, nid, {
        onlyUnresolved: !includeResolved,
      }),
  );
  const [draft, setDraft] = createSignal("");
  const [property, setProperty] = createSignal("");
  const [lineStart, setLineStart] = createSignal<number | "">("");
  const [lineEnd, setLineEnd] = createSignal<number | "">("");
  const [submitting, setSubmitting] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const submit = async () => {
    const text = draft().trim();
    if (!text) {
      setError("Body is required");
      return;
    }
    const target: CommentTarget | undefined = (() => {
      const t: CommentTarget = {};
      const p = property().trim();
      if (p) t.property = p;
      const s = lineStart();
      const e = lineEnd();
      if (typeof s === "number" && !Number.isNaN(s)) t.line_start = s;
      if (typeof e === "number" && !Number.isNaN(e)) t.line_end = e;
      return Object.keys(t).length === 0 ? undefined : t;
    })();
    setSubmitting(true);
    setError(null);
    try {
      await addNodeComment(props.projectId, props.kind, props.nodeId, text, target);
      setDraft("");
      setProperty("");
      setLineStart("");
      setLineEnd("");
      await refetch();
    } catch (err) {
      setError(String(err));
    } finally {
      setSubmitting(false);
    }
  };

  const doResolve = async (id: string) => {
    try {
      await resolveComment(props.projectId, id);
      await refetch();
    } catch (err) {
      setError(String(err));
    }
  };

  return (
    <div class="flex flex-col gap-2 p-2 text-sm">
      <div class="font-mono text-xs uppercase tracking-wider text-muted-foreground">
        Comments · {props.kind}:{props.nodeId}
      </div>

      <div class="flex flex-col gap-1 border border-border rounded p-2 bg-secondary/40">
        <textarea
          class="bg-background border border-border rounded px-2 py-1 text-sm min-h-[56px]"
          value={draft()}
          onInput={(e) => setDraft(e.currentTarget.value)}
          placeholder="Leave a review, observation, or question."
        />
        <div class="flex gap-1 text-[10px] font-mono">
          <input
            class="bg-background border border-border rounded px-2 py-0.5 flex-1"
            placeholder="property (optional)"
            value={property()}
            onInput={(e) => setProperty(e.currentTarget.value)}
          />
          <input
            class="bg-background border border-border rounded px-2 py-0.5 w-16"
            placeholder="L1"
            value={lineStart()}
            onInput={(e) => {
              const v = e.currentTarget.value;
              setLineStart(v === "" ? "" : Number.parseInt(v, 10));
            }}
          />
          <input
            class="bg-background border border-border rounded px-2 py-0.5 w-16"
            placeholder="L2"
            value={lineEnd()}
            onInput={(e) => {
              const v = e.currentTarget.value;
              setLineEnd(v === "" ? "" : Number.parseInt(v, 10));
            }}
          />
          <button
            type="button"
            class="ml-auto px-2 py-0.5 border border-border bg-accent text-background rounded hover:bg-accent/80 disabled:opacity-50"
            onClick={submit}
            disabled={submitting()}
          >
            {submitting() ? "…" : "post"}
          </button>
        </div>
        <Show when={error()}>
          <div class="text-xs text-signal-red">{error()}</div>
        </Show>
      </div>

      <div class="flex flex-col gap-1">
        <For each={comments() ?? []}>
          {(c) => <CommentRow comment={c} onResolve={() => doResolve(c.id)} />}
        </For>
        <Show when={(comments() ?? []).length === 0}>
          <div class="text-xs text-muted-foreground px-2 py-4">
            No {props.includeResolved ? "" : "unresolved "}comments.
          </div>
        </Show>
      </div>
    </div>
  );
};

const CommentRow: Component<{ comment: Comment; onResolve: () => void }> = (props) => {
  const c = () => props.comment;
  const resolved = () => !!c().resolved_at;
  const targetLabel = () => {
    const t = c().target;
    if (!t) return null;
    const parts: string[] = [];
    if (t.property) parts.push(t.property);
    if (t.line_start != null)
      parts.push(
        t.line_end != null && t.line_end !== t.line_start
          ? `L${t.line_start}-${t.line_end}`
          : `L${t.line_start}`,
      );
    return parts.join(":");
  };

  return (
    <div
      class={cn(
        "border rounded p-2 flex flex-col gap-1",
        resolved()
          ? "border-border/40 bg-secondary/20 opacity-60"
          : "border-border bg-background",
      )}
    >
      <div class="flex items-center gap-2 text-[10px] font-mono text-muted-foreground">
        <span class="text-foreground">@{c().author}</span>
        <span>·</span>
        <span>{c().posted_at.slice(0, 19).replace("T", " ")}</span>
        <Show when={targetLabel()}>
          <span class="px-1.5 py-[1px] border border-border rounded bg-secondary/60">
            {targetLabel()}
          </span>
        </Show>
        <span class="flex-1" />
        <Show when={!resolved()}>
          <button
            type="button"
            class="px-1.5 py-[1px] border border-border rounded hover:bg-accent/20"
            onClick={props.onResolve}
          >
            resolve
          </button>
        </Show>
        <Show when={resolved()}>
          <span class="text-signal-green">resolved</span>
        </Show>
      </div>
      <div class="text-sm whitespace-pre-wrap">{c().body}</div>
    </div>
  );
};

export default CommentsPanel;
