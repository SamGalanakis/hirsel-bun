import { apiFetch, parseJson } from "@/lib/api/core";
import type { Comment, CommentTarget } from "@/lib/api/types";

export async function listNodeComments(
  projectId: number,
  kind: string,
  nodeId: string,
  opts: { limit?: number; onlyUnresolved?: boolean } = {},
): Promise<Comment[]> {
  const params = new URLSearchParams();
  if (opts.limit != null) params.set("limit", String(opts.limit));
  if (opts.onlyUnresolved != null)
    params.set("only_unresolved", opts.onlyUnresolved ? "true" : "false");
  const suffix = params.toString();
  const path = `/projects/${projectId}/nodes/${encodeURIComponent(kind)}/${encodeURIComponent(
    nodeId,
  )}/comments${suffix ? `?${suffix}` : ""}`;
  const res = await apiFetch(path);
  return parseJson<Comment[]>(res);
}

export async function addNodeComment(
  projectId: number,
  kind: string,
  nodeId: string,
  body: string,
  target?: CommentTarget,
  author?: string,
): Promise<Comment> {
  const res = await apiFetch(
    `/projects/${projectId}/nodes/${encodeURIComponent(kind)}/${encodeURIComponent(nodeId)}/comments`,
    {
      method: "POST",
      body: JSON.stringify({ body, target, author }),
    },
  );
  return parseJson<Comment>(res);
}

export async function resolveComment(
  projectId: number,
  commentId: string,
): Promise<Comment> {
  const res = await apiFetch(
    `/projects/${projectId}/comments/${encodeURIComponent(commentId)}/resolve`,
    { method: "POST" },
  );
  return parseJson<Comment>(res);
}
