import { useEffect, useRef, useState } from "react";
import { listInstanceSaveFiles } from "../../api";
import { describeError } from "../../app-state";
import type { SaveFileListing } from "../../types";

/** Loads one read-only listing; the backend resolves and confines the root from ids only. */
export function useSaveFileListing(instanceId: string, backupId: string | null) {
  const [listing, setListing] = useState<SaveFileListing | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const request = useRef(0);

  async function load() {
    const id = ++request.current;
    setLoading(true);
    setError(null);
    try {
      const next = await listInstanceSaveFiles(instanceId, backupId);
      if (id === request.current) setListing(next);
    } catch (failure) {
      if (id === request.current) { setListing(null); setError(describeError(failure)); }
    } finally {
      if (id === request.current) setLoading(false);
    }
  }

  useEffect(() => {
    setListing(null);
    void load();
    return () => { request.current += 1; };
  }, [instanceId, backupId]);

  return { listing, error, loading, reload: load };
}
