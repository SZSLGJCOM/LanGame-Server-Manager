import { useEffect, useId, useRef } from "react";
import { useI18n } from "../../i18n";
import type { ManualModInventoryItem } from "../../types";
import "./workshop-collection-removal-dialog.css";

interface Props {
  item: ManualModInventoryItem;
  busy: boolean;
  blocked: boolean;
  error: string | null;
  onClose: () => void;
  onDelete: () => void;
}

export function ManagedModDeleteDialog(props: Props) {
  const { t } = useI18n();
  const titleId = useId();
  const descriptionId = useId();
  const dialog = useRef<HTMLDialogElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const management = props.item.management;

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
    const preventBusyEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && dialog.current?.open) event.preventDefault();
    };
    document.addEventListener("keydown", preventBusyEscape, true);
    return () => document.removeEventListener("keydown", preventBusyEscape, true);
  }, [props.busy]);

  return <dialog ref={dialog} className="mw-collection-removal-dialog mw-managed-delete-dialog"
    aria-labelledby={titleId} aria-describedby={descriptionId} aria-busy={props.busy} data-no-window-drag="true"
    onCancel={(event) => { event.preventDefault(); if (!props.busy) props.onClose(); }}>
    <header className="mw-collection-removal-header"><h2 id={titleId}>
      {t("servers.mods.managed.deleteTitle", { name: props.item.name }, "Delete {name}?")}
    </h2></header>
    <div className="mw-collection-removal-body">
      <p id={descriptionId}>{t("servers.mods.managed.deleteScope", undefined,
        "Remove this Mod's recorded, unchanged program files from this instance. Configuration, modified files and unowned files are kept. Dependencies are not deleted.")}</p>
      <dl className="mw-entry-info-grid">
        <div className="mw-entry-info-row"><dt>{t("servers.mods.managed.dependencies", undefined, "Requires")}</dt>
          <dd>{management?.dependencies.join(", ") || t("servers.mods.managed.noneDeclared", undefined, "None declared")}</dd></div>
        <div className="mw-entry-info-row"><dt>{t("servers.mods.managed.dependents", undefined, "Required by")}</dt>
          <dd>{management?.dependents.join(", ") || t("servers.mods.managed.noneKnown", undefined, "None known")}</dd></div>
      </dl>
      {management?.dependency_status !== "complete" ? <p className="mw-collection-removal-hint">
        {t("servers.mods.managed.dependenciesUnknown", undefined, "A complete dependency declaration was not provided.")}
      </p> : null}
      {props.blocked ? <p className="mw-collection-removal-warning">{t("servers.mods.stopBeforeChanges")}</p> : null}
    </div>
    <footer className="mw-collection-removal-footer">
      {props.error ? <p role="alert" className="mw-collection-removal-error">{props.error}</p> : null}
      <div className="mw-collection-removal-actions">
        <button ref={cancel} type="button" className="mw-ghost-btn" disabled={props.busy} onClick={props.onClose}>{t("common.cancel")}</button>
        <button type="button" className="mw-btn mw-collection-removal-confirm mw-managed-delete-confirm"
          disabled={props.busy || props.blocked} onClick={props.onDelete}>
          {props.busy ? t("common.processing") : t("servers.mods.managed.delete", undefined, "Delete Mod")}
        </button>
      </div>
    </footer>
  </dialog>;
}
