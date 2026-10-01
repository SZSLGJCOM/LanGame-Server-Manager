import { useId, useRef } from "react";
import { ShellIcon } from "../../components/ShellIcon";
import { useI18n } from "../../i18n";
import "./manual-mod-install-help.css";

export function ManualModInstallHelp({ note }: { note: string }) {
  const { t } = useI18n();
  const titleId = useId();
  const dialog = useRef<HTMLDialogElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const title = t("servers.mods.installHelp", undefined, "Installation notes");

  return <>
    <button ref={trigger} type="button" className="mw-ghost-btn mw-install-help-button"
      aria-haspopup="dialog" onClick={() => dialog.current?.showModal()}>
      <ShellIcon name="file-text" className="mw-btn-icon" />
      {title}
    </button>
    <dialog ref={dialog} className="mw-install-help-dialog" aria-labelledby={titleId}
      onClose={() => trigger.current?.focus()}>
      <header className="mw-install-help-header">
        <h2 id={titleId}>{title}</h2>
        <button type="button" className="mw-ghost-btn mw-install-help-close"
          aria-label={t("servers.mods.infoDialog.close", undefined, "Close")}
          onClick={() => dialog.current?.close()}>
          <ShellIcon name="x" className="mw-btn-icon" />
        </button>
      </header>
      <div className="mw-install-help-body" tabIndex={0}>{note}</div>
    </dialog>
  </>;
}
