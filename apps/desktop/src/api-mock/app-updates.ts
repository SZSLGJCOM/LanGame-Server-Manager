import { version as desktopVersion } from "../../package.json";
import type { AppUpdateCheckResult, AppUpdateInstallEvent } from "../types";

export function buildMockAppUpdateCheckResult(): AppUpdateCheckResult {
  return {
    status: "available",
    update: {
      version: "0.2.0",
      currentVersion: desktopVersion,
      body: "LanGame desktop update preview.",
      date: "2026-06-05T00:00:00Z"
    }
  };
}

export function emitMockAppUpdateInstallEvents(onEvent: unknown): void {
  if (typeof onEvent !== "function") {
    return;
  }

  const events: AppUpdateInstallEvent[] = [
    { event: "started", data: { content_length: 1024 } },
    { event: "progress", data: { chunk_length: 1024 } },
    { event: "finished" },
    { event: "installing" }
  ];
  events.forEach((event) => onEvent(event));
}
