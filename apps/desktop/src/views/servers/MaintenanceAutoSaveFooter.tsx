import { ActivityNotice } from "../../components/ActivityNotice";
import type { TranslateFn } from "../../i18n";
import type { InstanceSettingsSaveStatus } from "../settings/instance-settings-save-queue";

/** Typed maintenance values wait for a pause; toggles and selects save at once. */
export const MAINTENANCE_TYPING_DELAY_MS = 650;

interface Props {
  status: InstanceSettingsSaveStatus;
  /** Validation or a same-field conflict prevents the draft from being written. */
  blocked: boolean;
  conflictMessage?: string | null;
  edited: boolean;
  t: TranslateFn;
  onRetry(): void;
  onDiscard(): void;
}

/** Shared auto-save status row for Maintenance editors; it replaces per-section save buttons. */
export function MaintenanceAutoSaveFooter(props: Props) {
  const { status, t } = props;
  const failed = status.state === "failed" || status.state === "conflict";
  const state = props.blocked ? "blocked" : status.state;
  const tone = state === "saved" ? "success" : failed ? "error" : props.blocked ? "warning" : "info";
  const text = props.conflictMessage ? props.conflictMessage
    : props.blocked ? t("settings.configuration.save.blocked", undefined, "Fix invalid settings to save")
      : state === "saving" ? t("servers.backups.policySaving", undefined, "Saving policy…")
        : state === "dirty" ? t("settings.configuration.save.pending", undefined, "Changes waiting to save")
          : failed && "message" in status ? t("servers.backups.policyError", { message: status.message },
            `Unable to save policy: ${status.message}`)
            : t("servers.backups.policySaved", undefined, "Policy saved");
  return <div className="server-backup-policy-footer" data-save-state={state}>
    <ActivityNotice tone={tone}>{text}</ActivityNotice>
    {failed && !props.blocked ? <button className="secondary-button" type="button" onClick={props.onRetry}>
      {t("settings.configuration.save.retry", undefined, "Retry save")}
    </button> : null}
    {props.edited && (failed || props.blocked) ? <button className="ghost-button" type="button" onClick={props.onDiscard}>
      {t("servers.savePolicy.reload")}
    </button> : null}
  </div>;
}
