import { useCallback, useEffect, useRef, useState } from "react";
import { readInstanceDragonwildsBans } from "../../../api";
import { describeError } from "../../../app-state";
import type { DragonwildsBanRoster } from "../../../types";

interface RosterState {
  instanceId: string | null;
  roster: DragonwildsBanRoster | null;
  loading: boolean;
  stale: boolean;
  error: string | null;
}
interface ReadScope {
  instanceId: string;
  pending: Promise<boolean> | null;
  generation: number;
}

/** Persistence is read once per instance and after explicit moderation or refresh. */
export function useDragonwildsBanRoster(instanceId: string | null) {
  const scopeRef = useRef<ReadScope | null>(null);
  const [state, setState] = useState<RosterState>({
    instanceId, roster: null, loading: instanceId !== null, stale: true, error: null
  });

  const read = useCallback((scope: ReadScope): Promise<boolean> => {
    if (scope.pending) return scope.pending;
    const generation = scope.generation;
    setState((current) => ({ ...current, loading: true, stale: true, error: null }));
    const pending = Promise.resolve().then(async () => {
      try {
        const roster = await readInstanceDragonwildsBans(scope.instanceId);
        if (scopeRef.current !== scope) return false;
        if (roster.instance_id !== scope.instanceId) throw new Error("The ban roster belongs to another instance.");
        const fresh = scope.generation === generation;
        setState({ instanceId: scope.instanceId, roster, loading: false, stale: !fresh, error: null });
        return fresh;
      } catch (cause) {
        if (scopeRef.current === scope) setState((current) => ({
          ...current, loading: false, stale: true, error: describeError(cause)
        }));
        return false;
      } finally {
        scope.pending = null;
      }
    });
    scope.pending = pending;
    return pending;
  }, []);

  useEffect(() => {
    if (instanceId === null) {
      scopeRef.current = null;
      setState({ instanceId: null, roster: null, loading: false, stale: true, error: null });
      return;
    }
    const scope: ReadScope = { instanceId, pending: null, generation: 0 };
    scopeRef.current = scope;
    setState({ instanceId, roster: null, loading: true, stale: true, error: null });
    void read(scope);
    return () => { if (scopeRef.current === scope) scopeRef.current = null; };
  }, [instanceId, read]);

  const refresh = useCallback(() => {
    const scope = scopeRef.current;
    return scope ? read(scope) : Promise.resolve(false);
  }, [read]);
  const invalidate = useCallback(() => {
    if (scopeRef.current) scopeRef.current.generation++;
    setState((current) => ({ ...current, stale: true }));
  }, []);
  const refreshAfterMutation = useCallback(async () => {
    const scope = scopeRef.current;
    if (!scope) return false;
    invalidate();
    // A read started before the effect cannot certify the state after it. Wait
    // for that read, then coalesce callers into one fresh read for this scope.
    if (scope.pending) await scope.pending;
    return scopeRef.current === scope ? read(scope) : false;
  }, [invalidate, read]);

  const current = state.instanceId === instanceId ? state
    : { instanceId, roster: null, loading: instanceId !== null, stale: true, error: null };
  return { ...current, refresh, invalidate, refreshAfterMutation };
}

export type DragonwildsBanRosterState = ReturnType<typeof useDragonwildsBanRoster>;
