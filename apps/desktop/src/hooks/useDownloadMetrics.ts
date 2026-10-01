import { useEffect, useState } from "react";
import { downloadBytesPerSecond, sampleDownloadRate, type DownloadRateWindow, type DownloadSnapshot } from "../download-rate";

export function useDownloadMetrics(snapshot: DownloadSnapshot) {
  const [extraSeconds, setExtraSeconds] = useState(0);
  const [samples, setSamples] = useState<DownloadRateWindow | null>(null);
  const { operation_id, active, phase, downloaded_bytes, total_bytes, elapsed_seconds } = snapshot;
  useEffect(() => {
    setExtraSeconds(0);
    setSamples((previous) => sampleDownloadRate(previous, {
      operation_id, active, phase, downloaded_bytes, total_bytes, elapsed_seconds
    }));
    if (!active) return;
    const observedAt = performance.now();
    const timer = window.setInterval(() => setExtraSeconds(Math.floor((performance.now() - observedAt) / 1000)), 1000);
    return () => window.clearInterval(timer);
    // Re-reading the same cached job must not make stale byte reports look fresh.
  }, [operation_id, active, phase, downloaded_bytes, total_bytes, elapsed_seconds]);
  return {
    extraSeconds,
    elapsedSeconds: elapsed_seconds + (active ? extraSeconds : 0),
    bytesPerSecond: downloadBytesPerSecond(sampleDownloadRate(samples, snapshot), snapshot, extraSeconds)
  };
}
