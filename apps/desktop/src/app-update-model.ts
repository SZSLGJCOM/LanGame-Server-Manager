import type { AppUpdateCheckResult, AppUpdateInstallEvent, AppUpdateState } from "./types";

export function appUpdateReleaseUrl(version: string | null | undefined): string | null {
  // Release preparation supports stable versions and publishes exact v<version> tags.
  if (!version || version.length > 64 || version !== version.trim()
    || !/^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/.test(version)) return null;
  return `https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/tag/v${version}`;
}

export function createInitialAppUpdateState(currentVersion: string): AppUpdateState {
  return {
    status: "idle",
    currentVersion,
    downloadedBytes: 0,
    downloadPercent: 0
  };
}

export function applyAppUpdateCheckResult(state: AppUpdateState, result: AppUpdateCheckResult): AppUpdateState {
  if (result.status === "available") {
    return {
      ...state,
      status: "available",
      availableVersion: result.update.version,
      releaseNotes: result.update.body ?? null,
      publishedAt: result.update.date ?? null,
      error: null
    };
  }

  return {
    ...state,
    status: "current",
    currentVersion: result.currentVersion ?? result.current_version ?? state.currentVersion,
    availableVersion: null,
    releaseNotes: null,
    publishedAt: null,
    error: null
  };
}

export function reduceAppUpdateInstallEvent(state: AppUpdateState, event: AppUpdateInstallEvent): AppUpdateState {
  if (event.event === "started") {
    const contentLength = event.data.contentLength ?? event.data.content_length ?? null;
    return { ...state, status: "downloading", contentLength, downloadedBytes: 0, downloadPercent: 0 };
  }

  if (event.event === "progress") {
    const chunkLength = event.data.chunkLength ?? event.data.chunk_length ?? 0;
    const downloadedBytes = state.downloadedBytes + Math.max(0, chunkLength);
    const downloadPercent = state.contentLength && state.contentLength > 0
      ? Math.min(100, Math.round((downloadedBytes / state.contentLength) * 100))
      : state.downloadPercent;
    return { ...state, status: "downloading", downloadedBytes, downloadPercent };
  }

  if (event.event === "finished") {
    return { ...state, status: "downloading", downloadPercent: 100 };
  }

  return { ...state, status: "installing", downloadPercent: 100 };
}

export function failAppUpdateState(state: AppUpdateState, error: unknown): AppUpdateState {
  const message = error instanceof Error ? error.message : String(error);
  return { ...state, status: "failed", error: message || "Application update failed." };
}
