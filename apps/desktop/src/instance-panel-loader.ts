import {
  listInstanceBackups, listInstancesFromStorage, previewInstanceLaunch, readInstanceDetails,
  readInstanceLogDocument, readInstanceRuntime, readInstanceRuntimeWindowSnapshot, readInstanceIsolation
} from "./api";
import { describeError, resolveLogDocument, type SelectedInstancePanelData } from "./app-state";
import type { InstanceRuntimeOverview, LogTailSnapshot } from "./types";
import type { TranslateFn } from "./i18n";

export const INSTANCE_PANEL_PARTS = ["details", "backups", "runtime", "runtimeWindows", "logDocument", "launchPlan"] as const;
export type InstancePanelPart = typeof INSTANCE_PANEL_PARTS[number];
export type InstancePanelPatch = Partial<SelectedInstancePanelData>;
export interface InstancePanelLoadState {
  instanceId: string;
  pending: InstancePanelPart[];
  errors: Partial<Record<InstancePanelPart, string>>;
}

interface InstancePanelPorts {
  listInstanceBackups: typeof listInstanceBackups;
  listInstancesFromStorage: typeof listInstancesFromStorage;
  previewInstanceLaunch: typeof previewInstanceLaunch;
  readInstanceDetails: typeof readInstanceDetails;
  readInstanceIsolation: typeof readInstanceIsolation;
  readInstanceLogDocument: typeof readInstanceLogDocument;
  readInstanceRuntime: typeof readInstanceRuntime;
  readInstanceRuntimeWindowSnapshot: typeof readInstanceRuntimeWindowSnapshot;
}

interface PanelLoadObserver {
  signal: AbortSignal;
  onUpdate: (patch: InstancePanelPatch) => void;
  onProgress: (state: InstancePanelLoadState) => void;
}

const READ_DEADLINE_MS = 15_000;
const MAX_PENDING_READS = 48;
const TIMEOUT_MESSAGE = "Instance data request timed out.";
const BUSY_MESSAGE = "Previous instance data requests are still pending.";

export function formatInstancePanelError(error: string, t: TranslateFn): string {
  if (error === TIMEOUT_MESSAGE) return t("servers.loading.timeout");
  if (error === BUSY_MESSAGE) return t("servers.loading.busy");
  return error;
}

function observeRead<T>(flight: Promise<T>, signal: AbortSignal, deadlineMs: number): Promise<T> {
  return new Promise((resolve, reject) => {
    if (signal.aborted) { reject(signal.reason); return; }
    const finish = (complete: () => void) => {
      clearTimeout(timer);
      signal.removeEventListener("abort", abort);
      complete();
    };
    const abort = () => finish(() => reject(signal.reason));
    const timer = setTimeout(() => finish(() => reject(new Error(TIMEOUT_MESSAGE))), deadlineMs);
    signal.addEventListener("abort", abort, { once: true });
    flight.then(value => finish(() => resolve(value)), error => finish(() => reject(error)));
  });
}

/** Owns only in-flight reads: timed-out native IPC remains shared until it really settles. */
export class InstancePanelReader {
  private readonly pendingReads = new Map<string, { flight: Promise<unknown>; invalidated: boolean; observers: Set<{ signal: AbortSignal }> }>();

  constructor(private readonly ports: InstancePanelPorts, private readonly deadlineMs = READ_DEADLINE_MS) {}

  invalidate(instanceId: string) {
    for (const [key, pending] of this.pendingReads) {
      if (key === "instances" || key.startsWith(`${instanceId}:`)) pending.invalidated = true;
    }
  }

  private read<T>(key: string, read: () => Promise<T>, signal: AbortSignal): Promise<T> {
    if (signal.aborted) return Promise.reject(signal.reason);
    // Each key has exactly one reader and result type within this class.
    const previous = this.pendingReads.get(key);
    let pending = previous;
    const observer = { signal };
    if (!pending || pending.invalidated) {
      if (!previous && this.pendingReads.size >= MAX_PENDING_READS) {
        return Promise.reject(new Error(BUSY_MESSAGE));
      }
      // Native IPC cannot be cancelled. A recovered instance needs a fresh read,
      // serialized behind the invalidated flight rather than reusing its failure.
      const observers = new Set([observer]);
      const start = () => [...observers].some(owner => !owner.signal.aborted) ? read() : Promise.reject(signal.reason);
      const flight = previous ? previous.flight.then(start, start) : Promise.resolve().then(read);
      pending = { flight, invalidated: false, observers };
      this.pendingReads.set(key, pending);
      const settled = () => { if (this.pendingReads.get(key) === pending) this.pendingReads.delete(key); };
      void flight.then(settled, settled);
    }
    const active = pending;
    active.observers.add(observer);
    return observeRead(active.flight as Promise<T>, signal, this.deadlineMs).finally(() => active.observers.delete(observer));
  }

  readInstances(signal: AbortSignal) {
    return this.read("instances", this.ports.listInstancesFromStorage, signal);
  }

  readIsolation(instanceId: string, signal: AbortSignal) {
    return this.read(instanceId + ":isolation", () => this.ports.readInstanceIsolation(instanceId), signal);
  }

  async load(instanceId: string, observer: PanelLoadObserver): Promise<InstancePanelLoadState> {
    let state: InstancePanelLoadState = { instanceId, pending: [...INSTANCE_PANEL_PARTS], errors: {} };
    let runtime: InstanceRuntimeOverview | null = null;
    let logDocument: LogTailSnapshot | null = null;
    if (observer.signal.aborted) return state;
    observer.onProgress(state);
    const collect = async <T>(part: InstancePanelPart, read: () => Promise<T>, patch: (value: T) => InstancePanelPatch) => {
      try {
        const value = await this.read(`${instanceId}:${part}`, read, observer.signal);
        if (observer.signal.aborted) return;
        observer.onUpdate(patch(value));
      } catch (error) {
        if (observer.signal.aborted) return;
        state = { ...state, errors: { ...state.errors, [part]: describeError(error) } };
        if (part === "launchPlan") observer.onUpdate({ launchPlan: null, launchPlanError: describeError(error) });
      } finally {
        if (!observer.signal.aborted) {
          state = { ...state, pending: state.pending.filter(item => item !== part) };
          observer.onProgress(state);
        }
      }
    };
    await Promise.all([
      collect("details", () => this.ports.readInstanceDetails(instanceId), details => ({ details })),
      collect("backups", () => this.ports.listInstanceBackups(instanceId), backups => ({ backups })),
      collect("runtime", () => this.ports.readInstanceRuntime(instanceId), value => {
        runtime = value;
        return { runtime, logDocument: resolveLogDocument(runtime, logDocument) };
      }),
      collect("runtimeWindows", () => this.ports.readInstanceRuntimeWindowSnapshot(instanceId), runtimeWindows => ({ runtimeWindows })),
      collect("logDocument", () => this.ports.readInstanceLogDocument(instanceId, 400), value => {
        logDocument = value;
        return { logDocument: runtime ? resolveLogDocument(runtime, value) : value };
      }),
      collect("launchPlan", () => this.ports.previewInstanceLaunch(instanceId), launchPlan => ({ launchPlan, launchPlanError: null }))
    ]);
    return state;
  }
}

export const instancePanelReader = new InstancePanelReader({
  listInstanceBackups, listInstancesFromStorage, previewInstanceLaunch, readInstanceDetails,
  readInstanceLogDocument, readInstanceRuntime, readInstanceRuntimeWindowSnapshot, readInstanceIsolation
});
