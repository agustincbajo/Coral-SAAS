/**
 * API client stub.
 *
 * Reads `NEXT_PUBLIC_API_URL` from env (set by Railway via shared variable
 * reference to the `api` service URL). Wraps fetch with sane defaults
 * (credentials: include for cookies, content-type, error throwing).
 */

const API_URL = process.env.NEXT_PUBLIC_API_URL ?? "http://localhost:8080";

export async function apiGet<T>(path: string): Promise<T> {
  const res = await fetch(`${API_URL}${path}`, {
    credentials: "include",
    headers: { Accept: "application/json" },
  });
  if (!res.ok) throw new Error(`API ${res.status}: ${path}`);
  return res.json() as Promise<T>;
}

export async function apiPost<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`${API_URL}${path}`, {
    method: "POST",
    credentials: "include",
    headers: {
      Accept: "application/json",
      "Content-Type": "application/json",
      // CSRF double-submit token — frontend reads from non-HttpOnly cookie
      // and echoes here. Wire up the cookie in middleware later.
      "X-CSRF-Token":
        typeof document !== "undefined"
          ? document.cookie
              .split("; ")
              .find((c) => c.startsWith("csrf_token="))
              ?.split("=")[1] ?? ""
          : "",
    },
    body: JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`API ${res.status}: ${path}`);
  return res.json() as Promise<T>;
}
