import { formatDownloadBytes } from "../download-rate";
import { useDownloadMetrics } from "../hooks/useDownloadMetrics";
import { installationJobPercent, installationJobPhase, installationJobPhaseLabel, isActiveInstallationJob } from "../installation-job";
import type { TranslateFn } from "../i18n";
import type { BackgroundJob } from "../types";
import { ActivityProgress } from "./ActivityProgress";
import { formatDesktopError } from "../desktop-error-message";

interface InstallationActivityProps {
  job: BackgroundJob;
  name: string;
  locale: string;
  t: TranslateFn;
  onStop?: () => void;
  stopPending?: boolean;
  stopError?: string | null;
}

export function InstallationActivity({ job, name, locale, t, onStop, stopPending, stopError }: InstallationActivityProps) {
  const phase = installationJobPhase(job);
  const progress = job.install_progress;
  const snapshot = {
    operation_id: job.id, active: isActiveInstallationJob(job), phase,
    downloaded_bytes: progress?.downloaded_bytes ?? null, total_bytes: progress?.total_bytes ?? null,
    elapsed_seconds: progress?.elapsed_seconds ?? 0
  };
  const { elapsedSeconds, bytesPerSecond } = useDownloadMetrics(snapshot);
  const stopping = snapshot.active && (job.cancel_requested || stopPending);
  const label = t("installation.activity", { name, phase: stopping ? t("installation.stopping") : installationJobPhaseLabel(job, t) });
  const downloaded = snapshot.downloaded_bytes;
  const total = snapshot.total_bytes;
  const transfer = phase === "downloading" && downloaded !== null && Number.isFinite(downloaded) && downloaded >= 0
    ? total !== null && Number.isFinite(total) && total > 0
      ? `${formatDownloadBytes(downloaded, locale)} / ${formatDownloadBytes(total, locale)}`
      : t("steamcmd.downloaded", { bytes: formatDownloadBytes(downloaded, locale) }) : "";
  const speed = phase === "downloading" ? bytesPerSecond === null ? "—" : `${formatDownloadBytes(bytesPerSecond, locale)}/s` : "";
  const stopFailure = stopError ? t("installation.stopFailed", { message: formatDesktopError(t, stopError) }) : undefined;
  const title = [stopFailure, label, job.detail, job.output_excerpt].filter(Boolean).join("\n");
  return <ActivityProgress label={label} title={title} active={snapshot.active} percent={installationJobPercent(job)}
    detail={stopFailure}
    onStop={job.cancellable ? onStop : undefined} stopRequested={Boolean(stopping)} stopLabel={t(stopping ? "installation.stopping" : "installation.stop")}
    transfer={transfer} speed={speed} speedLabel={t("steamcmd.speed")}
    speedTitle={speed ? t(bytesPerSecond === null ? "steamcmd.speedPending" : "installation.speedHint") : undefined}
    elapsedLabel={progress ? t("steamcmd.elapsed", { seconds: elapsedSeconds }) : undefined} />;
}
