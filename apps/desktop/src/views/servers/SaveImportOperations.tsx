import { createContext, useContext, useRef, useSyncExternalStore, type ReactNode } from "react";
import { describeError } from "../../app-state";
import { importSave, type SaveImportResult } from "../../save-import";

interface ImportState { pending: boolean; result: SaveImportResult | null; error: string | null }
const idle: ImportState = { pending: false, result: null, error: null };

// The server workspace owns this bounded history, outside the keyed instance
// detail. Switching instances cannot detach the result of an accepted import.
function createOperations() {
  const states = new Map<string, ImportState>();
  const listeners = new Set<() => void>();
  function publish(id: string, state: ImportState) {
    states.delete(id);
    states.set(id, state);
    for (const [key, value] of states) {
      if (states.size <= 32) break;
      if (!value.pending) states.delete(key);
    }
    for (const listener of listeners) listener();
  }
  return {
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    snapshot(id: string) { return states.get(id) ?? idle; },
    clear(id: string) { if (!states.get(id)?.pending) publish(id, idle); },
    async run(id: string, source: string, fingerprint: string) {
      if (states.get(id)?.pending) return;
      if ([...states.values()].filter(state => state.pending).length >= 4) {
        throw new Error("Wait for an ongoing save import to finish before starting another.");
      }
      publish(id, { pending: true, result: null, error: null });
      try { publish(id, { pending: false, result: await importSave(id, source, fingerprint), error: null }); }
      catch (failure) { publish(id, { pending: false, result: null, error: describeError(failure) }); }
    }
  };
}
const ImportOperations = createContext<ReturnType<typeof createOperations> | null>(null);

export function SaveImportOperationsProvider({ children }: { children: ReactNode }) {
  const operations = useRef<ReturnType<typeof createOperations> | null>(null);
  if (!operations.current) operations.current = createOperations();
  return <ImportOperations.Provider value={operations.current}>{children}</ImportOperations.Provider>;
}

export function useSaveImportOperation(instanceId: string) {
  const operations = useContext(ImportOperations);
  if (!operations) throw new Error("Save import requires its server workspace owner.");
  const state = useSyncExternalStore(operations.subscribe, () => operations.snapshot(instanceId), () => idle);
  return { ...state, run: (source: string, fingerprint: string) => operations.run(instanceId, source, fingerprint),
    clear: () => operations.clear(instanceId) };
}
