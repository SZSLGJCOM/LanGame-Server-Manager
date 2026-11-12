import { ActivityNotice } from "../../components/ActivityNotice";
import { useI18n } from "../../i18n";
import { isWorkshopDownloadPending, type WorkshopDownloadRequestState } from "./workshop-download-operations";
import { workshopRequestPresentation } from "./workshop-download-presentation";

export function WorkshopDownloadActivity({ requests }: { requests: WorkshopDownloadRequestState[] }) {
  const { t } = useI18n();
  return requests.filter(isWorkshopDownloadPending).map((request) => {
    const names = request.ids.slice(0, 2).map((id) => request.items[id]?.title || id).join(", ");
    const progress = workshopRequestPresentation(request);
    const label = t(`servers.mods.lifecycle.${progress.state}`, undefined, progress.state);
    return <ActivityNotice key={request.requestId}>{`${request.ids.length > 2 ? `${names}…` : names} · ${label}${progress.progressPercent === null ? "" : ` ${progress.progressPercent}%`}`}</ActivityNotice>;
  });
}
