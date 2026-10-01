export interface DownloadSnapshot {
  operation_id: string;
  active: boolean;
  phase: string;
  downloaded_bytes: number | null;
  total_bytes: number | null;
  elapsed_seconds: number;
}

interface DownloadSample {
  seconds: number;
  bytes: number;
}

export interface DownloadRateWindow {
  operationId: string;
  phase: string;
  totalBytes: number | null;
  samples: DownloadSample[];
}

const WINDOW_SECONDS = 5;
const STALE_SECONDS = 3;

export function isActiveDownload(snapshot: DownloadSnapshot): boolean {
  return snapshot.active && (snapshot.phase === "downloading" || snapshot.phase === "updating")
    && snapshot.downloaded_bytes !== null && Number.isFinite(snapshot.downloaded_bytes)
    && snapshot.downloaded_bytes >= 0;
}

// Both the archive and Steam's self-update report cumulative byte counts. Use
// recent backend timestamps so delayed polls cannot create a false speed spike.
export function sampleDownloadRate(
  previous: DownloadRateWindow | null,
  snapshot: DownloadSnapshot
): DownloadRateWindow | null {
  if (!isActiveDownload(snapshot) || snapshot.downloaded_bytes === null
    || !Number.isFinite(snapshot.elapsed_seconds) || snapshot.elapsed_seconds < 0) return null;
  const sample = { seconds: snapshot.elapsed_seconds, bytes: snapshot.downloaded_bytes };
  const last = previous?.samples[previous.samples.length - 1];
  const sameTransfer = previous?.operationId === snapshot.operation_id && previous.phase === snapshot.phase
    && previous.totalBytes === snapshot.total_bytes && last
    && sample.seconds >= last.seconds && sample.bytes >= last.bytes;
  const samples = sameTransfer ? previous.samples.filter((value) =>
    value.seconds < sample.seconds && value.seconds >= sample.seconds - WINDOW_SECONDS) : [];
  return {
    operationId: snapshot.operation_id, phase: snapshot.phase, totalBytes: snapshot.total_bytes,
    samples: [...samples, sample]
  };
}

export function downloadBytesPerSecond(
  window: DownloadRateWindow | null,
  snapshot: DownloadSnapshot,
  secondsSinceSnapshot = 0
): number | null {
  if (!window || !isActiveDownload(snapshot) || window.operationId !== snapshot.operation_id
    || window.phase !== snapshot.phase || window.totalBytes !== snapshot.total_bytes
    || secondsSinceSnapshot >= STALE_SECONDS) return null;
  const first = window.samples[0];
  const last = window.samples[window.samples.length - 1];
  if (!first || !last || last.bytes !== snapshot.downloaded_bytes
    || last.seconds !== snapshot.elapsed_seconds || last.seconds <= first.seconds) return null;
  return (last.bytes - first.bytes) / (last.seconds - first.seconds);
}

export function formatDownloadBytes(value: number, locale: string): string {
  const unit = value >= 1024 ** 3 ? "GB" : value >= 1024 ** 2 ? "MB" : "KB";
  const divisor = unit === "GB" ? 1024 ** 3 : unit === "MB" ? 1024 ** 2 : 1024;
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(value / divisor)} ${unit}`;
}
