import { useI18n } from "../../i18n";
import retiredOptions from "../../../../../modules/projectzomboid/retired-server-options.json";
import type { ConfigurationSpecializedRendererProps } from "./module-types";
import type { SettingsObject } from "./settings-schema";

export const PROJECT_ZOMBOID_REVIEW_KEY = "projectzomboid_b42_policy_reviewed";

export function customizedRetiredProjectZomboidOptions(settings: Readonly<SettingsObject>): string[] {
  return Object.entries(retiredOptions)
    .filter(([key, entry]) => Object.prototype.hasOwnProperty.call(settings, key) && settings[key] !== entry.default)
    .map(([, entry]) => entry.native_key);
}

export function ProjectZomboidPolicyReview(props: ConfigurationSpecializedRendererProps) {
  const { t } = useI18n();
  const affected = customizedRetiredProjectZomboidOptions(props.settings);
  if (affected.length === 0) return null;
  return <div className="configuration-workspace__notice" data-projectzomboid-policy-review>
    <p>{t("projectzomboid.settings.review.description", undefined,
      "Build 42 no longer reads these customized options. Review the current configuration and Sandbox Lua before starting. Your previous values remain saved.")}</p>
    <p>{affected.join(", ")}</p>
    <label className="configuration-field settings-schema-field">
      <input type="checkbox" checked={props.settings[PROJECT_ZOMBOID_REVIEW_KEY] === true}
        disabled={props.disabled} onChange={(event) => props.onPatch({ [PROJECT_ZOMBOID_REVIEW_KEY]: event.currentTarget.checked })} />
      {t("projectzomboid.settings.review.confirm", undefined,
        "I have reviewed the current settings. Save to confirm before starting.")}
    </label>
  </div>;
}
