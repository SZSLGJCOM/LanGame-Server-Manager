import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  buildDragonwildsWorldSettingsPatch, dragonwildsWorldSettingValue, isDragonwildsWorldSettingEditable,
  isDragonwildsWorldSettingValueValid, readDragonwildsWorldSettings, writeDragonwildsWorldSettings,
  type DragonwildsWorldMode, type DragonwildsWorldSettingDefinition, type DragonwildsWorldSettingsSnapshot
} from "../../dragonwilds-world-settings";
import type { InstanceDetails } from "../../types";
import { useInstanceSettingsSaveCoordinator } from "./InstanceSettingsSaveContext";
import { InstanceSettingsDraftInvalidError, type InstanceSettingsSaveStatus } from "./instance-settings-save-queue";
import type { ConfigurationWorkspaceProviderProps } from "./module-types";

interface WorldDraft {
  snapshot: DragonwildsWorldSettingsSnapshot | null;
  mode: DragonwildsWorldMode | null;
  values: Record<string, string>;
}

interface WorldEditor extends WorldDraft {
  loading: boolean;
  status: InstanceSettingsSaveStatus;
  error: string | null;
  invalid: boolean;
  dirty: boolean;
  stopped: boolean;
  refresh(): Promise<void>;
  changeMode(mode: DragonwildsWorldMode): void;
  changeValue(definition: DragonwildsWorldSettingDefinition, raw: string): void;
}

const WorldContext = createContext<WorldEditor | null>(null);
class WorldDraftNotWritableError extends Error {}

function isStopped(details: InstanceDetails): boolean {
  return details.summary.status === "Stopped" && details.summary.active_process_count === 0 && !details.active_run;
}

function pending(model: WorldDraft) {
  const { snapshot, mode, values } = model;
  const invalid = Boolean(snapshot && mode && snapshot.definitions.some((definition) =>
    values[definition.tag] !== undefined && isDragonwildsWorldSettingEditable(definition, mode) &&
    !isDragonwildsWorldSettingValueValid(definition, values[definition.tag])));
  const patch = snapshot && mode && !invalid ? buildDragonwildsWorldSettingsPatch(snapshot, mode, values) : {};
  return { invalid, patch, dirty: Boolean(snapshot && mode && (mode !== snapshot.world_mode || invalid || Object.keys(patch).length)) };
}

function checkSnapshot(snapshot: DragonwildsWorldSettingsSnapshot, instanceId: string) {
  if (snapshot.instance_id !== instanceId || (snapshot.status === "ready" &&
    (!snapshot.world_file || !snapshot.revision || !snapshot.world_mode))) {
    throw new Error("The world settings response does not match this server.");
  }
}

