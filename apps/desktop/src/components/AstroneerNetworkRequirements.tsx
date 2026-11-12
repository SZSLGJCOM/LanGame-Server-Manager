import { useI18n } from "../i18n";
import { ConfigurationHelp } from "../views/settings/ConfigurationFieldHelp";
import "./astroneer-network-requirements.css";

export function AstroneerNetworkRequirements({ showReachability = false }: { showReachability?: boolean }) {
  const { t } = useI18n();
  const description = [t("astroneer.network.requirements.online"), t("astroneer.network.requirements.inbound"),
    t("astroneer.network.requirements.discoveryLimit"), showReachability ? t("astroneer.network.requirements.unverified") : null]
    .filter(Boolean).join("\n\n");
  return <ConfigurationHelp description={description}>{(help) =>
    <button type="button" className="astroneer-network-requirements" ref={help.anchorRef} {...help.interactionProps}
      aria-describedby={help.descriptionId}>
      <span className="astroneer-network-requirements__icon" aria-hidden="true">?</span>
      {t("astroneer.network.requirements.title")}
    </button>}
  </ConfigurationHelp>;
}
