/**
 * API client. Calls the Rust control plane directly. Session cookies
 * are sent automatically (same domain in Railway). CSRF token is read
 * from the `csrf_token` cookie (set by the api on login) and echoed
 * back in the `X-CSRF-Token` header on every state-changing call.
 *
 * On 401 we redirect to /login (one-way; login completes the OAuth
 * round-trip and lands the user back here).
 */

const API_BASE = process.env.NEXT_PUBLIC_API_URL ?? "";

function readCsrfCookie(): string {
  if (typeof document === "undefined") return "";
  const pair = document.cookie
    .split("; ")
    .find((c) => c.startsWith("csrf_token="));
  return pair ? decodeURIComponent(pair.split("=")[1]) : "";
}

export class ApiError extends Error {
  constructor(
    public status: number,
    public path: string,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

async function request<T>(
  path: string,
  init: RequestInit = {},
): Promise<T> {
  const headers = new Headers(init.headers);
  if (!headers.has("Accept")) headers.set("Accept", "application/json");

  const method = (init.method ?? "GET").toUpperCase();
  const mutating = method !== "GET" && method !== "HEAD";

  if (mutating) {
    headers.set("X-CSRF-Token", readCsrfCookie());
    if (init.body && !headers.has("Content-Type")) {
      headers.set("Content-Type", "application/json");
    }
  }

  const res = await fetch(`${API_BASE}${path}`, {
    ...init,
    headers,
    credentials: "include",
    cache: "no-store",
  });

  if (res.status === 401 && typeof window !== "undefined") {
    // Soft redirect to login; the api/me query will retry post-login.
    if (window.location.pathname !== "/login") {
      window.location.href = "/login";
    }
  }

  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new ApiError(res.status, path, `API ${res.status}: ${text || res.statusText}`);
  }

  if (res.status === 204) return undefined as unknown as T;
  return (await res.json()) as T;
}

export const api = {
  get: <T>(path: string) => request<T>(path),
  post: <T>(path: string, body?: unknown) =>
    request<T>(path, { method: "POST", body: body ? JSON.stringify(body) : undefined }),
  delete: <T>(path: string) => request<T>(path, { method: "DELETE" }),
};
