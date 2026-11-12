import { useI18n } from "../../i18n";
import type { SteamWorkshopLookupItem } from "../../types";
import { SteamWorkshopPreview } from "./SteamWorkshopStoreDetail";
import { WorkshopStatus } from "./WorkshopStatus";
import type { WorkshopDownloadItemPresentation } from "./workshop-download-presentation";

export function WorkshopDownloadRows({ ids, lookup, downloads, errors, onOpen }: {
  ids: string[];
  lookup: Record<string, SteamWorkshopLookupItem>;
  downloads: Record<string, WorkshopDownloadItemPresentation>;
  errors: Record<string, string>;
  onOpen: (id: string) => void;
}) {
  const { t } = useI18n();
  return ids.map((id) => <div key={id} className="mw-entry-row mw-workshop-download-row" data-workshop-id={id}>
    <button type="button" className="mw-entry-row-select" onClick={() => onOpen(id)}>
      <span className="mw-entry-row-media"><SteamWorkshopPreview url={lookup[id]?.preview_url} loading /></span>
      <span className="mw-entry-row-copy">
        <span className="mw-entry-row-title mw-entry-row-title--singleline">{lookup[id]?.title ?? id}</span>
        <span className="mw-entry-row-status">
          {errors[id] ? <span className="mw-chip mw-chip--warn" title={errors[id]}>{t("status.job.failed", undefined, "Failed")}</span>
            : downloads[id] ? <WorkshopStatus state={downloads[id].state} progressPercent={downloads[id].progressPercent} /> : null}
          <span className="mw-entry-id">Steam Workshop / {id}</span>
        </span>
      </span>
    </button>
  </div>);
}
