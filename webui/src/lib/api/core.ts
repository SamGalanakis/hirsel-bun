export class ApiError extends Error {
  status: number;

  constructor(message: string, status: number) {
    super(message);
    this.status = status;
  }
}

export async function apiFetch(path: string, init: RequestInit = {}): Promise<Response> {
  const headers = new Headers(init.headers || {});
  if (init.body && !(init.body instanceof FormData) && !headers.has("content-type")) {
    headers.set("content-type", "application/json");
  }
  return fetch(`/api${path}`, { ...init, headers, credentials: "same-origin" });
}

export function apiUrl(path: string): string {
  return `/api${path}`;
}

export async function parseJson<T>(res: Response): Promise<T> {
  const raw = await res.text();
  const body = raw.trim()
    ? (() => {
        try {
          return JSON.parse(raw);
        } catch {
          return {};
        }
      })()
    : {};
  if (!res.ok) {
    const message =
      typeof body?.message === "string"
        ? body.message
        : raw.trim() || res.statusText;
    throw new ApiError(message || `Request failed (${res.status})`, res.status);
  }
  return body as T;
}
