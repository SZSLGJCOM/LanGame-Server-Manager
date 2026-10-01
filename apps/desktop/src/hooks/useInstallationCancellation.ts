import { useEffect, useRef, useState } from "react";
import { cancelInstallationJob } from "../api";
import { isActiveInstallationJob } from "../installation-job";
import type { BackgroundJob } from "../types";

export function useInstallationCancellation(jobs: BackgroundJob[]) {
  const mounted = useRef(true);
  const pending = useRef(new Set<string>());
  const jobsRef = useRef(jobs);
  jobsRef.current = jobs;
  const [installationStopPendingIds, setPendingIds] = useState<string[]>([]);
  const [installationStopErrors, setErrors] = useState<Record<string, string>>({});
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);
  useEffect(() => {
    const activeIds = new Set(jobs.filter(isActiveInstallationJob).map((job) => job.id));
    const completed = [...pending.current].filter((id) => !activeIds.has(id));
    if (completed.length) {
      completed.forEach((id) => pending.current.delete(id));
      setPendingIds([...pending.current]);
    }
    setErrors((current) => Object.fromEntries(Object.entries(current).filter(([id]) => activeIds.has(id))));
  }, [jobs]);

  async function handleCancelInstallation(jobId: string) {
    const job = jobsRef.current.find((item) => item.id === jobId);
    if (!job || !isActiveInstallationJob(job) || !job.cancellable || job.cancel_requested || pending.current.has(jobId)) return;
    pending.current.add(jobId);
    setPendingIds([...pending.current]);
    setErrors((current) => { const next = { ...current }; delete next[jobId]; return next; });
    try {
      await cancelInstallationJob(jobId);
      // Keep the stop request visible until the job reader reports a terminal status.
    } catch (error) {
      if (!mounted.current) return;
      pending.current.delete(jobId);
      setPendingIds([...pending.current]);
      const latest = jobsRef.current.find((item) => item.id === jobId);
      if (latest && isActiveInstallationJob(latest)) {
        const detail = error instanceof Error ? error.message : String(error);
        setErrors((current) => ({ ...current, [jobId]: detail }));
      }
    }
  }
  return { handleCancelInstallation, installationStopPendingIds, installationStopErrors };
}
