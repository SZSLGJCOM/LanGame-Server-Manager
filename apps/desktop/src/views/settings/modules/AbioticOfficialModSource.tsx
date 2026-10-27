import { useId } from "react";
import { useI18n } from "../../../i18n";
import { ConfigurationFieldResourceLink } from "../ConfigurationFieldResourceLink";

/** A section reference, separate from credential-acquisition field labels. */
export function AbioticOfficialModSource() {
  const { t } = useI18n();
  const id = useId();
  return <p className="form-note">
    <ConfigurationFieldResourceLink id={`abiotic-mod-catalog-${id}`}
      url="https://mod.io/g/abiotic-factor" t={t}>
      {t("abiotic.settings.mods.browseCatalog", undefined, "Browse official Abiotic Factor Mods on mod.io")}
    </ConfigurationFieldResourceLink>
  </p>;
}
