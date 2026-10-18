import { useEffect, useId, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { openExternalUrl } from "../../api";
import { clearNexusApiKey, readNexusAccountStatus, storeNexusApiKey, type NexusAccountStatus } from "../../nexus-mods-api";
import { NexusDialog } from "./NexusDialog";
import { nexusErrorText, nexusText } from "./nexus-mods-messages";

export function NexusCredentialsDialog(props: { disabled: boolean; onClose(): void; onStatus(status: NexusAccountStatus): void }) {
  const { locale } = useI18n(), text = (key: Parameters<typeof nexusText>[1]) => nexusText(locale, key), id = useId();
  const [status, setStatus] = useState<NexusAccountStatus | null>(null), [secret, setSecret] = useState("");
  const [busy, setBusy] = useState(false), [error, setError] = useState<string | null>(null), [note, setNote] = useState<string | null>(null);
  const mounted = useRef(true), active = useRef(false);
  useEffect(() => { mounted.current = true; void reload(); return () => { mounted.current = false; }; }, []);
  async function reload() {
    if (active.current) return;
    active.current = true; setBusy(true); setError(null);
    try { const fresh = await readNexusAccountStatus(); if (mounted.current) { setStatus(fresh); props.onStatus(fresh); } }
    catch (failure) { if (mounted.current) setError(nexusErrorText(locale, failure)); }
    finally { active.current = false; if (mounted.current) setBusy(false); }
  }
  async function mutate(operation: "save" | "clear") {
    if (active.current || props.disabled || (operation === "save" && !secret.trim())) return;
    active.current = true; setBusy(true); setError(null); setNote(null);
    try {
      const fresh = operation === "save" ? await storeNexusApiKey(secret) : await clearNexusApiKey();
      if (mounted.current) { setStatus(fresh); props.onStatus(fresh); setSecret(""); setNote(text(operation === "save" ? "saved" : "cleared")); }
    } catch (failure) { if (mounted.current) setError(nexusErrorText(locale, failure)); }
    finally { active.current = false; if (mounted.current) setBusy(false); }
  }
  return <NexusDialog title={text("authTitle")} closeLabel={text("close")} busy={busy} onClose={props.onClose} className="nexus-credentials">
    <div className="nexus-dialog__body" data-nexus-credentials>
      <p>{text("authHelp")}</p>
      <p role="status">{status ? status.configured ? `${text("configured")}${status.user_name ? ` · ${status.user_name}` : ""}` : text("missing") : text(busy ? "reading" : "statusUnknown")}</p>
      {status?.configured ? <p className="nexus-dialog__muted">{text(status.is_premium ? "premium" : "standard")}</p> : null}
      <label className="nexus-credentials__field" htmlFor={id}>{text("key")}
        <input id={id} className="mw-reference-input" type="password" autoComplete="new-password" spellCheck={false}
          data-nexus-api-key value={secret} disabled={busy || props.disabled} maxLength={4096} aria-describedby={`${id}-help`}
          onChange={(event) => setSecret(event.target.value)} />
      </label>
      <p id={`${id}-help`} className="nexus-dialog__muted">{text("keyHelp")}</p>
      <a href="https://www.nexusmods.com/users/myaccount?tab=api" onClick={(event) => {
        event.preventDefault(); void openExternalUrl("https://www.nexusmods.com/users/myaccount?tab=api").catch(() => {
          if (mounted.current) setError(text("openFailed"));
        });
      }}>{text("application")}</a>
      {error ? <p role="alert" className="nexus-dialog__error">{error} <button type="button" className="mw-ghost-btn" disabled={busy} onClick={() => void reload()}>{text("retry")}</button></p> : null}
      {note ? <p role="status">{note}</p> : null}
    </div>
    <footer className="nexus-dialog__footer">
      <button type="button" className="mw-ghost-btn" data-nexus-key-close disabled={busy} onClick={props.onClose}>{text("close")}</button>
      <button type="button" className="mw-btn mw-btn--secondary" data-nexus-key-clear disabled={busy || props.disabled || status?.configured === false} onClick={() => void mutate("clear")}>{text("clear")}</button>
      <button type="button" className="mw-btn mw-btn--primary" data-nexus-key-save disabled={busy || props.disabled || !secret.trim()} onClick={() => void mutate("save")}>{text(busy ? "busy" : "save")}</button>
    </footer>
  </NexusDialog>;
}
