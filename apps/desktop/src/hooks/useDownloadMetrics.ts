import { useEffect, useState } from "react";
import { downloadBytesPerSecond, sampleDownloadRate, type DownloadRateWindow, type DownloadSnapshot } from "../download-rate";

export function useDownloadMetrics(snapshot: DownloadSnapshot, observedAt?: number) {
  const sinceObservation = () => snapshot.active && observedAt !== undefined
    ? Math.max(0, Math.floor((performance.now() - observedAt) / 1000)) : 0;
  const [clock, setClock] = useState(() => ({ observation: { ...snapshot, observedAt }, extraSeconds: sinceObservation() }));
  const [samples, setSamples] = useState<DownloadRateWindow | null>(null);
  const { operation_id, active, phase, downloaded_bytes, total_bytes, elapsed_seconds } = snapshot;
  const previous = clock.observation;
  const sameObservation = previous.operation_id === operation_id && previous.active === active && previous.phase === phase
    && previous.downloaded_bytes === downloaded_bytes && previous.total_bytes === total_bytes
    && previous.elapsed_seconds === elapsed_seconds && previous.observedAt === observedAt;
  // A new snapshot commits before the effect resets its timer. Never combine
  // its native elapsed time with extra seconds from the previous observation.
  const extraSeconds = sameObservation ? clock.extraSeconds : sinceObservation();
  useEffect(() => {
    const observation = { operation_id, active, phase, downloaded_bytes, total_bytes, elapsed_seconds, observedAt };
    const updateClock = (extraSeconds: number) => setClock({ observation, extraSeconds });
    updateClock(sinceObservation());
    setSamples((previous) => sampleDownloadRate(previous, {
      operation_id, active, phase, downloaded_bytes, total_bytes, elapsed_seconds
    }));
    if (!active) return;
    // A shared observer can retain this monotonic anchor while a view or the
    // activity details panel is remounted, including during failed polls.
    const anchor = observedAt ?? performance.now();
    const timer = window.setInterval(() => updateClock(Math.max(0, Math.floor((performance.now() - anchor) / 1000))), 1000);
    return () => window.clearInterval(timer);
    // Re-reading the same cached job must not make stale byte reports look fresh.
  }, [operation_id, active, phase, downloaded_bytes, total_bytes, elapsed_seconds, observedAt]);
  return {
    extraSeconds,
    elapsedSeconds: elapsed_seconds + (active ? extraSeconds : 0),
    bytesPerSecond: downloadBytesPerSecond(sampleDownloadRate(samples, snapshot), snapshot, extraSeconds)
  };
}
