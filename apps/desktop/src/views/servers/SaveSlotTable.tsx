import { useId, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { shouldUseLanApi } from "../../api-transport";
import type { LocaleCode, TranslateFn } from "../../i18n";
import type { SaveFileListing, SaveSlot } from "../../types";
import { ShellIcon } from "../../components/ShellIcon";
import { formatRelativeTime, formatTime } from "../../view-models";
import { formatStorageSize } from "./format-storage-size";
import { MaintenanceAction, MaintenanceToolbar } from "./MaintenanceToolbar";
import { entriesUnder, ListingNotesHint, listingNotes, SaveFileTable, SaveListingNotices } from "./SaveFileTable";
import { StorageRecordList, StorageRecordRow } from "./StorageRecordList";
import { useSaveFileListing } from "./use-save-file-listing";

interface Props {
  instanceId: string;
  locale: LocaleCode;
  t: TranslateFn;
  onOpenLocalPath?: (path: string) => void;
}

/** Read-only world/save slots of the live saves directory as dense rows; secondary facts live in help bubbles. */
export function SaveSlotTable(props: Props) {
  if (!isTauri() && shouldUseLanApi()) {
    return <p className="form-note" role="note">{props.t("servers.saveFiles.localOnly")}</p>;
  }
  return <LocalSaveSlotTable {...props} />;
}

function joinPath(root: string, relative: string) {
  const separator = root.includes("\\") ? "\\" : "/";
  return relative ? `${root.replace(/[\\/]+$/, "")}${separator}${relative.split("/").join(separator)}` : root;
}

/** Folder to open for a slot: its own directory, or the folder holding its files. */
export function slotFolder(listing: SaveFileListing, slot: SaveSlot) {
  const [first] = slot.paths;
  if (slot.paths.length === 1 && listing.entries.some((entry) => entry.is_directory && entry.relative_path === first)) {
    return joinPath(listing.root_path, first);
  }
  const parent = first?.includes("/") ? first.slice(0, first.lastIndexOf("/")) : "";
  return joinPath(listing.root_path, parent);
}

export function slotName(slot: SaveSlot, t: TranslateFn) {
  return slot.kind === "files" ? t("servers.saveSlots.looseFiles") : slot.name;
}

function LocalSaveSlotTable({ instanceId, locale, t, onOpenLocalPath }: Props) {
  const { listing, error, loading, reload } = useSaveFileListing(instanceId, null);
  const title = t("servers.saveSlots.title");
  const notes = [
    ...(listing?.exists && listing.slots.length > 0 && listing.slot_model === "fallback" ? [t("servers.saveSlots.fallbackNote")] : []),
    ...listingNotes(listing, t)
  ];
  return <div className="save-file-browser save-slot-table" aria-busy={loading}>
    <div className="server-workbench-section-label server-maintenance-card-label">
      <ShellIcon name="folder" className="server-workbench-section-icon" />
      <span>{title}</span>
      <ListingNotesHint notes={notes} label={title} />
      <MaintenanceToolbar label={title}>
        <MaintenanceAction icon="refresh" iconOnly label={t("common.refresh")} busy={loading} onClick={() => void reload()} />
        {onOpenLocalPath ? <MaintenanceAction icon="folder" iconOnly label={t("servers.saveSlots.openSaves")}
          help={listing?.exists ? `${t("servers.saveSlots.openSaves")}\n${listing.root_path}` : null}
          disabledReason={listing?.exists ? null : t("servers.saveFiles.missing")}
          onClick={() => { if (listing?.exists) onOpenLocalPath(listing.root_path); }} /> : null}
      </MaintenanceToolbar>
    </div>
    {listing && listing.slots.length > 0 ? <StorageRecordList label={title}>
      {listing.slots.map((slot) => <SaveSlotRow key={slot.id} listing={listing} slot={slot} locale={locale} t={t}
        onOpenLocalPath={onOpenLocalPath} />)}
    </StorageRecordList> : null}
    <SaveListingNotices listing={listing} loading={loading} error={error} t={t} />
  </div>;
}

function SaveSlotRow({ listing, slot, locale, t, onOpenLocalPath }: {
  listing: SaveFileListing; slot: SaveSlot; locale: LocaleCode; t: TranslateFn; onOpenLocalPath?: (path: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const regionId = useId();
  const name = slotName(slot, t);
  const folder = slotFolder(listing, slot);
  const details = [
    slot.label && slot.label.value !== slot.name ? t(`servers.saveSlots.label.${slot.label.kind}`, { value: slot.label.value }) : null,
    slot.modified_at_unix_ms == null ? t("servers.saveSlots.neverSaved")
      : t("servers.saveSlots.lastSaved", { time: formatTime(locale, slot.modified_at_unix_ms) }),
    t("servers.backups.filesMeta", { count: slot.file_count, size: formatStorageSize(slot.size_bytes) }),
    folder
  ].filter(Boolean).join("\n");
  return <StorageRecordRow name={name} current={slot.current} details={details}
    badge={slot.current ? <span className="server-file-backup-kind server-file-backup-kind--current">{t("servers.saveSlots.current")}</span> : null}
    facts={<>
      <span>{formatStorageSize(slot.size_bytes)}</span>
      <span>{slot.modified_at_unix_ms == null ? t("servers.saveSlots.neverSaved") : formatRelativeTime(locale, slot.modified_at_unix_ms)}</span>
    </>} actions={<>
      <MaintenanceAction icon="list" iconOnly label={t(open ? "servers.backups.hideFiles" : "servers.backups.showFiles")}
        aria-expanded={open} aria-controls={regionId} onClick={() => setOpen((value) => !value)} />
      {onOpenLocalPath ? <MaintenanceAction icon="folder" iconOnly label={t("servers.saveFiles.openFolder")}
        help={`${t("servers.saveFiles.openFolder")}\n${folder}`} onClick={() => onOpenLocalPath(folder)} /> : null}
    </>}>
    {open ? <div id={regionId} className="server-file-backup-files">
      <SaveFileTable entries={entriesUnder(listing.entries, slot.paths)} label={name} locale={locale} t={t} />
    </div> : null}
  </StorageRecordRow>;
}
