import { apiFetch, parseJson } from "@/lib/api/core";
import type {
  CodexDevicePollResponse,
  CodexDeviceStartResponse,
  SettingsResponse,
} from "@/lib/api/types";

export async function getSettings(): Promise<SettingsResponse> {
  const res = await apiFetch("/settings");
  return parseJson<SettingsResponse>(res);
}

export async function saveSettingsProvider(
  provider: "codex" | "openrouter",
): Promise<void> {
  const res = await apiFetch("/settings/provider", {
    method: "POST",
    body: JSON.stringify({ provider }),
  });
  await parseJson<{ ok: true }>(res);
}

export async function saveRoleModels(data: {
  shepherd: { model: string | null; model_variant: string | null };
  librarian: { model: string | null; model_variant: string | null };
  thread: { model: string | null; model_variant: string | null };
  search: { model: string | null; model_variant: string | null };
}): Promise<void> {
  const res = await apiFetch("/settings/models", {
    method: "POST",
    body: JSON.stringify(data),
  });
  await parseJson<{ ok: true }>(res);
}

export async function saveOpenRouterSettings(data: {
  api_key?: string;
  base_url?: string;
}): Promise<void> {
  const res = await apiFetch("/settings/openrouter", {
    method: "POST",
    body: JSON.stringify(data),
  });
  await parseJson<{ ok: true }>(res);
}

export async function startCodexDeviceFlow(): Promise<CodexDeviceStartResponse> {
  const res = await apiFetch("/settings/provider/codex/device/start", {
    method: "POST",
  });
  return parseJson<CodexDeviceStartResponse>(res);
}

export async function pollCodexDeviceFlow(data: {
  deviceAuthId: string;
  userCode: string;
}): Promise<CodexDevicePollResponse> {
  const res = await apiFetch("/settings/provider/codex/device/poll", {
    method: "POST",
    body: JSON.stringify(data),
  });
  return parseJson<CodexDevicePollResponse>(res);
}

export async function saveTavilyKey(api_key: string): Promise<void> {
  const res = await apiFetch("/settings/tavily", {
    method: "POST",
    body: JSON.stringify({ api_key }),
  });
  await parseJson<{ ok: true }>(res);
}

export async function saveGithubToken(token: string): Promise<void> {
  const res = await apiFetch("/settings/github", {
    method: "POST",
    body: JSON.stringify({ token }),
  });
  await parseJson<{ ok: true }>(res);
}
