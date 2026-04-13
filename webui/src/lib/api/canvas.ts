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
