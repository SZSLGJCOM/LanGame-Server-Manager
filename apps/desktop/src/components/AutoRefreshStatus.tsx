import { formatDateTime, useI18n, type LocaleCode } from "../i18n";
import type { RuntimeRefreshIssue } from "../app-state";

interface AutoRefreshStatusProps {
  intervalMs: number;
  issue: RuntimeRefreshIssue | null;
  paused: boolean;
  failureLimit: number;
  onResume: () => void;
}

function formatRefreshTime(locale: LocaleCode, timestamp: number): string {
  return formatDateTime(locale, timestamp, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit"
  });
}

export function AutoRefreshStatus(props: AutoRefreshStatusProps) {
  const { locale, t } = useI18n();
  if (!props.issue && !props.paused) {
    return null;
  }

  const intervalSeconds = Math.max(1, Math.round(props.intervalMs / 1000));
  const cadence = props.paused
    ? t("autoRefresh.cadenceAfterResume", { seconds: intervalSeconds })
    : t("autoRefresh.cadence", { seconds: intervalSeconds });
  const failureMeta = props.issue
    ? props.paused
      ? t("autoRefresh.pausedAfterFailures", {
          count: Math.max(props.issue.consecutiveFailures, props.failureLimit)
        })
      : t("autoRefresh.recentFailure", {
          time: formatRefreshTime(locale, props.issue.failedAt),
          count: props.issue.consecutiveFailures
        })
    : null;
  const label = failureMeta ?? t("autoRefresh.paused");
  const toneClass = props.paused ? "is-paused" : "is-throttled";
  const details = [label, cadence, props.issue?.message].filter(Boolean).join("\n");

  return (
    <div className={`auto-refresh-status ${toneClass}`} title={details}>
      <span className="auto-refresh-status-dot" aria-hidden="true" />
      <span className={`shell-activity-text${props.issue ? " auto-refresh-status-alert-message" : ""}`}
        role={props.issue ? "alert" : undefined} title={props.issue?.message ?? details}>
        {props.issue?.message ?? label}
      </span>
      {props.paused ? (
        <button type="button" className="auto-refresh-status-resume" onClick={props.onResume}>
          {t("autoRefresh.resume")}
        </button>
      ) : null}
    </div>
  );
}
