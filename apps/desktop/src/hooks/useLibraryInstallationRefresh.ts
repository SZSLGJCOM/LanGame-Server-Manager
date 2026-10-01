import { useCallback, useEffect, useRef } from "react";
import { refreshModules } from "../api";
import { loadLaunchPlanPreview } from "../app-state";
import {
  applyInstallationSnapshot,
  createForegroundInstallationPolling,
  LibraryInstallationReader,
  type InstallationSnapshot,
  type LaunchPreviewResult
} from "../library-installation-refresh";
import type { ModuleSummary } from "../types";

export function useLibraryInstallationRefresh(options: {
  enabled: boolean;
  getCurrentInstanceId: () => string | null;
  onModules: (modules: ModuleSummary[]) => void;
  onProgramCounts: (modules: ModuleSummary[]) => void;
  onPreview: (preview: LaunchPreviewResult) => void;
  onError: (error: unknown) => void;
}) {
  const latest = useRef(options);
  latest.current = options;
  const mounted = useRef(true);
  const readerRef = useRef<LibraryInstallationReader | null>(null);
  const countsFlight = useRef<Promise<void> | null>(null);
  if (!readerRef.current) {
    readerRef.current = new LibraryInstallationReader({
      readModules: () => refreshModules({ includePreservedProgramCounts: false }),
      readPreview: loadLaunchPlanPreview,
      getCurrentInstanceId: () => latest.current.getCurrentInstanceId()
    });
  }
  const reader = readerRef.current;
  const applySnapshot = useCallback((snapshot: InstallationSnapshot) => {
    if (!mounted.current) return;
    applyInstallationSnapshot(snapshot, latest.current);
    // The inventory queue may be held by a long archive operation. Keep one
    // count read in flight without making installation status wait for it.
    if (!countsFlight.current) {
      countsFlight.current = refreshModules().then(
        (modules) => { if (mounted.current) latest.current.onProgramCounts(modules); },
        (error) => { if (mounted.current) latest.current.onError(error); }
      ).finally(() => { countsFlight.current = null; });
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);

  useEffect(() => {
    if (!options.enabled) return;
    const polling = createForegroundInstallationPolling({
      read: () => reader.read(),
      onValue: applySnapshot,
      onError: (error) => latest.current.onError(error),
      schedule: (callback, delayMs) => window.setTimeout(callback, delayMs),
      cancel: (handle) => window.clearTimeout(handle)
    });
    const syncForeground = () => {
      polling.setForeground(document.visibilityState === "visible" && document.hasFocus());
    };
    const pause = () => polling.setForeground(false);
    syncForeground();
    window.addEventListener("focus", syncForeground);
    window.addEventListener("blur", pause);
    document.addEventListener("visibilitychange", syncForeground);
    return () => {
      polling.dispose();
      window.removeEventListener("focus", syncForeground);
      window.removeEventListener("blur", pause);
      document.removeEventListener("visibilitychange", syncForeground);
    };
  }, [options.enabled, reader, applySnapshot]);

  return useCallback(async () => {
    const snapshot = await reader.read();
    applySnapshot(snapshot);
    return snapshot;
  }, [reader, applySnapshot]);
}
