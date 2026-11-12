import type { SteamWorkshopLookupItem } from "../../types";
import { isWorkshopDownloadPending, type WorkshopDownloadRequestState } from "./workshop-download-operations";

export interface WorkshopDownloadItemPresentation {
  state: "queued" | "installing" | "ready" | "applying";
  progressPercent: number | null;
}

export function workshopRequestPresentation(request: WorkshopDownloadRequestState): WorkshopDownloadItemPresentation {
  const state = request.status === "preparing" ? "installing" : request.status === "ready" ? "ready"
    : request.status === "applying" ? "applying" : "queued";
  const progress = request.job?.progress_percent;
  return { state, progressPercent: (state === "installing" || state === "applying") && typeof progress === "number" && Number.isFinite(progress)
    ? Math.max(0, Math.min(100, Math.round(progress))) : null };
}

/** Each item takes progress from its own earliest unfinished request. */
export function workshopDownloadPresentation(requests: WorkshopDownloadRequestState[], locale: string) {
  const active = requests.filter(isWorkshopDownloadPending);
  const byId: Record<string, WorkshopDownloadItemPresentation> = {};
  const latestItems = new Map<string, WorkshopDownloadRequestState>();
  const items: Record<string, SteamWorkshopLookupItem> = {};
  for (const request of requests) {
    for (const id of request.contentIds) latestItems.set(id, request);
    if (request.locale === locale) Object.assign(items, request.items);
  }
  // A later overlapping collection cannot relabel the item currently deploying.
  for (const request of [...active].reverse()) {
    for (const id of request.ids) byId[id] = workshopRequestPresentation(request);
    for (const id of request.contentIds) latestItems.set(id, request);
  }
  const installingIds = new Set(Object.keys(byId).filter((id) => byId[id].state !== "queued"));
  const queuedIds = new Set(Object.keys(byId).filter((id) => byId[id].state === "queued"));
  const pending = [...latestItems].filter(([, request]) => request.status !== "success");
  return {
    byId, installingIds, queuedIds, items,
    busy: active.length > 0,
    ids: [...new Set(requests.flatMap((request) => request.ids))],
    contentIds: pending.map(([id]) => id),
    errors: Object.fromEntries(pending.flatMap(([id, request]) => request.status === "error" && request.error ? [[id, request.error]] : [])),
    completedKey: requests.filter((request) => request.status === "success" || request.status === "error")
      .map((request) => request.requestId).join(",")
  };
}
