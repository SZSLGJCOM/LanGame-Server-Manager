import { useI18n } from "../../i18n";
import { ConfigurationHelp } from "../settings/ConfigurationFieldHelp";
import type { SettingsObject } from "../settings/settings-schema";
import { readAsaModCompatibility } from "./asa-mod-compatibility";

export function AsaModCompatibility(props: {
  settings: SettingsObject;
  onOpenSettings?: (fieldKey: string) => void;
}) {
  const { t } = useI18n();
  const current = readAsaModCompatibility(props.settings);
  const action = props.onOpenSettings ? t("servers.mods.asaCompatibility.openSettingsHint") : "";
  const description = [t(`servers.mods.asaCompatibility.${current.mode}Help`), action].filter(Boolean).join("\n\n");
  return <ConfigurationHelp description={description}>{(help) =>
    <button type="button" className="mw-manifest-help-trigger asa-mod-compatibility" data-asa-compatibility-help
      data-asa-open-settings={props.onOpenSettings ? "" : undefined}
      ref={help.anchorRef} {...help.interactionProps}
      aria-label={[t("servers.mods.asaCompatibility.label"), action].filter(Boolean).join(" · ")}
      aria-describedby={help.descriptionId}
      onClick={() => props.onOpenSettings?.("server_platform")}>
      <span aria-hidden="true">?</span>
    </button>}
  </ConfigurationHelp>;
}
