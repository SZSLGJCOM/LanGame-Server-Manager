import { useI18n } from "../i18n";
import type { StorageInitializationState } from "../storage-initialization";

interface StorageInitializationViewProps {
  state: StorageInitializationState;
  onRetry: () => void;
}

export function StorageInitializationView(props: StorageInitializationViewProps) {
  const { t } = useI18n();

  if (props.state.status === "ready") {
    return null;
  }

  if (props.state.status === "pending") {
    const retrying = props.state.attempt > 0;
    return (
      <section
        className="storage-initialization-page is-loading"
        role="status"
        aria-live="polite"
        aria-busy="true"
      >
        <div className="storage-initialization-content">
          <h1>
            {t(
              retrying
                ? "storage.initialization.retryingTitle"
                : "storage.initialization.loadingTitle"
            )}
          </h1>
          <p>{t("storage.initialization.loadingBody")}</p>
        </div>
      </section>
    );
  }

  return (
    <section className="storage-initialization-page is-failed">
      <div className="storage-initialization-content">
        <header
          className="storage-initialization-header"
          role="alert"
          aria-labelledby="storage-initialization-title"
        >
          <h1 id="storage-initialization-title">
            {t("storage.initialization.failedTitle")}
          </h1>
          <p>{t("storage.initialization.failedBody")}</p>
        </header>

        <details className="storage-initialization-details">
          <summary>
            {t("storage.initialization.errorDetails")}
            <span className="storage-initialization-error-preview">{props.state.error}</span>
          </summary>
          <pre className="storage-initialization-error">{props.state.error}</pre>
          {props.state.logPath && (
            <dl className="storage-initialization-log">
              <dt>{t("storage.initialization.logPath")}</dt>
              <dd>{props.state.logPath}</dd>
            </dl>
          )}
        </details>

        <button
          type="button"
          className="storage-initialization-retry-button"
          onClick={props.onRetry}
        >
          {t("storage.initialization.retry")}
        </button>
      </div>
    </section>
  );
}
