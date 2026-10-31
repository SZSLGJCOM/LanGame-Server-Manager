import { ActivityNotice } from "../../components/ActivityNotice";
import { selectLocaleText, useI18n } from "../../i18n";
import { focusConfigurationControl } from "./configuration-workspace-state";

/** Local drafts in native editors are not part of the workspace's schema validation. */
export function ConfigurationValidationNotice({ message, inputId, label }: {
  message: string | null | undefined;
  inputId: string;
  label?: string;
}) {
  const { locale } = useI18n();
  if (!message) return null;
  return <ActivityNotice key={`${inputId}\u0000${message}`} tone="error" action={
    <button type="button" className="secondary-button configuration-validation-target"
      data-configuration-validation-target={inputId} aria-controls={inputId}
      onClick={() => focusConfigurationControl(inputId)}>
      {label ?? selectLocaleText(locale, "定位设置", "Go to setting")}
    </button>
  }>{message}</ActivityNotice>;
}
