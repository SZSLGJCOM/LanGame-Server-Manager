import { createContext, useContext, useEffect, useState, useSyncExternalStore, type ReactNode } from "react";
import { prepareArkTools, readArkToolsInstallProgress, readArkToolsStatus, removeArkTools } from "../api-ark-tools";
import { createArkToolsInstallationStore, type ArkToolsInstallationStore } from "../ark-tools-installation-store";

const ArkToolsInstallationContext = createContext<ArkToolsInstallationStore | null>(null);

export function ArkToolsInstallationProvider({ children }: { children: ReactNode }) {
  const [store] = useState(() => createArkToolsInstallationStore({
    prepare: prepareArkTools,
    remove: removeArkTools,
    readProgress: readArkToolsInstallProgress,
    readStatus: readArkToolsStatus,
    newOperationId: () => crypto.randomUUID().replace(/-/g, ""),
    now: () => performance.now(),
    schedule: (callback, delay) => window.setTimeout(callback, delay),
    cancel: (handle: number) => window.clearTimeout(handle)
  }));
  useEffect(() => {
    store.connect();
    return () => store.disconnect();
  }, [store]);
  return <ArkToolsInstallationContext.Provider value={store}>{children}</ArkToolsInstallationContext.Provider>;
}

export function useArkToolsInstallationStore(): ArkToolsInstallationStore {
  const store = useContext(ArkToolsInstallationContext);
  if (!store) throw new Error("ARK installation requires ArkToolsInstallationProvider.");
  return store;
}

export function useArkToolsInstallationEntries() {
  const store = useArkToolsInstallationStore();
  const entries = useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot);
  return { store, entries };
}
