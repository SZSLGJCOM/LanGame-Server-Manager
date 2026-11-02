import type { ArkToolsInstallProgress, ArkToolsInstallObservationIssue } from "../ark-tools-installation";
import { ActivityProgress } from "./ActivityProgress";
import { ActivityNoticeContent } from "./ActivityNotice";
import { formatDownloadBytes } from "../download-rate";
import { useDownloadMetrics } from "../hooks/useDownloadMetrics";
import { selectLocaleText, useI18n } from "../i18n";
import { useArkToolsInstallationEntries } from "./ArkToolsInstallationContext";

export function arkToolsDownloadPercent(snapshot: ArkToolsInstallProgress): number | null {
  return snapshot.phase === "downloading" && snapshot.downloadedBytes !== null && snapshot.totalBytes !== null
    && snapshot.totalBytes > 0 ? Math.min(100, snapshot.downloadedBytes / snapshot.totalBytes * 100) : null;
}

function phaseLabel(snapshot: ArkToolsInstallProgress, locale: string, instanceName?: string) {
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const labels = {
    queued: text("等待安装扩展", "Waiting to install extension"),
    checking: text("检查安装条件", "Checking installation"),
    connecting: text("连接下载源", "Connecting to download source"),
    downloading: text("下载服务端扩展", "Downloading server extension"),
    verifying: text("校验扩展文件", "Verifying extension files"),
    installing: text("安装服务端扩展", "Installing server extension"),
    ready: text("服务端扩展已安装", "Server extension installed"),
    failed: text("服务端扩展安装失败", "Server extension installation failed")
  };
  return [instanceName, labels[snapshot.phase]].filter(Boolean).join(" · ");
}

export function ArkToolsInstallActivity({ snapshot, locale, issue, instanceName, observedAt }: {
  snapshot: ArkToolsInstallProgress; locale: string; issue: ArkToolsInstallObservationIssue | null;
  instanceName?: string; observedAt?: number;
}) {
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const { elapsedSeconds, bytesPerSecond } = useDownloadMetrics({
    operation_id: snapshot.operationId, active: snapshot.active, phase: snapshot.phase,
    downloaded_bytes: snapshot.downloadedBytes, total_bytes: snapshot.totalBytes, elapsed_seconds: snapshot.elapsedSeconds
  }, observedAt);
  const label = phaseLabel(snapshot, locale, instanceName);
  const bytes = snapshot.phase === "downloading" && snapshot.downloadedBytes !== null
    ? `${formatDownloadBytes(snapshot.downloadedBytes, locale)}${snapshot.totalBytes ? ` / ${formatDownloadBytes(snapshot.totalBytes, locale)}` : ""}` : "";
  const title = [label, snapshot.source, snapshot.error, issue?.message].filter(Boolean).join("\n");
  const issueLabel = issue?.kind === "receipt" ? text("安装回执未确认，继续检查", "Install receipt unconfirmed; checking")
    : issue?.kind === "verification" ? text("正在核实安装结果", "Checking installation outcome")
    : issue ? text("进度读取重试中", "Retrying progress check") : undefined;
  const speed = snapshot.active && snapshot.phase === "downloading"
    ? bytesPerSecond === null ? "—" : `${formatDownloadBytes(bytesPerSecond, locale)}/s` : "";
  return <ActivityProgress label={label} active={snapshot.active} error={snapshot.phase === "failed"}
      detail={issueLabel ?? snapshot.source ?? undefined}
      title={title} percent={arkToolsDownloadPercent(snapshot)} transfer={bytes}
      speed={speed} speedLabel={text("下载速度", "Download speed")}
      elapsedLabel={text(`已用时 ${elapsedSeconds} 秒`, `${elapsedSeconds}s elapsed`)} />;
}

export function useArkToolsActivity() {
  const { locale, t } = useI18n();
  const { store, entries } = useArkToolsInstallationEntries();
  const visible = entries.flatMap(entry => entry.progress && !entry.dismissed ? [{ entry, snapshot: entry.progress }] : []);
  return {
    progressEntries: visible.filter(({ entry }) => entry.observing).map(({ entry, snapshot }) => ({
      id: `ark-install:${snapshot.operationId}`,
      content: <ArkToolsInstallActivity snapshot={snapshot} locale={locale} issue={entry.issue}
        instanceName={entry.instanceName} observedAt={entry.observedAt} />
    })),
    outcomes: visible.filter(({ entry }) => !entry.observing).map(({ entry, snapshot }) => {
      return { id: `ark-install:${snapshot.operationId}`, content:
        <ActivityNoticeContent tone={snapshot.error ? "error" : "success"}
          details={[snapshot.source, snapshot.error].filter(Boolean).join("\n")}
          dismissLabel={t("servers.mods.closeToast", undefined, "Close")}
          onDismiss={() => store.dismiss(snapshot.operationId)}>
          {`${phaseLabel(snapshot, locale, entry.instanceName)} · ${t("steamcmd.elapsed", { seconds: snapshot.elapsedSeconds })}`}
        </ActivityNoticeContent> };
    })
  };
}
