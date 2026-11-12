import { ShellIcon } from "../../components/ShellIcon";
import { useI18n } from "../../i18n";
import type { WorkshopLifecycleState } from "./steam-workshop-store-model";

/** Normal membership is already expressed by the action or enablement control. */
export function WorkshopStatus({ state, progressPercent }: {
  state: WorkshopLifecycleState;
  progressPercent?: number | null;
}) {
  const { t } = useI18n();
  if (state === "enabled" || state === "downloaded" || state === "not-installed") return null;
  const label = t(`servers.mods.lifecycle.${state}`, undefined, state);
  return <span className={`mw-chip mw-chip--lifecycle mw-chip--lifecycle-${state}`}>
    {state === "checking" ? <ShellIcon name="loader" className="mw-btn-icon mw-btn-icon--spin" /> : null}
    {state === "queued" || state === "ready" ? <ShellIcon name="clock" className="mw-btn-icon" /> : null}
    {label}{(state === "installing" || state === "applying") && progressPercent != null ? ` ${progressPercent}%` : ""}
  </span>;
}
