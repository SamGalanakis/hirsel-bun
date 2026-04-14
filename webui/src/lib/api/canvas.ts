import { apiFetch, parseJson } from "@/lib/api/core";
import type { CanvasView, CanvasPosition } from "@/lib/api/types";

export async function getCanvas(projectId: number): Promise<CanvasView> {
  const res = await apiFetch(`/projects/${projectId}/canvas`);
  return parseJson<CanvasView>(res);
}

export async function patchCanvasLayout(
  projectId: number,
  positions: Record<string, CanvasPosition>,
): Promise<void> {
  const res = await apiFetch(`/projects/${projectId}/canvas/layout`, {
    method: "PATCH",
    body: JSON.stringify(positions),
  });
  await parseJson<{ ok: true }>(res);
}

export async function resetCanvasLayout(projectId: number): Promise<void> {
  const res = await apiFetch(`/projects/${projectId}/canvas/layout`, {
    method: "DELETE",
  });
  await parseJson<{ ok: true }>(res);
}

export async function deleteCanvasNode(
  projectId: number,
  kind: string,
  nodeId: string,
): Promise<void> {
  const res = await apiFetch(
    `/projects/${projectId}/canvas/node/${encodeURIComponent(kind)}/${encodeURIComponent(nodeId)}`,
    { method: "DELETE" },
  );
  await parseJson<{ ok: true }>(res);
}

export async function createCanvasNode(
  projectId: number,
  data: { kind: string; title: string; content?: string },
): Promise<{ ok: true; id: string; kind: string }> {
  const res = await apiFetch(`/projects/${projectId}/canvas/node`, {
    method: "POST",
    body: JSON.stringify(data),
  });
  return parseJson(res);
}

export interface CompanionAction {
  action: string;
  payload: Record<string, unknown>;
}

export async function drainCompanionActions(
  projectId: number,
): Promise<CompanionAction[]> {
  const res = await apiFetch(`/projects/${projectId}/companion/actions`);
  const body = await parseJson<{ actions: CompanionAction[] }>(res);
  return body.actions ?? [];
}

export async function updateCanvasNode(
  projectId: number,
  kind: string,
  nodeId: string,
  data: { title?: string; content?: string; status?: string },
): Promise<void> {
  const res = await apiFetch(
    `/projects/${projectId}/canvas/node/${encodeURIComponent(kind)}/${encodeURIComponent(nodeId)}`,
    { method: "PATCH", body: JSON.stringify(data) },
  );
  await parseJson<{ ok: true }>(res);
}
