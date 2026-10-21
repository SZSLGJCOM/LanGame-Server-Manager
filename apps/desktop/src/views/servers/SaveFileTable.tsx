import type { LocaleCode, TranslateFn } from "../../i18n";
import type { SaveFileEntry, SaveFileListing } from "../../types";
import { formatTime } from "../../view-models";
import { ActivityNotice } from "../../components/ActivityNotice";
import { MaintenanceHelp } from "./MaintenanceToolbar";
import { formatStorageSize } from "./format-storage-size";
import "./save-file-browser.css";

interface TableProps {
  entries: SaveFileEntry[];
  label: string;
  locale: LocaleCode;
  t: TranslateFn;
}

/** Read-only path / size / modified table shared by the save slots and backup contents. */
export function SaveFileTable({ entries, label, locale, t }: TableProps) {
  return <div className="save-file-browser__scroll" tabIndex={0} role="region" aria-label={label}>
    <table className="save-file-browser__table">
      <thead><tr>
        <th scope="col">{t("servers.saveFiles.columnPath")}</th>
        <th scope="col">{t("servers.saveFiles.columnSize")}</th>
        <th scope="col">{t("servers.saveFiles.columnModified")}</th>
      </tr></thead>
      <tbody>{entries.map((entry) => <tr key={entry.relative_path} data-directory={entry.is_directory || undefined}>
        <td><code>{entry.is_directory ? `${entry.relative_path}/` : entry.relative_path}</code></td>
        <td>{entry.is_directory ? "—" : formatStorageSize(entry.size_bytes)}</td>
        <td>{entry.modified_at_unix_ms == null ? "—" : formatTime(locale, entry.modified_at_unix_ms)}</td>
      </tr>)}</tbody>
    </table>
  </div>;
}

/** Loading and error go to the activity bar; missing/empty stay as one quiet line. */
export function SaveListingNotices({ listing, loading, error, t }: {
  listing: SaveFileListing | null; loading: boolean; error: string | null; t: TranslateFn;
}) {
  return <>
    {loading && !listing ? <ActivityNotice>{t("servers.saveFiles.loading")}</ActivityNotice> : null}
    {error ? <ActivityNotice tone="error">{t("servers.saveFiles.error", { message: error })}</ActivityNotice> : null}
    {listing && !listing.exists ? <p className="form-note save-file-browser__empty" role="status">{t("servers.saveFiles.missing")}</p> : null}
    {listing?.exists && listing.entries.length === 0 ? <p className="form-note save-file-browser__empty" role="status">{t("servers.saveFiles.empty")}</p> : null}
  </>;
}

/** Truncation and skipped links, read in a help bubble rather than as inline blocks. */
export function listingNotes(listing: SaveFileListing | null, t: TranslateFn): string[] {
  if (!listing) return [];
  return [
    ...(listing.truncated ? [t("servers.saveFiles.truncated", { count: listing.entries.length })] : []),
    ...(listing.skipped_links.length > 0
      ? [t("servers.saveFiles.skippedLinks", { count: listing.skipped_links.length, names: listing.skipped_links.join(", ") })] : [])
  ];
}

export function ListingNotesHint({ notes, label }: { notes: string[]; label: string }) {
  return notes.length ? <MaintenanceHelp className="save-file-browser__hint" label={label} help={notes.join("\n")} /> : null;
}

/** Entries covered by a slot's relative roots. */
export function entriesUnder(entries: SaveFileEntry[], roots: string[]) {
  return entries.filter((entry) => roots.some((root) => entry.relative_path === root || entry.relative_path.startsWith(`${root}/`)));
}
