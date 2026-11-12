import { ActivityNotice } from "../../components/ActivityNotice";
import { useI18n } from "../../i18n";
import type { WorkshopDownloadRequestState } from "./workshop-download-operations";

/** Retry the original request so a download-only list keeps its original intent. */
export function WorkshopDownloadFailures({ requests, disabled, onRetry }: {
  requests: WorkshopDownloadRequestState[];
  disabled: boolean;
  onRetry: (requestId: number) => Promise<void> | null;
}) {
  const { t } = useI18n();
  return requests.filter((request) => request.status === "error").map((request) => {
    const names = request.ids.slice(0, 3).map((id) => request.items[id]?.title || id).join(", ");
    const title = request.ids.length > 3 ? `${names}…` : names;
    return <ActivityNotice key={request.requestId} tone="error" action={
      <button type="button" className="mw-ghost-btn" disabled={disabled} onClick={() => {
        // The queue retains and renders a failed retry, including its start barrier.
        void onRetry(request.requestId)?.catch(() => undefined);
      }}>{t("common.retry", undefined, "Retry")}</button>
    }>{`${title}: ${request.error}`}</ActivityNotice>;
  });
}
