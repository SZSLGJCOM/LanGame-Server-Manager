import { useEffect, useRef } from "react";
import type { ArkToolsStatus } from "../../api-ark-tools";
import { useArkToolsInstallationEntries } from "../../components/ArkToolsInstallationContext";

// The page subscribes to the application-owned operation. Navigation never
// disposes the observer or removes its activity-bar progress.
export function useArkToolsInstallation(instanceId: string, instanceName: string, host: boolean, onInstalled: (status: ArkToolsStatus) => void) {
  const { store, entries } = useArkToolsInstallationEntries();
  const entry = entries.find(value => value.instanceId === instanceId);
  const onInstalledRef = useRef(onInstalled);
  onInstalledRef.current = onInstalled;
  useEffect(() => { if (host) store.recover(instanceId, instanceName); }, [host, instanceId, instanceName, store]);
  useEffect(() => {
    if (entry?.status) onInstalledRef.current(entry.status);
  }, [instanceId, entry?.status]);

  return {
    active: entry?.observing ?? false,
    removing: entry?.removing ?? false,
    removalResult: entry?.removalResult ?? null,
    removalError: entry?.removalError ?? null,
    recovering: host && (!entry || entry.recovering),
    install() { if (host) store.install(instanceId, instanceName); },
    async remove() { return host ? store.remove(instanceId, instanceName) : null; },
    issue: entry?.issue ?? null,
    progress: entry?.progress ?? null
  };
}
