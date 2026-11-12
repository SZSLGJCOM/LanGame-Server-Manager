import type { ArkToolsPreparationResult, ArkToolsRemovalResult, ArkToolsStatus } from "./api-ark-tools";
import { observeArkToolsInstallation, queuedArkToolsInstallation, type ArkToolsInstallObservationIssue, type ArkToolsInstallProgress } from "./ark-tools-installation";

export interface ArkToolsInstallationEntry {
  instanceId: string;
  instanceName: string;
  progress: ArkToolsInstallProgress | null;
  observing: boolean;
  recovering: boolean;
  removing: boolean;
  removalResult: ArkToolsRemovalResult | null;
  removalError: string | null;
  issue: ArkToolsInstallObservationIssue | null;
  status: ArkToolsStatus | null;
  dismissed: boolean;
  observedAt: number;
}

export interface ArkToolsInstallationStoreOptions<TimerHandle> {
  prepare: (instanceId: string, operationId: string) => Promise<ArkToolsPreparationResult>;
  remove: (instanceId: string) => Promise<ArkToolsRemovalResult>;
  readProgress: (instanceId: string, operationId?: string) => Promise<ArkToolsInstallProgress | null>;
  readStatus: (instanceId: string) => Promise<ArkToolsStatus>;
  newOperationId: () => string;
  now: () => number;
  schedule: (callback: () => void, delayMs: number) => TimerHandle;
  cancel: (handle: TimerHandle) => void;
}

const COMPLETED_HISTORY_LIMIT = 32;

