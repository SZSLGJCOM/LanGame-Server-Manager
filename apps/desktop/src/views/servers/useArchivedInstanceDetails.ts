import { useEffect, useState } from "react";
import { readModuleDetails } from "../../api";
import { readInstanceArchiveDetails } from "../../api-storage";
import type { InstanceArchiveDetails, InstanceArchiveSummary } from "../../storage-management-types";
import type { ModuleDetails } from "../../types";

interface ArchivedDetailsState {
  archiveId: string;
  details: InstanceArchiveDetails | null;
  moduleDetails: ModuleDetails | null;
  error: string | null;
  moduleError: string | null;
  moduleLoading: boolean;
}

function emptyState(archiveId: string): ArchivedDetailsState {
  return { archiveId, details: null, moduleDetails: null, error: null, moduleError: null, moduleLoading: false };
}

export function useArchivedInstanceDetails(archive: InstanceArchiveSummary) {
  const [retryVersion, setRetryVersion] = useState(0);
  const [state, setState] = useState(() => emptyState(archive.archive_id));
  const readable = archive.state === "archived";
  const current = readable && state.archiveId === archive.archive_id ? state : emptyState(archive.archive_id);

  useEffect(() => {
    let stale = false;
    const controller = new AbortController();
    setState(emptyState(archive.archive_id));
    if (readable) void (async () => {
      try {
        const details = await readInstanceArchiveDetails(archive.archive_id, { signal: controller.signal });
        if (stale) return;
        if (details.archive_id !== archive.archive_id) throw new Error("Archive detail identity mismatch.");
        setState({ ...emptyState(archive.archive_id), details, moduleLoading: true });
        try {
          const moduleDetails = await readModuleDetails(details.instance.summary.module_id, { includePreservedProgramCounts: false });
          if (stale) return;
          if (moduleDetails.summary.id !== details.instance.summary.module_id) throw new Error("Archive module identity mismatch.");
          setState({ ...emptyState(archive.archive_id), details, moduleDetails });
        } catch (error) {
          if (!stale) setState({ ...emptyState(archive.archive_id), details,
            moduleError: String((error as Error).message || error) });
        }
      } catch (error) {
        if (!stale) setState({ ...emptyState(archive.archive_id), error: String((error as Error).message || error) });
      }
    })();
    // Abort prevents queued inspections from reaching native storage. Reads already in progress
    // may complete, but cannot replace another archive's displayed snapshot.
    return () => { stale = true; controller.abort(); };
  }, [archive.archive_id, readable, retryVersion]);

  const phase = !readable ? "unavailable" : current.error ? "error" : !current.details ? "loading" : "ready";
  return { ...current, phase, retry: () => setRetryVersion((version) => version + 1) };
}
