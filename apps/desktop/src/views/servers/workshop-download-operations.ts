import { isActiveJobStatus } from "../../app-state";
import type { BackgroundJob, SteamWorkshopLookupItem } from "../../types";

export interface WorkshopDownloadRequest {
  ids: string[];
  contentIds: string[];
  items: Record<string, SteamWorkshopLookupItem>;
  locale: string;
}

export interface WorkshopDownloadRequestState extends WorkshopDownloadRequest {
  requestId: number;
  attemptToken: string | null;
  jobId: string | null;
  status: "queued" | "preparing" | "ready" | "applying" | "success" | "error";
  error: string | null;
  job: BackgroundJob | null;
}

export interface WorkshopDownloadState { requests: WorkshopDownloadRequestState[]; }

export interface WorkshopDownloadOperation {
  beginAttempt(): string;
  apply(operation: () => Promise<void>): Promise<void>;
}
export type WorkshopDownloadExecutor = (operation: WorkshopDownloadOperation) => Promise<void>;

interface QueuedDownload {
  instanceId: string;
  requestId: number;
  execute: WorkshopDownloadExecutor;
  predecessor: Promise<void>;
  settled(): void;
  resolve(): void;
  reject(error: unknown): void;
}

const IDLE: WorkshopDownloadState = { requests: [] };
export const isWorkshopDownloadPending = (request: WorkshopDownloadRequestState) =>
  request.status !== "success" && request.status !== "error";

function createAttemptToken(): string {
  if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
  // LAN HTTP pages expose secure random bytes but may not expose randomUUID.
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0"));
  return `${hex.slice(0, 4).join("")}-${hex.slice(4, 6).join("")}-${hex.slice(6, 8).join("")}-${hex.slice(8, 10).join("")}-${hex.slice(10).join("")}`;
}

export function workshopDownloadOwner(request: WorkshopDownloadRequest): string {
  return `workshop-download:${[...new Set(request.ids)].sort().join(",")}`;
}

export function activeWorkshopDownloadJob(jobs: BackgroundJob[], instanceId: string): BackgroundJob | null {
  return jobs.find((job) => job.target_id === instanceId && job.id.startsWith("workshop-download:") &&
    ["downloadworkshop", "download_workshop"].includes(job.kind.toLowerCase()) && isActiveJobStatus(job.status)) ?? null;
}

