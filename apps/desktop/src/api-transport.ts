import { invoke, isTauri } from "@tauri-apps/api/core";
import { desktopExitLifecycle, FINAL_EXIT_ERROR } from "./desktop-exit-lifecycle";

const LAN_MANAGEMENT_TOKEN_STORAGE_KEY = "langameLanToken";

export async function invokeOrMock<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri()) {
    if (desktopExitLifecycle.getSnapshot().requested) throw new Error(FINAL_EXIT_ERROR);
    try {
      return await invoke<T>(command, args);
    } catch (error) {
      await desktopExitLifecycle.observeOperationError(error);
      throw error;
    }
  }
  if (shouldUseLanApi()) return invokeLanApi<T>(command, args);

  // Preview data is a development capability, never a fallback for a missing
  // management token or an unavailable production backend.
  if (import.meta.env.DEV) {
    const { invokeMock } = await import("./api-mock");
    return invokeMock<T>(command, args);
  }
  throw new Error("LanGame requires its desktop host or an HTTP(S) management connection.");
}

export function shouldUseLanApi(): boolean {
  if (typeof window === "undefined") return false;
  const { hostname, protocol } = window.location;
  if (protocol !== "http:" && protocol !== "https:") return false;

  const hasLanToken = readLanApiToken() !== null;
  const isLoopbackHost = ["localhost", "127.0.0.1", "::1", "[::1]"].includes(hostname);
  return !import.meta.env.DEV || hasLanToken || !isLoopbackHost;
}

async function invokeLanApi<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const token = readLanApiToken();
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (token) headers["X-LanGame-Token"] = token;

  const response = await fetch("/__langame/api", {
    method: "POST",
    headers,
    credentials: "omit",
    redirect: "error",
    cache: "no-store",
    body: JSON.stringify({ command, args: args ?? {} })
  });
  const payload: unknown = await response.json().catch(() => null);
  if (!payload || typeof payload !== "object" || !("ok" in payload)) {
    throw new Error(`Invalid LanGame LAN API response: ${response.status}`);
  }
  if (!response.ok || payload.ok !== true) {
    const error = "error" in payload && typeof payload.error === "string" ? payload.error : null;
    throw new Error(error ?? `LanGame LAN API request failed: ${response.status}`);
  }
  if (!("value" in payload)) throw new Error("LanGame LAN API response is missing its result.");
  return payload.value as T;
}

function readLanApiToken(): string | null {
  const fragment = new URLSearchParams(window.location.hash.replace(/^#/, ""));
  const tokenFromUrl = fragment.get("langameToken");
  if (tokenFromUrl) {
    window.sessionStorage.setItem(LAN_MANAGEMENT_TOKEN_STORAGE_KEY, tokenFromUrl);
    const cleanUrl = `${window.location.pathname}${window.location.search}` || "/";
    window.history.replaceState(null, "", cleanUrl);
    return tokenFromUrl;
  }
  return window.sessionStorage.getItem(LAN_MANAGEMENT_TOKEN_STORAGE_KEY);
}