// The application owns these observers. Subscriber lifetimes only affect
// rendering; navigation must not release ownership of an unknown native result.
export function createArkToolsInstallationStore<TimerHandle>(options: ArkToolsInstallationStoreOptions<TimerHandle>) {
  type Observer = ReturnType<typeof observeArkToolsInstallation<TimerHandle>>;
  interface Record {
    entry: ArkToolsInstallationEntry;
    pending: "recovery" | "installation" | "observation" | "removal" | null;
    observer: Observer | null;
    recoveryRunning: boolean;
    recovered: boolean;
    version: number;
    order: number;
  }
  const records = new Map<string, Record>();
  const listeners = new Set<() => void>();
  let snapshot: readonly ArkToolsInstallationEntry[] = [];
  let connected = false;
  let session = 0;
  let order = 0;

  function publish() {
    const completed = [...records.values()].filter(record => record.pending === null)
      .sort((left, right) => left.order - right.order);
    for (const record of completed.slice(0, Math.max(0, completed.length - COMPLETED_HISTORY_LIMIT))) {
      records.delete(record.entry.instanceId);
    }
    snapshot = [...records.values()].sort((left, right) => left.order - right.order).map(record => record.entry);
    listeners.forEach(listener => listener());
  }

  function replace(record: Record, change: Partial<ArkToolsInstallationEntry>) {
    record.entry = { ...record.entry, ...change };
    publish();
  }

  function ensureRecord(instanceId: string, instanceName: string): Record {
    const existing = records.get(instanceId);
    if (existing) {
      if (existing.entry.instanceName !== instanceName) replace(existing, { instanceName });
      return existing;
    }
    const record: Record = {
      entry: { instanceId, instanceName, progress: null, observing: false, recovering: false,
        removing: false, removalResult: null, removalError: null,
        issue: null, status: null, dismissed: false, observedAt: options.now() },
      pending: null, observer: null, recoveryRunning: false, recovered: false, version: 0, order: ++order
    };
    records.set(instanceId, record);
    return record;
  }

  function isCurrent(record: Record, currentSession: number, version: number) {
    return connected && session === currentSession && record.version === version
      && records.get(record.entry.instanceId) === record;
  }

  function observe(record: Record, initial: ArkToolsInstallProgress, prepare?: () => Promise<ArkToolsPreparationResult>) {
    const currentSession = session;
    const version = ++record.version;
    record.pending = "observation";
    record.recoveryRunning = false;
    replace(record, { progress: initial, observing: true, recovering: false, issue: null,
      observedAt: initial === record.entry.progress ? record.entry.observedAt : options.now() });
    const observer = observeArkToolsInstallation({
      initial, prepare,
      read: () => options.readProgress(initial.instanceId, initial.operationId),
      readStatus: () => options.readStatus(initial.instanceId), schedule: options.schedule, cancel: options.cancel,
      onProgress: progress => {
        if (isCurrent(record, currentSession, version)) replace(record, { progress,
          observedAt: progress === record.entry.progress ? record.entry.observedAt : options.now() });
      },
      onStatus: status => {
        if (isCurrent(record, currentSession, version)) replace(record, {
          status: { installed: status.installed, updateAvailable: status.updateAvailable,
            connected: status.connected, canUninstall: status.canUninstall, issue: status.issue }
        });
      },
      onIssue: issue => {
        if (isCurrent(record, currentSession, version)) replace(record, { issue });
      },
      onSettled: () => {
        if (!isCurrent(record, currentSession, version)) return;
        record.pending = null;
        record.observer = null;
        record.order = ++order;
        replace(record, { observing: false, recovering: false });
      }
    });
    if (isCurrent(record, currentSession, version) && record.pending === "observation") record.observer = observer;
    else observer.dispose();
  }

  function beginInstall(record: Record) {
    const operationId = options.newOperationId();
    const instanceId = record.entry.instanceId;
    record.order = ++order;
    replace(record, { status: null, dismissed: false, removalResult: null, removalError: null });
    observe(record, queuedArkToolsInstallation(instanceId, operationId), () => options.prepare(instanceId, operationId));
  }

  function recoverRecord(record: Record) {
    const currentSession = session;
    const version = ++record.version;
    record.recoveryRunning = true;
    replace(record, { recovering: true, issue: null });
    void (async () => {
      try {
        const progress = await options.readProgress(record.entry.instanceId);
        if (!isCurrent(record, currentSession, version)) return;
        record.recoveryRunning = false;
        if (progress && progress.instanceId !== record.entry.instanceId) throw new Error("ARK installation progress belongs to another instance.");
        record.recovered = true;
        if (progress?.active) {
          replace(record, { status: null, dismissed: false });
          observe(record, progress);
        } else if (record.pending === "installation") {
          beginInstall(record);
        } else if (progress?.phase === "ready") {
          // Restore the terminal timer as-is while the observer inspects status.
          observe(record, progress);
        } else {
          record.pending = null;
          record.order = ++order;
          replace(record, { progress, recovering: false, observing: false, status: null, issue: null,
            observedAt: progress === record.entry.progress ? record.entry.observedAt : options.now() });
        }
      } catch (error) {
        if (!isCurrent(record, currentSession, version)) return;
        record.recoveryRunning = false;
        // Keep the intent pending. A later explicit recovery/install or provider
        // reconnect can read again, but an unavailable service never permits prepare.
        replace(record, { recovering: false, issue: { kind: "progress", message: error instanceof Error ? error.message : String(error) } });
      }
    })();
  }

  function advance(record: Record) {
    if (!connected || record.observer || record.recoveryRunning) return;
    if (record.pending === "observation" && record.entry.progress) observe(record, record.entry.progress);
    else if (record.pending === "recovery" || record.pending === "installation") recoverRecord(record);
  }

  return {
    getSnapshot: () => snapshot,
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    recover(instanceId: string, instanceName: string) {
      const record = ensureRecord(instanceId, instanceName);
      if (record.pending === null && !record.recovered) {
        record.pending = "recovery";
        replace(record, { recovering: true });
      }
      advance(record);
    },
    install(instanceId: string, instanceName: string) {
      const record = ensureRecord(instanceId, instanceName);
      if (record.pending === "removal") return;
      if (record.pending !== "observation") {
        record.pending = "installation";
        replace(record, { recovering: true });
      }
      advance(record);
    },
    async remove(instanceId: string, instanceName: string): Promise<ArkToolsRemovalResult | null> {
      const record = ensureRecord(instanceId, instanceName);
      if (!connected || !record.recovered || record.pending !== null) return null;
      // Removal owns this instance across page navigation. Invalidate completed
      // installation observers before deleting so their late receipts stay stale.
      ++record.version;
      record.pending = "removal";
      replace(record, { removing: true, removalResult: null, removalError: null });
      try {
        const result = await options.remove(instanceId);
        record.pending = null;
        record.order = ++order;
        replace(record, { removing: false, removalResult: result, status: result,
          progress: null, issue: null, dismissed: true });
        return result;
      } catch (error) {
        record.pending = null;
        replace(record, { removing: false, removalError: error instanceof Error ? error.message : String(error) });
        return null;
      }
    },
    dismiss(operationId: string) {
      const record = [...records.values()].find(item => item.entry.progress?.operationId === operationId);
      if (!record || record.pending !== null || record.entry.progress?.active || record.entry.dismissed) return;
      replace(record, { dismissed: true });
    },
    connect() {
      if (connected) return;
      connected = true;
      ++session;
      for (const record of [...records.values()]) advance(record);
    },
    disconnect() {
      if (!connected) return;
      connected = false;
      ++session;
      for (const record of records.values()) {
        ++record.version;
        record.observer?.dispose();
        record.observer = null;
        record.recoveryRunning = false;
        record.entry = { ...record.entry, observing: false, recovering: false };
      }
      publish();
    }
  };
}

export type ArkToolsInstallationStore = ReturnType<typeof createArkToolsInstallationStore>;
