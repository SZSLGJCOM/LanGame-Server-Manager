import { ActivityNotice } from "./ActivityNotice";
import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { describeError } from "../app-state";
import { useI18n } from "../i18n";
import "./backup-rename-action.css";

interface BackupRenameActionProps {
  name: string;
  disabled?: boolean;
  onRename?: (displayName: string) => Promise<boolean>;
}

export function BackupRenameAction({ name, onRename, disabled }: BackupRenameActionProps) {
  const { t } = useI18n();
  const helpId = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const inFlight = useRef(false);
  const restoreFocus = useRef(false);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(name);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (editing) {
      inputRef.current?.focus();
      inputRef.current?.select();
    } else if (restoreFocus.current) {
      restoreFocus.current = false;
      buttonRef.current?.focus();
    }
  }, [editing]);

  function close() {
    if (inFlight.current) return;
    restoreFocus.current = true;
    setEditing(false);
  }

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (inFlight.current || disabled || !onRename) return;
    inFlight.current = true;
    setSaving(true);
    setError(null);
    try {
      if (await onRename(draft)) {
        restoreFocus.current = true;
        setEditing(false);
      }
    } catch (cause) {
      setError(describeError(cause));
    } finally {
      inFlight.current = false;
      setSaving(false);
    }
  }

  if (!editing) {
    return <button ref={buttonRef} type="button" className="secondary-button" disabled={disabled || !onRename} onClick={() => {
      if (disabled || !onRename) return;
      setDraft(name);
      setError(null);
      setEditing(true);
    }}>{t("servers.backups.rename")}</button>;
  }

  return <form className="backup-rename-action" aria-label={t("servers.backups.rename")}
    aria-busy={saving} onSubmit={(event) => void save(event)} onKeyDown={(event) => {
      if (event.key === "Enter" && event.nativeEvent.isComposing) {
        event.preventDefault();
        return;
      }
      if (event.key === "Escape" && !event.nativeEvent.isComposing) {
        event.preventDefault();
        event.stopPropagation();
        close();
      }
    }}>
    <input ref={inputRef} value={draft} disabled={saving}
      aria-label={t("servers.backups.rename")} aria-describedby={helpId}
      onChange={(event) => setDraft(event.target.value)} />
    <button type="submit" className="secondary-button" disabled={saving}>
      {t(saving ? "common.processing" : "common.save")}
    </button>
    <button type="button" className="ghost-button" disabled={saving} onClick={close}>{t("common.cancel")}</button>
    <span id={helpId} className="backup-rename-action-help">{t("servers.backups.renamePrompt", { name })}</span>
    {error ? <ActivityNotice tone="error" onDismiss={() => setError(null)}>{error}</ActivityNotice> : null}
  </form>;
}
