import { useI18n } from "../../i18n";
import type { SteamWorkshopLookupItem } from "../../types";
import { SteamWorkshopPreview } from "./SteamWorkshopStoreDetail";
import { WorkshopStatus } from "./WorkshopStatus";

export function WorkshopDownloadRows({ ids, lookup, progressPercent, error, onOpen }: {
  ids: string[];
  lookup: Record<string, SteamWorkshopLookupItem>;
  progressPercent: number | null;
  error: string | null;
  onOpen: (id: string) => void;
}) {
  const { t } = useI18n();
  return ids.map((id) => <div key={id} className="mw-entry-row mw-workshop-download-row" data-workshop-id={id}>
    <button type="button" className="mw-entry-row-select" onClick={() => onOpen(id)}>
      <span className="mw-entry-row-media"><SteamWorkshopPreview url={lookup[id]?.preview_url} loading /></span>
      <span className="mw-entry-row-copy">
        <span className="mw-entry-row-title mw-entry-row-title--singleline">{lookup[id]?.title ?? id}</span>
        <span className="mw-entry-row-status">
          {error ? <span className="mw-chip mw-chip--warn" title={error}>{t("status.job.failed", undefined, "Failed")}</span>
            : <WorkshopStatus state="installing" progressPercent={progressPercent} />}
          <span className="mw-entry-id">Steam Workshop / {id}</span>
        </span>
      </span>
    </button>
  </div>);
}