/** The app owns accepted requests; a keyed Mod page is only a subscriber. */
export class WorkshopDownloadOperations {
  private sequence = 0;
  private readonly states = new Map<string, WorkshopDownloadState>();
  private readonly queue: QueuedDownload[] = [];
  private readonly commitTails = new Map<string, Promise<void>>();
  private running = 0;
  private readonly failedExecutors = new Map<number, WorkshopDownloadExecutor>();
  private readonly listeners = new Set<() => void>();

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  };

  snapshot(instanceId: string): WorkshopDownloadState { return this.states.get(instanceId) ?? IDLE; }

  private publish(instanceId: string, state: WorkshopDownloadState) {
    const successful = state.requests.filter((request) => request.status === "success");
    const retained = new Set(successful.slice(-32).map((request) => request.requestId));
    this.states.delete(instanceId);
    this.states.set(instanceId, { requests: state.requests.filter((request) => request.status !== "success" || retained.has(request.requestId)) });
    // Failed requests remain visible for retry while the start barrier retains
    // their error. Only successful history may be evicted.
    for (const [id, entry] of this.states) {
      if (this.states.size <= 32) break;
      if (entry.requests.every((request) => request.status === "success")) this.states.delete(id);
    }
    for (const listener of this.listeners) listener();
  }

  enqueue(instanceId: string, request: WorkshopDownloadRequest, execute: WorkshopDownloadExecutor): Promise<void> | null {
    const current = this.snapshot(instanceId).requests;
    const owner = workshopDownloadOwner(request);
    const retryingFailed = current.some((entry) => entry.status === "error" && workshopDownloadOwner(entry) === owner);
    const pendingIds = new Set(current.filter(isWorkshopDownloadPending).flatMap((entry) => entry.ids));
    // Individual members may already be queued, but retrying a failed batch
    // must still finish that batch's original configuration/enablement intent.
    if (!retryingFailed && request.ids.every((id) => pendingIds.has(id))) return null;
    const requestId = ++this.sequence;
    const predecessor = this.commitTails.get(instanceId) ?? Promise.resolve();
    let settled!: () => void;
    const finished = new Promise<void>((resolve) => { settled = resolve; });
    // A failed preparation may finish before its predecessor. Keep that earlier
    // request in the ordering chain so a third request cannot overtake it.
    const orderedCompletion = Promise.all([predecessor, finished]).then(() => undefined);
    this.commitTails.set(instanceId, orderedCompletion);
    void orderedCompletion.then(() => {
      if (this.commitTails.get(instanceId) === orderedCompletion) this.commitTails.delete(instanceId);
    });
    const completion = new Promise<void>((resolve, reject) => {
      this.queue.push({ instanceId, requestId, execute, predecessor, settled, resolve, reject });
    });
    const retained: WorkshopDownloadRequestState[] = [];
    for (const entry of current) {
      if (isWorkshopDownloadPending(entry) || workshopDownloadOwner(entry) !== owner) retained.push(entry);
      else this.failedExecutors.delete(entry.requestId);
    }
    this.publish(instanceId, { requests: [
      ...retained,
      { ...request, ids: [...request.ids], contentIds: [...request.contentIds], items: { ...request.items },
        requestId, attemptToken: null, jobId: null, status: "queued", error: null, job: null }
    ] });
    // Accept synchronously, then give the caller time to register the entire
    // queued operation with the instance start barrier before any work begins.
    void Promise.resolve().then(() => this.pump());
    return completion;
  }

  retry(instanceId: string, requestId: number): Promise<void> | null {
    const request = this.snapshot(instanceId).requests.find((entry) => entry.requestId === requestId && entry.status === "error");
    const execute = this.failedExecutors.get(requestId);
    if (!request || !execute) return null;
    return this.enqueue(instanceId, request, execute);
  }

  private update(instanceId: string, requestId: number, patch: Partial<WorkshopDownloadRequestState>) {
    this.publish(instanceId, { requests: this.snapshot(instanceId).requests.map((request) =>
      request.requestId === requestId ? { ...request, ...patch } : request) });
  }

  private pump() {
    // Bound whole requests, including ready tickets and configuration saves.
    // Global admission order prevents later ready work from occupying every
    // slot while an earlier request still waits to start preparing.
    while (this.running < 2 && this.queue.length) {
      this.running += 1;
      void this.execute(this.queue.shift()!);
    }
  }

  private async execute(entry: QueuedDownload) {
    const { instanceId, requestId } = entry;
    this.update(instanceId, requestId, { status: "preparing" });
    const operation: WorkshopDownloadOperation = {
      beginAttempt: () => {
        const current = this.snapshot(instanceId).requests.find((request) => request.requestId === requestId);
        if (!current || !isWorkshopDownloadPending(current)) throw new Error("Workshop request is no longer active");
        const attemptToken = createAttemptToken();
        this.update(instanceId, requestId, { attemptToken,
          jobId: `workshop-download:${instanceId}:${attemptToken}`, job: null });
        return attemptToken;
      },
      apply: async (apply) => {
        this.update(instanceId, requestId, { status: "ready" });
        await entry.predecessor;
        this.update(instanceId, requestId, { status: "applying" });
        await apply();
      }
    };
    try {
      await entry.execute(operation);
      this.update(instanceId, requestId, { status: "success", error: null, job: null });
      entry.resolve();
    } catch (error) {
      this.failedExecutors.set(requestId, entry.execute);
      this.update(instanceId, requestId, { status: "error", error: error instanceof Error ? error.message : String(error), job: null });
      entry.reject(error);
    } finally {
      entry.settled();
      this.running -= 1;
      this.pump();
    }
  }

  recordJob(instanceId: string, requestId: number, attemptToken: string, job: BackgroundJob) {
    const current = this.snapshot(instanceId).requests.find((request) => request.requestId === requestId);
    if (!current || current.attemptToken !== attemptToken || !isWorkshopDownloadPending(current) ||
      current.jobId !== job.id || job.target_id !== instanceId ||
      !["downloadworkshop", "download_workshop"].includes(job.kind.toLowerCase())) return;
    if (current.job?.id === job.id && current.job.status === job.status &&
      current.job.progress_percent === job.progress_percent && current.job.detail === job.detail) return;
    this.update(instanceId, requestId, { job });
  }
}
