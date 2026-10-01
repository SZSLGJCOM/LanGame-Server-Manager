import { useId, type ReactNode } from "react";
import { ActivityNotice } from "../../components/ActivityNotice";
import { ShellIcon } from "../../components/ShellIcon";
import { useI18n } from "../../i18n";
import "../../styles/configuration-load-error.css";

interface ConfigurationLoadErrorProps {
  error: string;
  onRetry?: () => void;
  children?: ReactNode;
  repair?: { value: string; onChange: (value: string) => void };
}

export function ConfigurationLoadError({ error, onRetry, repair, children }: ConfigurationLoadErrorProps) {
  const { t } = useI18n();
  const id = useId();
  const title = t("settings.configuration.workspace.error", undefined, "Configuration could not be loaded.");

  return <section className="configuration-load-error" data-configuration-state="error"
    aria-labelledby={`${id}-title`} aria-describedby={`${id}-description`}>
    <ActivityNotice key={error} tone="error">{title}</ActivityNotice>
    <div className="configuration-load-error__content">
      <div className="configuration-load-error__heading">
        <ShellIcon name="alert-circle" aria-hidden="true" />
        <h2 id={`${id}-title`}>{title}</h2>
      </div>
      <p id={`${id}-description`} className="configuration-load-error__description">
        {onRetry
          ? t("settings.configuration.workspace.retryHint", undefined,
            "Configuration editing is paused. Reload the configuration to continue.")
          : repair
            ? t("settings.configuration.workspace.repairHint", undefined,
              "Correct the JSON below to continue editing.")
            : t("settings.configuration.workspace.schemaErrorHint", undefined,
              "The configuration definition could not be read. Check the error details and the game module.")}
      </p>
      {onRetry ? <button type="button" className="secondary-button configuration-load-error__retry" onClick={onRetry}>
        <ShellIcon name="refresh" aria-hidden="true" />
        {t("settings.configuration.workspace.retry", undefined, "Reload configuration")}
      </button> : null}
      <details className="configuration-load-error__details">
        <summary>{t("settings.configuration.workspace.errorDetails", undefined, "Error details")}</summary>
        <pre tabIndex={0}>{error}</pre>
      </details>
      {repair ? <label className="configuration-load-error__repair">
        <span>{t("settings.details.settingsJson", undefined, "Settings JSON")}</span>
        <textarea className="settings-schema-input settings-schema-textarea"
          spellCheck={false} value={repair.value} onChange={(event) => repair.onChange(event.target.value)} />
      </label> : null}
      {children}
    </div>
  </section>;
}
