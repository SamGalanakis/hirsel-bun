import { apiFetch, parseJson } from "@/lib/api/core";
import type { Task } from "@/lib/api/types";

export async function listTasks(projectId: number): Promise<Task[]> {
  const res = await apiFetch(`/projects/${projectId}/tasks`);
  return parseJson<Task[]>(res);
}

export async function createTask(
  projectId: number,
  data: { title: string; content?: string },
): Promise<Task> {
  const res = await apiFetch(`/projects/${projectId}/tasks`, {
    method: "POST",
    body: JSON.stringify(data),
  });
  return parseJson<Task>(res);
}

export async function updateTask(
  projectId: number,
  taskId: string,
  data: { title?: string; status?: string; content?: string },
): Promise<Task> {
  const res = await apiFetch(`/projects/${projectId}/tasks/${taskId}`, {
    method: "PATCH",
    body: JSON.stringify(data),
  });
  return parseJson<Task>(res);
}

export async function deleteTask(
  projectId: number,
  taskId: string,
): Promise<void> {
  const res = await apiFetch(`/projects/${projectId}/tasks/${taskId}`, {
    method: "DELETE",
  });
  await parseJson<{ ok: true }>(res);
}

export async function reorderTasks(
  projectId: number,
  taskIds: string[],
): Promise<void> {
  const res = await apiFetch(`/projects/${projectId}/tasks/reorder`, {
    method: "POST",
    body: JSON.stringify({ task_ids: taskIds }),
  });
  await parseJson<{ ok: true }>(res);
}
