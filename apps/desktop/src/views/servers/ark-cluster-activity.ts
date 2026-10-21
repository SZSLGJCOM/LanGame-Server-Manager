import { useCallback, useEffect, useSyncExternalStore } from "react";

/** Cluster start/stop (ARK 集群) and cluster snapshots (备份) render in separate
 * maintenance categories but must never overlap; each side publishes its own
 * activity here and reads the other's. Keyed by the instance that owns the cluster. */
export type ArkClusterActivityKind = "operation" | "snapshot";

const active = new Map<string, Set<ArkClusterActivityKind>>();
const listeners = new Set<() => void>();

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

function publish(instanceId: string, kind: ArkClusterActivityKind, busy: boolean) {
  const kinds = active.get(instanceId) ?? new Set<ArkClusterActivityKind>();
  if (kinds.has(kind) === busy) return;
  if (busy) kinds.add(kind); else kinds.delete(kind);
  if (kinds.size) active.set(instanceId, kinds); else active.delete(instanceId);
  listeners.forEach((listener) => listener());
}

/** Publishes `busy` for `kind` while mounted and returns whether `other` is active. */
export function useArkClusterActivity(instanceId: string, kind: ArkClusterActivityKind, busy: boolean) {
  const other: ArkClusterActivityKind = kind === "operation" ? "snapshot" : "operation";
  const read = useCallback(() => active.get(instanceId)?.has(other) ?? false, [instanceId, other]);
  const otherBusy = useSyncExternalStore(subscribe, read, read);
  useEffect(() => {
    publish(instanceId, kind, busy);
  }, [instanceId, kind, busy]);
  useEffect(() => () => publish(instanceId, kind, false), [instanceId, kind]);
  return otherBusy;
}
