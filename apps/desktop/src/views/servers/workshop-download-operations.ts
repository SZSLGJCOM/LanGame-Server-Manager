import { isActiveJobStatus } from "../../app-state";
import type { BackgroundJob, SteamWorkshopLookupItem } from "../../types";

export interface WorkshopDownloadState {
  requestId: number;
  status: "idle" | "running" | "success" | "error";
  ids: string[];
  contentIds: string[];
  items: Record<string, SteamWorkshopLookupItem>;
  locale: string;
  error: string | null;
  job: BackgroundJob | null;
  excludedJobIds: string[] | null;
}

const IDLE: WorkshopDownloadState = {
  requestId: 0, status: "idle", ids: [], contentIds: [], items: {}, locale: "", error: null, job: null, excludedJobIds: null
};

export function activeWorkshopDownloadJob(jobs: BackgroundJob[], instanceId: string, excludedJobIds: string[] = []): BackgroundJob | null {
  return jobs.find((job) => job.target_id === instanceId && job.id.startsWith("workshop-download:") &&
    !excludedJobIds.includes(job.id) &&
    ["downloadworkshop", "download_workshop"].includes(job.kind.toLowerCase()) && isActiveJobStatus(job.status)) ?? null;
}

/** The app owns accepted requests; a keyed Mod page is only a subscriber. */
export class WorkshopDownloadOperations {
  private sequence = 0;
  private readonly states = new Map<string, WorkshopDownloadState>();
  private readonly tokens = new Map<string, symbol>();
  private readonly listeners = new Set<() => void>();

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  };

  snapshot(instanceId: string): WorkshopDownloadState { return this.states.get(instanceId) ?? IDLE; }

  private publish(instanceId: string, state: WorkshopDownloadState) {
    this.states.delete(instanceId);
    this.states.set(instanceId, state);
    // Retain a bounded session history without evicting accepted operations.
    for (const [id, entry] of this.states) {
      if (this.states.size <= 32) break;
      if (entry.status !== "running") this.states.delete(id);
    }
    for (const listener of this.listeners) listener();
  }

  begin(instanceId: string, request: Pick<WorkshopDownloadState, "ids" | "contentIds" | "items" | "locale">) {
    if (this.snapshot(instanceId).status === "running") return null;
    const token = Symbol(instanceId);
    this.tokens.set(instanceId, token);
    this.publish(instanceId, { ...IDLE, ...request, requestId: ++this.sequence, status: "running" });
    const update = (patch: Partial<WorkshopDownloadState>) => {
      if (this.tokens.get(instanceId) !== token) return;
      this.publish(instanceId, { ...this.snapshot(instanceId), ...patch });
    };
    const finish = (status: "success" | "error", error: string | null) => {
      update({ status, error, job: null });
      if (this.tokens.get(instanceId) === token) this.tokens.delete(instanceId);
    };
    return {
      // Capture existing jobs immediately before each native attempt, including
      // account retries. Waiting for a lock must not borrow another task's progress.
      trackJobs: (existing: BackgroundJob[]) => update({ requestId: ++this.sequence, excludedJobIds: existing.map((job) => job.id), job: null }),
      succeed: () => finish("success", null),
      fail: (error: string) => finish("error", error)
    };
  }

  recordJob(instanceId: string, requestId: number, job: BackgroundJob) {
    const current = this.snapshot(instanceId);
    if (current.requestId !== requestId || current.status !== "running" || !current.excludedJobIds ||
      !activeWorkshopDownloadJob([job], instanceId, current.excludedJobIds) || (current.job && current.job.id !== job.id)) return;
    if (current.job?.id === job.id && current.job.status === job.status &&
      current.job.progress_percent === job.progress_percent && current.job.detail === job.detail) return;
    this.publish(instanceId, { ...current, job });
  }
}
