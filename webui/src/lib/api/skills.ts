import { apiFetch, parseJson } from "@/lib/api/core";
import type { SkillSummary } from "@/lib/api/types";

export async function listProjectSkills(projectId: number): Promise<SkillSummary[]> {
  const res = await apiFetch(`/projects/${projectId}/skills`);
  return parseJson<SkillSummary[]>(res);
}
