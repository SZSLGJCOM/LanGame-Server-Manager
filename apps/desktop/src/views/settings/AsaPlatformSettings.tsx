import { useState } from "react";
import { useI18n } from "../../i18n";
import { instanceHasRunningProcess } from "../../runtime-action-state";
import { buildAsaModCompatibilityPlan, readAsaModCompatibility } from "../servers/asa-mod-compatibility";
import { buildConfigurationFieldIds } from "./ConfigurationField";
import type { ConfigurationSpecializedRendererProps } from "./module-types";
import "./asa-platform-settings.css";

export function AsaPlatformSettings(props: ConfigurationSpecializedRendererProps) {
  const { t } = useI18n();
  const [custom, setCustom] = useState(false);
  const current = readAsaModCompatibility(props.settings);
  const mode = custom ? "custom" : current.mode;
  const id = buildConfigurationFieldIds("server_platform", `configuration-${props.moduleDetails.summary.id}`).inputId;
  const running = instanceHasRunningProcess(props.details.summary, props.details.active_run)
    || ["starting", "stopping"].includes(String(props.details.summary.status).toLowerCase());
  const disabled = props.disabled || running;

  return <section className="asa-platform-settings" data-field-key="server_platform" aria-labelledby={`${id}-label`}>
    <label id={`${id}-label`} htmlFor={id}>{t("settings.asaPlatforms.label")}</label>
    <select id={id} className="settings-schema-input" data-asa-platform-mode value={mode}
      disabled={disabled || current.problem !== null} aria-describedby={`${id}-help${current.problem ? ` ${id}-problem` : ""}`}
      onChange={(event) => {
        if (disabled || current.problem) return;
        const selected = event.target.value;
        if (selected === "custom") { setCustom(true); return; }
        if (selected !== "steam" && selected !== "crossplay") return;
        const next = buildAsaModCompatibilityPlan(props.settings, selected);
        props.onPatch({ server_platform: next.server_platform, custom_launch_flags: next.custom_launch_flags });
        setCustom(false);
      }}>
      <option value="crossplay">{t("settings.asaPlatforms.crossplay")}</option>
      <option value="steam">{t("settings.asaPlatforms.steam")}</option>
      <option value="custom">{t("settings.asaPlatforms.custom")}</option>
    </select>
    <p id={`${id}-help`}>{t(`settings.asaPlatforms.${mode}Help`)}</p>
    {mode === "custom" ? <div className="asa-platform-settings__custom">
      <label htmlFor={`${id}-custom`}>{t("settings.asaPlatforms.customLabel")}</label>
      <input id={`${id}-custom`} className="settings-schema-input" disabled={disabled}
        value={String(props.settings.server_platform ?? "")} aria-describedby={`${id}-custom-help`}
        onChange={(event) => props.onPatch({ server_platform: event.target.value })} />
      <p id={`${id}-custom-help`}>{t("settings.asaPlatforms.customPlatformsHelp")}</p>
    </div> : null}
    {running ? <p>{t("settings.asaPlatforms.stopFirst")}</p> : null}
    {current.problem ? <p id={`${id}-problem`} role="alert">{t(`settings.asaPlatforms.${current.problem}`)}</p> : null}
  </section>;
}
