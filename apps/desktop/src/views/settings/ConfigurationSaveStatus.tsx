import { ActivityNotice } from "../../components/ActivityNotice";
import type { TranslateFn } from "../../i18n";
import type { InstanceSettingsSaveStatus } from "./instance-settings-save-queue";

interface ConfigurationSaveStatusProps {
  status: InstanceSettingsSaveStatus;
  validationBlocked: boolean;
  t: TranslateFn;
}

export function ConfigurationSaveStatus(props: ConfigurationSaveStatusProps) {
  const state = props.validationBlocked ? "blocked" : props.status.state;
  const labels = {
    saved: props.t("settings.configuration.save.saved", undefined, "Configuration saved"),
    dirty: props.t("settings.configuration.save.pending", undefined, "Changes waiting to save"),
    saving: props.t("settings.configuration.save.saving", undefined, "Saving configuration…"),
    blocked: props.t("settings.configuration.save.blocked", undefined, "Fix invalid settings to save"),
    failed: props.t("settings.configuration.save.unsaved", undefined, "Changes not saved"),
    conflict: props.t("settings.configuration.save.unsaved", undefined, "Changes not saved")
  };
  const tone = state === "saved" ? "success" : state === "failed" || state === "conflict" ? "error"
    : state === "blocked" ? "warning" : "info";
  return <ActivityNotice tone={tone}>{labels[state]}</ActivityNotice>;
}
