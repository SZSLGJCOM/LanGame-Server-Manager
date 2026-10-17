import { useCallback, useEffect, useRef, useState } from "react";
import { readInstanceDragonwildsBans, readInstanceNativeBans } from "../../../api";
import { describeError } from "../../../app-state";
import type { NativeBanRoster } from "../../../types";

export const nativeBanRosterModules = new Set([
  "runescapedragonwilds", "conanexiles", "palworld", "rimworld", "soulmask",
  "arksurvivalascended", "arksurvivalevolved", "squad", "necesse", "unturned"
]);

interface RosterState {
  instanceId: string | null;
  moduleId: string | null;
  roster: NativeBanRoster | null;
  loading: boolean;
  stale: boolean;
  error: string | null;
}
interface ReadScope {
  instanceId: string;
  moduleId: string;
  pending: Promise<boolean> | null;
  generation: number;
  roster: NativeBanRoster | null;
  rosterGeneration: number;
}

/** Persistence is read once per instance and after explicit moderation or refresh. */
export function useNativeBanRoster(instanceId: string | null, moduleId: string | null) {
  const scopeRef = useRef<ReadScope | null>(null);
  const [state, setState] = useState<RosterState>({
    instanceId, moduleId, roster: null, loading: instanceId !== null, stale: true, error: null
  });

  const read = useCallback((scope: ReadScope): Promise<boolean> => {
    if (scope.pending) return scope.pending;
    const generation = scope.generation;
    setState((current) => ({ ...current, loading: true, stale: true, error: null }));
    const pending = Promise.resolve().then(async () => {
      try {
        const roster: NativeBanRoster = scope.moduleId === "runescapedragonwilds"
          ? await readInstanceDragonwildsBans(scope.instanceId).then((value) => ({
            ...value, module_id: scope.moduleId,
            entries: value.entries.map((entry) => ({ ...entry, identity_kind: "eos_id" as const }))
          })) : await readInstanceNativeBans(scope.instanceId);
        if (scopeRef.current !== scope) return false;
        if (roster.instance_id !== scope.instanceId || roster.module_id !== scope.moduleId) {
          throw new Error("The ban roster belongs to another instance or module.");
        }
        const fresh = scope.generation === generation;
        scope.roster = roster;
        scope.rosterGeneration = fresh ? generation : -1;
        setState({ instanceId: scope.instanceId, moduleId: scope.moduleId, roster, loading: false, stale: !fresh, error: null });
        return fresh;
      } catch (cause) {
        scope.rosterGeneration = -1;
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
    if (instanceId === null || moduleId === null) {
      scopeRef.current = null;
      setState({ instanceId, moduleId, roster: null, loading: false, stale: true, error: null });
      return;
    }
    const scope: ReadScope = { instanceId, moduleId, pending: null, generation: 0, roster: null, rosterGeneration: -1 };
    scopeRef.current = scope;
    setState({ instanceId, moduleId, roster: null, loading: true, stale: true, error: null });
    void read(scope);
    return () => { if (scopeRef.current === scope) scopeRef.current = null; };
  }, [instanceId, moduleId, read]);

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
  const getFreshRoster = useCallback(() => {
    const scope = scopeRef.current;
    return scope && !scope.pending && scope.rosterGeneration === scope.generation ? scope.roster : null;
  }, []);

  const current = state.instanceId === instanceId && state.moduleId === moduleId ? state
    : { instanceId, moduleId, roster: null, loading: instanceId !== null, stale: true, error: null };
  return { ...current, refresh, invalidate, refreshAfterMutation, getFreshRoster };
}

export type NativeBanRosterState = ReturnType<typeof useNativeBanRoster>;
