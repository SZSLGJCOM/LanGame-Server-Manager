import { useI18n } from "../i18n";
import type { AppUpdateState } from "../types";
import { ActivityNotice } from "./ActivityNotice";
import { appUpdateCopy } from "./app-update-copy";
import "./app-update.css";

export function AppUpdateActivity({ state, onCheck }: { state: AppUpdateState; onCheck: () => void }) {
  const { locale } = useI18n();
  const copy = appUpdateCopy(locale);
  if (state.status !== "downloading" && state.status !== "installing" && state.status !== "failed") return null;
  const failed = state.status === "failed";
  const percent = state.status === "downloading" && (state.contentLength || state.downloadPercent === 100)
    ? state.downloadPercent : undefined;
  return <ActivityNotice tone={failed ? "error" : "info"} action={failed
    ? <button type="button" className="app-update-action" onClick={onCheck}>{copy.retry}</button>
    : <div className="shell-update-progress">
      <progress aria-label={copy[state.status]} max={100} value={percent} />
      {percent !== undefined ? <span>{percent}%</span> : null}
    </div>}>
    {`${copy[state.status]}${state.availableVersion ? ` · ${state.availableVersion}` : ""} · ${failed
      ? state.error || copy.failedDetail : state.status === "installing" ? copy.installingDetail : copy.downloadingDetail}`}
  </ActivityNotice>;
}
