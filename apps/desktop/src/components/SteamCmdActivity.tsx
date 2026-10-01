import { formatDesktopError } from "../desktop-error-message";
import type { TranslateFn } from "../i18n";
import type { SteamCmdPrepareSnapshot } from "../types";
import { formatDownloadBytes as bytes, isActiveDownload } from "../download-rate";
import { useDownloadMetrics } from "../hooks/useDownloadMetrics";
import { ActivityProgress } from "./ActivityProgress";

const WAITING_MESSAGE_KEYS = {
  queued: "steamcmd.waitingQueued",
  inspecting: "steamcmd.waitingInspection",
  downloading: "steamcmd.waitingDownload",
  extracting: "steamcmd.waitingExtraction",
  updating: "steamcmd.waiting",
  verifying: "steamcmd.waiting",
  ready: "steamcmd.prepare.ready"
} as const;

export function steamCmdDownloadPercent(snapshot: SteamCmdPrepareSnapshot): number | null {
  const { downloaded_bytes: downloaded, total_bytes: total } = snapshot;
  return (snapshot.phase === "downloading" || snapshot.phase === "updating") && downloaded !== null && total !== null
    && Number.isFinite(downloaded) && Number.isFinite(total) && downloaded >= 0 && total > 0
    ? Math.min(100, (downloaded / total) * 100) : null;
}

interface SteamCmdActivityProps {
  snapshot: SteamCmdPrepareSnapshot;
  message: string;
  locale: string;
  t: TranslateFn;
  onStop?: () => void;
  stopPending?: boolean;
  stopError?: string | null;
}

export function SteamCmdActivity({ snapshot, message, locale, t, onStop, stopPending, stopError }: SteamCmdActivityProps) {
  const { extraSeconds, elapsedSeconds: elapsed, bytesPerSecond: rate } = useDownloadMetrics(snapshot);
  const phase = t(`steamcmd.prepare.${snapshot.phase}`);
  const percent = steamCmdDownloadPercent(snapshot);
  // The diagnostic tail appends unfinished fragments from multiple streams;
  // its last line is not necessarily the latest reported progress record.
  const detail = snapshot.output_excerpt.trim() ? snapshot.detail.trim() : "";
  const lastOutput = detail.includes("\uFFFD") ? "" : detail;
  const error = snapshot.error ? formatDesktopError(t, snapshot.error) : null;
  const stopping = snapshot.active && (snapshot.cancel_requested || stopPending);
  const waiting = snapshot.active && snapshot.idle_seconds + extraSeconds >= 15;
  const hint = waiting ? t(WAITING_MESSAGE_KEYS[snapshot.phase]) : t("steamcmd.prepareHint");
  const statusDetail = stopError ? t("installation.stopFailed", { message: formatDesktopError(t, stopError) }) : error ? error.split("\n")[0]
    : snapshot.cancelled || stopping ? ""
    : message && message !== phase ? t("steamcmd.progressRetry") : waiting ? t("steamcmd.waitingShort") : "";
  const title = stopError ? statusDetail : error ?? [phase, hint, message !== phase ? message : "",
    lastOutput ? `${t("steamcmd.latestOutput")} ${lastOutput}` : ""].filter(Boolean).join("\n");
  const transfer = snapshot.downloaded_bytes !== null && Number.isFinite(snapshot.downloaded_bytes)
    && snapshot.downloaded_bytes >= 0 && (snapshot.phase === "downloading" || snapshot.phase === "updating")
    ? percent !== null && snapshot.total_bytes !== null
      ? `${bytes(snapshot.downloaded_bytes, locale)} / ${bytes(snapshot.total_bytes, locale)}`
      : t("steamcmd.downloaded", { bytes: bytes(snapshot.downloaded_bytes, locale) })
    : "";
  const elapsedLabel = t("steamcmd.elapsed", { seconds: elapsed });
  const speed = isActiveDownload(snapshot) ? rate === null ? "—" : `${bytes(rate, locale)}/s` : "";
  const speedTitle = speed ? t(rate === null ? "steamcmd.speedPending" : "steamcmd.speedHint") : undefined;

  return (
    <ActivityProgress label={snapshot.cancelled ? t("steamcmd.cancelled") : stopping ? t("steamcmd.stopping") : error ? t("steamcmd.failed") : phase} detail={statusDetail} title={title}
      active={snapshot.active} error={Boolean(error)} percent={percent} transfer={transfer}
      onStop={snapshot.cancellable ? onStop : undefined} stopRequested={Boolean(stopping)} stopLabel={t(stopping ? "installation.stopping" : "installation.stop")}
      speed={speed} speedTitle={speedTitle} speedLabel={t("steamcmd.speed")} elapsedLabel={elapsedLabel} />
  );
}
