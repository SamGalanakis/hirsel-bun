# Recursive decomposition with `chunk_subgraph`

When a coordinator thread needs to reason over a knowledge-graph
neighbourhood that's too large to fit in one context window, the pattern
is **chunk → spawn-batch → reduce**. All three steps are ordinary tool
calls — no lashlang, no RLM mode, no special runtime semantics.

## The primitives

- `graph.chunk_subgraph(kind, node_id, max_tokens?)` — BFS from the root,
  emits token-bounded chunks plus cross-chunk references so children know
  there's more context next door.
- `spawn_thread_batch(requests[])` — fan out one child per chunk. Each
  child is a real thread row with `parent_id` set to the coordinator.
- `await_threads(thread_ids[])` — collect the children's final_output.
- `spawn_thread(...)` — a single child for the reducer step.

## Canonical flow

```
# 1. Split.
chunks = graph.chunk_subgraph(kind: "document", node_id: "auth-overview")
# -> { count: 4, chunks: [{index: 0, nodes: [...], edges: [...], cross_refs: [...]}, ...] }

# 2. Spawn one child per chunk.
requests = []
for chunk in chunks:
    requests.append({
        objective: "Summarise the following subgraph chunk. Cross-refs "
                   "point to other chunks; mention them but don't follow.\n"
                   + json.dumps(chunk),
        title: f"summarise-chunk-{chunk.index}",
        capabilities: ["graph_read"],
    })
handles = spawn_thread_batch(requests)
# -> [{thread_id: "...", ...}, ...]

# 3. Await all children.
summaries = await_threads(
    thread_ids=[h.thread_id for h in handles.handles],
    timeout_ms=120000,
)
# -> [{state: "done", thread_id, final_output}, ...]

# 4. Reduce.
reducer = spawn_thread({
    objective: "Merge these subgraph summaries into one cohesive view. "
               "Reconcile contradictions. Flag gaps.\n"
               + json.dumps([s.final_output for s in summaries.results]),
    title: "reduce",
    capabilities: ["graph_read"],
})
final = await_thread(reducer.thread_id)
# -> {state: "done", final_output: "…"}
```

## Why not just fetch the whole subgraph?

Three reasons:

1. **Token budgets.** A 400-node subgraph with full content can exceed
   any single context. Chunking keeps each child's view bounded.
2. **Parallelism.** Children run concurrently. A 4-chunk problem
   completes in ~1x child-turn duration, not 4x.
3. **Partial-context reasoning.** Children are forced to describe their
   slice without assuming global knowledge. The reducer's job is
   reconciliation. This matches how humans with specialised knowledge
   collaborate on large artefacts.

## Cross-references

Each chunk carries `cross_refs: [{from, relation, to, other_chunk}]`.
These are edges whose endpoints land in different chunks. Children see
them as "there's a related node, but its details are in chunk N" and
should name the connection without following it. The reducer can use
`other_chunk` to align summaries.

## Settings

- `graph.chunk.max_tokens` — default max tokens per chunk (30 000).
  Override per-call via `max_tokens`.
- `thread.spawn.max_children_per_turn` — cap on batch size. If your
  subgraph exceeds this, either raise the cap or chunk with a larger
  `max_tokens`.
- `thread.await.default_timeout_ms` — how long to wait before
  `await_thread` returns `pending`. Re-await pending handles to collect
  late children.

## When to use

Use this pattern when:

- A single `search_context` would return too much to act on.
- You want independent partial analyses (e.g. per-module code review).
- A task decomposes naturally along the graph's topology (components,
  decisions, files grouped by concern).

Don't use it for:

- Small subgraphs that fit in one turn — use `search_context` directly.
- Problems that require global state from step 1 — chunking hides
  cross-chunk structure from children by design.
- Free-form exploration where the "root" isn't obvious — spawning
  blindly wastes turns.

## Failure modes

- **Child fails to complete.** `await_thread` returns
  `state: "pending"` past timeout. Re-await or investigate with
  `inspect_thread`.
- **Merge conflicts in reducer's workspace.** If the reducer has
  `workspace_write`, use the normal merge lifecycle
  (`inspect_thread` → `merge_thread` / `discard_thread`).
- **Token budget too tight.** Chunks become too fragmented, children
  lose context. Raise `max_tokens` or pre-filter the subgraph.

## Related primitives

- `graph.comment` / `graph.comments` — children can leave structured
  reviews on nodes they inspected; the reducer reads them for free via
  the per-iteration comment prompt injection.
- `kg_read` — every node a child surfaces via `search_context`
  auto-appends a read-mark, visible to other threads.
