import { useEffect, useId, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import type { SteamWorkshopLookupItem } from "../../types";
import type { ManagedWorkshopCollection } from "./mod-workbench-collections";
import "./workshop-collection-removal-dialog.css";

interface Props {
  collection: ManagedWorkshopCollection;
  protectedIds: ReadonlySet<string>;
  unavailableReason: string | null;
  lookup: Record<string, SteamWorkshopLookupItem>;
  busy: boolean;
  error: string | null;
  onClose: () => void;
  onRemove: (memberIds: string[]) => void;
}

export function WorkshopCollectionRemovalDialog(props: Props) {
  const { t } = useI18n();
  const titleId = useId();
  const descriptionId = useId();
  const dialog = useRef<HTMLDialogElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const [selected, setSelected] = useState(() => new Set(props.collection.member_ids.filter((id) => !props.protectedIds.has(id))));
  const selectedIds = props.collection.member_ids.filter((id) => selected.has(id) && !props.protectedIds.has(id));
  const memberNames = new Map(props.lookup[props.collection.id]?.children.map((child) => [child.id, child.title]) ?? []);

  useEffect(() => {
    const node = dialog.current;
    if (!node) return;
    const previous = document.activeElement;
    node.showModal();
    cancel.current?.focus();
    return () => {
      node.close();
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, []);

  useEffect(() => {
    if (!props.busy) return;
    // A browser close request can emit a non-cancelable cancel event. Stop the
    // Escape default action before it reaches that event while a save is pending.
    const preventBusyEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && dialog.current?.open) event.preventDefault();
    };
    document.addEventListener("keydown", preventBusyEscape, true);
    return () => document.removeEventListener("keydown", preventBusyEscape, true);
  }, [props.busy]);

  function close() { if (!props.busy) props.onClose(); }
  function remove(ids: string[]) { if (!props.busy) props.onRemove(ids); }

  return <dialog ref={dialog} className="mw-collection-removal-dialog" aria-labelledby={titleId}
    aria-describedby={descriptionId} aria-busy={props.busy} data-no-window-drag="true"
    onCancel={(event) => { event.preventDefault(); close(); }}>
    <header className="mw-collection-removal-header">
      <h2 id={titleId}>{t("servers.mods.collections.removeDialog.title", { name: props.collection.title }, "Remove {name}")}</h2>
    </header>
    <div className="mw-collection-removal-body">
      <p id={descriptionId}>{t("servers.mods.collections.removeDialog.scope", undefined,
        "Choose the Mods to remove from this instance. Downloaded files and saved Mod options are kept.")}</p>
      {props.unavailableReason ? <p className="mw-collection-removal-warning">{props.unavailableReason}</p> : null}
      {props.collection.member_ids.length ? <fieldset disabled={props.busy || Boolean(props.unavailableReason)}>
        <legend>{t("servers.mods.collections.removeDialog.members", undefined, "Mods to remove")}
          <span>{t("servers.mods.collections.removeDialog.selectedCount", { count: selectedIds.length }, "{count} selected")}</span>
        </legend>
        <div className="mw-collection-removal-members">
          {props.collection.member_ids.map((id) => {
            const shared = props.protectedIds.has(id);
            const title = props.lookup[id]?.title || memberNames.get(id);
            return <label key={id} className={`mw-collection-removal-member${shared ? " is-shared" : ""}`}>
              <input type="checkbox" value={id} checked={!shared && selected.has(id)} disabled={shared}
                onChange={(event) => {
                  const checked = event.target.checked;
                  setSelected((current) => {
                    const next = new Set(current);
                    if (checked) next.add(id); else next.delete(id);
                    return next;
                  });
                }} />
              <span className="mw-collection-removal-member-name">{title || id}
                {title && title !== id ? <small>#{id}</small> : null}
              </span>
              {shared ? <span className="mw-collection-removal-shared">{t("servers.mods.collections.removeDialog.shared", undefined, "Shared")}</span> : null}
            </label>;
          })}
        </div>
        {props.collection.member_ids.some((id) => props.protectedIds.has(id)) ? <p className="mw-collection-removal-hint">
          {t("servers.mods.collections.removeDialog.sharedHint", undefined, "Mods also used by another collection are kept.")}
        </p> : null}
      </fieldset> : <p className="mw-collection-removal-hint">{t("servers.mods.collections.removeDialog.empty", undefined,
        "No saved members are available. You can remove the collection record only.")}</p>}
    </div>
    <footer className="mw-collection-removal-footer">
      {props.error ? <p role="alert" className="mw-collection-removal-error">{props.error}</p> : null}
      <div className="mw-collection-removal-actions">
        <button ref={cancel} type="button" className="mw-ghost-btn" disabled={props.busy} onClick={close}>{t("common.cancel")}</button>
        <button type="button" className="mw-ghost-btn" disabled={props.busy} onClick={() => remove([])}>
          {t("servers.mods.collections.removeDialog.recordOnly", undefined, "Remove collection record only")}
        </button>
        <button type="button" className="mw-btn mw-collection-removal-confirm"
          disabled={props.busy || Boolean(props.unavailableReason) || selectedIds.length === 0} onClick={() => remove(selectedIds)}>
          {props.busy ? t("common.processing") : t("servers.mods.collections.removeDialog.selected", undefined, "Remove collection and selected Mods")}
        </button>
      </div>
    </footer>
  </dialog>;
}
