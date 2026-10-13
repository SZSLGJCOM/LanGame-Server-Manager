import { useI18n } from "../i18n";

export function ViewLoadingState(props: { error: Error | null; canRetry: boolean; onRetry: () => void }) {
  const { t } = useI18n();

  return (
    <div aria-live="polite" className="form-note" role={props.error ? "alert" : "status"}>
      <p>{props.error
        ? props.canRetry
          ? t("app.loadViewFailed", undefined, "This view could not be loaded. Try again.")
          : t("app.loadViewUnavailable", undefined, "This view still cannot be loaded. Your current work is preserved and other views remain available. Resolve the loading issue, then reopen the app.")
        : t("app.loadingView", undefined, "Loading view...")}</p>
      {props.error && props.canRetry ? <button className="secondary-button" type="button" onClick={props.onRetry}>
        {t("common.retry", undefined, "Retry")}
      </button> : null}
    </div>
  );
}
