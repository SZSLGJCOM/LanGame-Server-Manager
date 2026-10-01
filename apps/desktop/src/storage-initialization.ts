import type { StorageStatus } from "./types";

export type StorageInitializationState =
  | { status: "pending"; attempt: number }
  | { status: "ready"; attempt: number }
  | {
      status: "failed";
      attempt: number;
      error: string;
      databasePath: string | null;
      logPath: string | null;
      logDirectory: string | null;
    };

export type StorageInitializationEvent =
  | { type: "retry" }
  | { type: "ready"; attempt: number }
  | {
      type: "failed";
      attempt: number;
      error: string;
      storage: StorageStatus | null;
    };

export type StorageInitializationSurface = "loading" | "failure" | "content";

export function createStorageInitializationState(): StorageInitializationState {
  return { status: "pending", attempt: 0 };
}

export function reduceStorageInitializationState(
  state: StorageInitializationState,
  event: StorageInitializationEvent
): StorageInitializationState {
  if (event.type === "retry") {
    return state.status === "failed"
      ? { status: "pending", attempt: state.attempt + 1 }
      : state;
  }

  if (state.status !== "pending" || event.attempt !== state.attempt) {
    return state;
  }

  if (event.type === "ready") {
    return { status: "ready", attempt: state.attempt };
  }

  const logPath = normalizePath(event.storage?.app_log_path);
  return {
    status: "failed",
    attempt: state.attempt,
    error: event.error,
    databasePath: normalizePath(event.storage?.database_path),
    logPath,
    logDirectory: resolveParentDirectory(logPath)
  };
}

export function resolveParentDirectory(filePath: string | null | undefined): string | null {
  const normalized = normalizePath(filePath);
  if (!normalized) {
    return null;
  }

  const separatorIndex = Math.max(normalized.lastIndexOf("/"), normalized.lastIndexOf("\\"));
  if (separatorIndex < 0) {
    return null;
  }
  if (separatorIndex === 0) {
    return normalized[0];
  }
  if (separatorIndex === 2 && /^[A-Za-z]:[\\/]/.test(normalized)) {
    return normalized.slice(0, 3);
  }

  return normalized.slice(0, separatorIndex);
}

export function resolveStorageInitializationSurface(
  state: StorageInitializationState
): StorageInitializationSurface {
  switch (state.status) {
    case "pending":
      return "loading";
    case "failed":
      return "failure";
    case "ready":
      return "content";
  }
}

function normalizePath(value: string | null | undefined): string | null {
  const normalized = value?.trim();
  return normalized ? normalized : null;
}
