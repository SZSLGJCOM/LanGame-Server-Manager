import { isTauri } from "@tauri-apps/api/core";
import { shouldUseLanApi } from "../../api-transport";
import type { LocaleCode, TranslateFn } from "../../i18n";
import { ListingNotesHint, listingNotes, SaveFileTable, SaveListingNotices } from "./SaveFileTable";
import { useSaveFileListing } from "./use-save-file-listing";

interface Props {
  instanceId: string;
  backupId: string;
  title: string;
  locale: LocaleCode;
  t: TranslateFn;
}

/** Read-only contents of one backup, loaded when its row expands. The backend resolves and confines the root. */
export function SaveFileBrowser(props: Props) {
  if (!isTauri() && shouldUseLanApi()) {
    return <p className="form-note" role="note">{props.t("servers.saveFiles.localOnly")}</p>;
  }
  return <LocalSaveFileBrowser {...props} />;
}

function LocalSaveFileBrowser({ instanceId, backupId, title, locale, t }: Props) {
  const { listing, error, loading } = useSaveFileListing(instanceId, backupId);
  return <div className="save-file-browser" aria-busy={loading}>
    {listing && listing.entries.length > 0 ? <SaveFileTable entries={listing.entries} label={title} locale={locale} t={t} /> : null}
    <ListingNotesHint notes={listingNotes(listing, t)} label={title} />
    <SaveListingNotices listing={listing} loading={loading} error={error} t={t} />
  </div>;
}
