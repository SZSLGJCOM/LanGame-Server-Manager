import { useCallback, useRef, useState } from "react";

export type InstanceRetirementKind = "delete" | "archive";

/** The synchronous owner closes the gap before React disposes polling effects. */
export function useInstanceRetirement(onBegin?: (instanceId: string) => void) {
  const pending = useRef(new Map<string, InstanceRetirementKind>());
  const revision = useRef(0);
  const [operations, setOperations] = useState<ReadonlyMap<string, InstanceRetirementKind>>(() => new Map());
  const begin = useCallback((instanceId: string, kind: InstanceRetirementKind) => {
    if (pending.current.size > 0) return false;
    pending.current.set(instanceId, kind);
    revision.current++;
    onBegin?.(instanceId);
    setOperations(new Map(pending.current));
    return true;
  }, [onBegin]);
  const finish = useCallback((instanceId: string) => {
    pending.current.delete(instanceId);
    revision.current++;
    setOperations(new Map(pending.current));
  }, []);
  const isPending = useCallback((instanceId?: string | null) => instanceId === undefined
    ? pending.current.size > 0 : instanceId !== null && pending.current.has(instanceId), []);
  const currentRevision = useCallback(() => revision.current, []);
  const mergeInstances = useCallback(<T extends { id: string }>(current: T[], incoming: T[], reconcileInstanceId?: string) => {
    const protectedIds = new Set([...pending.current.keys()].filter((id) => id !== reconcileInstanceId));
    return [...incoming.filter((item) => !protectedIds.has(item.id)), ...current.filter((item) => protectedIds.has(item.id))];
  }, []);
  return { operations, begin, finish, isPending, currentRevision, mergeInstances };
}