/** One saved-world draft survives category changes and participates in the start barrier. */
export function DragonwildsWorldSettingsProvider(props: ConfigurationWorkspaceProviderProps) {
  const instanceId = props.details.summary.id;
  const coordinator = useInstanceSettingsSaveCoordinator();
  const [model, setModel] = useState<WorldDraft>({ snapshot: null, mode: null, values: {} });
  const [loading, setLoading] = useState(true);
  const [status, setStatus] = useState<InstanceSettingsSaveStatus>({ state: "saved" });
  const [error, setError] = useState<string | null>(null);
  const modelRef = useRef(model);
  const detailsRef = useRef(props.details);
  const mounted = useRef(true);
  const readGeneration = useRef(0);
  const readInFlight = useRef<Promise<void> | null>(null);
  const worker = useRef<Promise<void> | null>(null);
  const failed = useRef<Error | null>(null);
  const timer = useRef<number | null>(null);
  const flushRef = useRef<() => Promise<void>>(async () => {});
  const owner = useRef({});
  detailsRef.current = props.details;

  const publish = useCallback((next: WorldDraft) => {
    modelRef.current = next;
    if (mounted.current) setModel(next);
  }, []);

  const reportFailure = useCallback((cause: unknown) => {
    if (cause instanceof InstanceSettingsDraftInvalidError || cause instanceof WorldDraftNotWritableError) {
      if (mounted.current) setStatus({ state: "dirty" });
      return;
    }
    const failure = cause instanceof Error ? cause : new Error(String(cause));
    failed.current = failure;
    if (mounted.current) {
      setError(failure.message);
      setStatus({ state: /conflict|changed|revision/iu.test(failure.message) ? "conflict" : "failed", message: failure.message });
    }
  }, []);

  const refresh = useCallback(async () => {
    if (worker.current || readInFlight.current) return;
    const generation = ++readGeneration.current;
    setLoading(true);
    setError(null);
    const operation = (async () => {
      try {
        const next = await readDragonwildsWorldSettings(instanceId);
        checkSnapshot(next, instanceId);
        if (generation !== readGeneration.current) return;
        failed.current = null;
        publish({ snapshot: next, mode: next.world_mode, values: {} });
        if (mounted.current) setStatus({ state: "saved" });
      } catch (cause) {
        if (generation === readGeneration.current) {
          if (pending(modelRef.current).dirty) reportFailure(cause);
          else if (mounted.current) setError(cause instanceof Error ? cause.message : String(cause));
        }
      } finally {
        if (generation === readGeneration.current && mounted.current) setLoading(false);
      }
    })();
    readInFlight.current = operation;
    await operation;
    if (readInFlight.current === operation) readInFlight.current = null;
  }, [instanceId, publish, reportFailure]);

  const flush = useCallback(async () => {
    if (timer.current !== null) { window.clearTimeout(timer.current); timer.current = null; }
    if (worker.current) return worker.current;
    // A read is not an edited draft. In particular, StrictMode's cleanup must
    // not create an empty worker that blocks the replayed initial read.
    if (!pending(modelRef.current).dirty) return;
    if (readInFlight.current) await readInFlight.current;
    if (worker.current) return worker.current;
    if (!pending(modelRef.current).dirty) return;
    if (failed.current && pending(modelRef.current).dirty) throw failed.current;
    const operation = (async () => {
      for (;;) {
        const captured = modelRef.current;
        const change = pending(captured);
        if (!change.dirty) return;
        if (change.invalid) throw new InstanceSettingsDraftInvalidError();
        const { snapshot, mode } = captured;
        if (!isStopped(detailsRef.current) || !snapshot?.writable || !snapshot.world_file || !snapshot.revision || !mode) {
          throw new WorldDraftNotWritableError("Stop the server and refresh its world rules before saving changes.");
        }
        if (mounted.current) setStatus({ state: "saving" });
        const next = await writeDragonwildsWorldSettings({
          instance_id: instanceId, world_file: snapshot.world_file, expected_revision: snapshot.revision,
          world_mode: mode, values: change.patch
        });
        checkSnapshot(next, instanceId);
        if (next.status !== "ready" || next.world_file !== snapshot.world_file) {
          throw new Error("The saved world identity changed before its rules could be confirmed.");
        }
        // A completed write only owns its captured values. Edits made while saving remain queued.
        const latest = modelRef.current;
        const remaining = { ...latest.values };
        for (const [tag, raw] of Object.entries(captured.values)) {
          const definition = snapshot.definitions.find((setting) => setting.tag === tag);
          if (definition && isDragonwildsWorldSettingEditable(definition, mode) && remaining[tag] === raw) delete remaining[tag];
        }
        publish({ snapshot: next, mode: latest.mode, values: remaining });
        if (mounted.current) { setError(null); setStatus({ state: pending(modelRef.current).dirty ? "dirty" : "saved" }); }
      }
    })();
    worker.current = operation;
    try { await operation; }
    catch (cause) { reportFailure(cause); throw cause; }
    finally { if (worker.current === operation) worker.current = null; }
  }, [instanceId, publish, reportFailure]);
  flushRef.current = flush;

  useLayoutEffect(() => {
    mounted.current = true;
    const registration = coordinator.register(instanceId, () => flushRef.current(), owner.current);
    return () => {
      mounted.current = false;
      readGeneration.current += 1;
      readInFlight.current = null;
      if (timer.current !== null) { window.clearTimeout(timer.current); timer.current = null; }
      registration.detach(flushRef.current());
    };
  }, [coordinator, instanceId]);

  useEffect(() => { void refresh(); }, [refresh]);

  const change = pending(model);
  const stopped = isStopped(props.details);
  const signature = JSON.stringify([model.mode, model.values]);
  useEffect(() => {
    if (!change.dirty || change.invalid || failed.current || loading || !model.snapshot?.writable || !isStopped(detailsRef.current)) return;
    timer.current = window.setTimeout(() => {
      timer.current = null;
      void flushRef.current().then(undefined, reportFailure);
    }, 650);
    return () => { if (timer.current !== null) { window.clearTimeout(timer.current); timer.current = null; } };
  }, [signature, change.dirty, change.invalid, loading, model.snapshot?.writable, stopped, reportFailure]);

  function edit(next: WorldDraft) {
    publish(next);
    if (!failed.current) setStatus({ state: pending(next).dirty ? "dirty" : "saved" });
  }

  function changeMode(mode: DragonwildsWorldMode) {
    const current = modelRef.current;
    const values = { ...current.values };
    if (mode === "Custom" && current.mode && current.mode !== "Custom" && current.snapshot) {
      for (const setting of current.snapshot.definitions.filter((item) => isDragonwildsWorldSettingEditable(item))) {
        values[setting.tag] ??= dragonwildsWorldSettingValue(current.snapshot, current.mode, setting, values);
      }
    }
    edit({ ...current, mode, values });
    // A mode transition also determines locked native values. Persist it in order;
    // subsequent slider movements are still coalesced by the normal debounce.
    void flushRef.current().then(undefined, reportFailure);
  }

  function changeValue(definition: DragonwildsWorldSettingDefinition, raw: string) {
    const current = modelRef.current;
    if (!current.snapshot || !current.mode || !isStopped(detailsRef.current) || !current.snapshot.writable ||
      !isDragonwildsWorldSettingEditable(definition)) return;
    let mode = current.mode;
    const values = { ...current.values };
    if (!isDragonwildsWorldSettingEditable(definition, mode)) {
      // Match the native Custom transition: keep the currently displayed world before editing one rule.
      for (const setting of current.snapshot.definitions.filter((item) => isDragonwildsWorldSettingEditable(item))) {
        values[setting.tag] ??= dragonwildsWorldSettingValue(current.snapshot, mode, setting, values);
      }
      mode = "Custom";
    }
    values[definition.tag] = raw;
    edit({ ...current, mode, values });
  }

  return <WorldContext.Provider value={{ ...model, loading, status, error, invalid: change.invalid,
    dirty: change.dirty, stopped: isStopped(props.details), refresh, changeMode, changeValue }}>
    {props.children}
  </WorldContext.Provider>;
}

export function useDragonwildsWorldSettings(): WorldEditor {
  const context = useContext(WorldContext);
  if (!context) throw new Error("Dragonwilds world settings require their workspace provider.");
  return context;
}
