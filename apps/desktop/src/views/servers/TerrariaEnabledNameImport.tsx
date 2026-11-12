import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { ActivityNotice, ActivityNoticeTarget } from "../../components/ActivityNotice";
import { formatDesktopError } from "../../desktop-error-message";
import { useI18n } from "../../i18n";
import { ConfigurationHelp } from "../settings/ConfigurationFieldHelp";
import { isTerrariaModName, readTerrariaEnabledModNames } from "./mod-workbench-terraria";
import "./workshop-collection-removal-dialog.css";

interface Props {
  disabled: boolean;
  onAdd: (names: string[]) => Promise<void>;
}

/** Accept a saved modpack or the internal names of locally installed Mods. */
export function TerrariaEnabledNameImport({ disabled, onAdd }: Props) {
  const { t } = useI18n();
  const id = useId();
  const dialog = useRef<HTMLDialogElement>(null);
  const closeButton = useRef<HTMLButtonElement>(null);
  const savingRef = useRef(false);
  const [open, setOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [closeBlocked, setCloseBlocked] = useState(false);
  const [draft, setDraft] = useState("");
  const [message, setMessage] = useState<{ error: boolean; text: string } | null>(null);
  const pending = Boolean(draft.length || saving);

  useEffect(() => {
    const node = dialog.current;
    if (!node || !open) return;
    const previous = document.activeElement;
    node.showModal();
    closeButton.current?.focus();
    return () => {
      node.close();
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, [open]);

  useEffect(() => {
    if (!open || !pending) return;
    const preserveDraft = (event: KeyboardEvent) => {
      if (event.key === "Escape" && dialog.current?.open) {
        event.preventDefault();
        setCloseBlocked(true);
      }
    };
    document.addEventListener("keydown", preserveDraft, true);
    return () => document.removeEventListener("keydown", preserveDraft, true);
  }, [open, pending]);

  function close() {
    if (pending || savingRef.current) { setCloseBlocked(true); return; }
    setCloseBlocked(false);
    setOpen(false);
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (disabled || savingRef.current || !draft.trim()) return;
    const input = draft.trim();
    let names: string[];
    if (input.startsWith("[")) {
      let parsed: unknown;
      try { parsed = JSON.parse(input); } catch { parsed = null; }
      if (!Array.isArray(parsed) || !parsed.length || !parsed.every((value) =>
        typeof value === "string" && value.trim() && !/[\r\n]/.test(value))) {
        setMessage({ error: true, text: t("servers.mods.terraria.invalidNameList") });
        return;
      }
      names = parsed.map((value: string) => value.trim());
    } else {
      names = readTerrariaEnabledModNames(input);
    }
    if (!names.length || !names.every(isTerrariaModName)) {
      setMessage({ error: true, text: t("servers.mods.terraria.invalidNameList") });
      return;
    }
    savingRef.current = true;
    setSaving(true);
    try {
      await onAdd(names);
      setDraft("");
      setCloseBlocked(false);
      setMessage({ error: false, text: t("servers.mods.terraria.namesSaved") });
    } catch (error) {
      setMessage({ error: true, text: formatDesktopError(t, error) });
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  }

  return <>
    <ConfigurationHelp description={t("servers.mods.terraria.nameListHelp")}>{(help) =>
      <button type="button" className="mw-ghost-btn mw-terraria-import-button" aria-haspopup="dialog"
        ref={help.anchorRef} {...help.interactionProps} aria-describedby={help.descriptionId}
        onClick={() => setOpen(true)}>{t("servers.mods.terraria.importNames")}</button>}
    </ConfigurationHelp>
    <dialog ref={dialog} className="mw-collection-removal-dialog mw-terraria-import-dialog"
      aria-labelledby={`${id}-title`} aria-busy={saving} data-no-window-drag="true"
      onCancel={(event) => { event.preventDefault(); close(); }}>
      <header className="mw-collection-removal-header"><h2 id={`${id}-title`}>{t("servers.mods.terraria.importNames")}</h2></header>
      <form id={`${id}-form`} className="mw-collection-removal-body" onSubmit={submit}>
        <ConfigurationHelp description={t("servers.mods.terraria.nameListHelp")}>{(help) =>
          <textarea className="mw-textarea" rows={5} maxLength={65536} value={draft} disabled={disabled || saving}
            aria-labelledby={`${id}-title`} ref={help.anchorRef} {...help.interactionProps} aria-describedby={help.descriptionId}
            onChange={(event) => { if (!disabled && !savingRef.current) { setDraft(event.target.value); setMessage(null); } }} />}
        </ConfigurationHelp>
        {message ? <ActivityNoticeTarget.Provider value={undefined}>
          <ActivityNotice tone={message.error ? "error" : "success"}>{message.text}</ActivityNotice>
        </ActivityNoticeTarget.Provider> : null}
      </form>
      <footer className="mw-collection-removal-footer">
        {closeBlocked && pending ? <p role="alert" className="mw-collection-removal-error">{t("servers.mods.sourceLists.unsaved")}</p> : null}
        <div className="mw-collection-removal-actions">
          <button className="mw-btn mw-btn--secondary" form={`${id}-form`} type="submit" disabled={disabled || saving || !draft.trim()}>
            {t("servers.mods.terraria.addNames")}
          </button>
          {draft ? <button type="button" className="mw-ghost-btn" disabled={saving} onClick={() => {
            if (savingRef.current) return;
            setDraft(""); setMessage(null); setCloseBlocked(false);
          }}>{t("common.cancel")}</button> : null}
          <button ref={closeButton} type="button" className="mw-ghost-btn mw-terraria-import-close" onClick={close}>{t("common.closeWindow")}</button>
        </div>
      </footer>
    </dialog>
  </>;
}
