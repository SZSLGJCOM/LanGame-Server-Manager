import { useEffect, useId, useMemo, useRef, useState } from "react";
import { ActivityNotice } from "../../components/ActivityNotice";
import { ShellIcon } from "../../components/ShellIcon";
import { describeError } from "../../app-state";
import { useI18n } from "../../i18n";
import { parseAsaModManifest } from "./asa-mod-manifest";

export interface AsaModManifestPanelProps {
  instanceId: string;
  currentIds: string[];
  initialText?: string | null;
  onDraftChange: (text: string) => void;
  disabled: boolean;
  readOnly?: boolean;
  blockedNote?: string;
  /** Use the workbench's stopped-instance and CAS guards to persist active membership. */
  onApply: (ids: string[]) => Promise<boolean>;
}

export function AsaModManifestPanel(props: AsaModManifestPanelProps) {
  const { t, locale } = useI18n();
  const inputId = useId();
  const [text, setText] = useState(() => props.initialText ?? props.currentIds.join("\n"));
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const inFlight = useRef(false);
  const mounted = useRef(true);
  const context = JSON.stringify([props.instanceId, locale]);
  const currentContext = useRef(context);
  currentContext.current = context;
  const revision = useRef(0);
  const parsed = useMemo(() => parseAsaModManifest(text), [text]);
  const activeIds = useMemo(() => new Set(props.currentIds), [props.currentIds]);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; revision.current += 1; };
  }, []);
  useEffect(() => {
    revision.current += 1;
    setText(props.initialText ?? props.currentIds.join("\n"));
    setError(null); setMessage(null);
  }, [props.instanceId]);
  useEffect(() => { setError(null); setMessage(null); }, [locale]);
  function change(next: string) {
    if (props.readOnly || inFlight.current) return;
    revision.current += 1;
    setText(next); props.onDraftChange(next); setError(null); setMessage(null);
  }
  const valid = parsed.ids.length > 0 && !parsed.tooLarge && parsed.invalidTokens.length === 0;
  async function apply() {
    if (!valid || props.disabled || props.readOnly || props.blockedNote || inFlight.current) return;
    inFlight.current = true;
    setApplying(true); setError(null); setMessage(null);
    const requestRevision = revision.current;
    const isCurrent = () => mounted.current && currentContext.current === context && revision.current === requestRevision;
    try {
      const saved = await props.onApply(parsed.ids);
      if (isCurrent()) {
        if (saved) setMessage(t("servers.mods.asaManifest.saved"));
        else setError(t("servers.mods.asaManifest.saveFailed"));
      }
    } catch (cause) {
      if (isCurrent()) setError(describeError(cause));
    } finally {
      inFlight.current = false;
      if (mounted.current) setApplying(false);
    }
  }
  return <section className="mw-manifest" aria-busy={applying}>
    <div className="mw-manifest-input-head">
      <label htmlFor={inputId}>{t("servers.mods.asaManifest.input")}</label>
      <button type="button" className="mw-ghost-btn" disabled={applying || props.readOnly}
        onClick={() => change(props.currentIds.join("\n"))}>{t("servers.mods.manifest.useCurrent")}</button>
    </div>
    <textarea id={inputId} className="mw-textarea mw-manifest-input" value={text} disabled={applying}
      readOnly={props.readOnly} spellCheck={false} maxLength={1_048_577} aria-describedby={`${inputId}-help`}
      placeholder={t("servers.mods.asaManifest.placeholder")} onChange={(event) => change(event.target.value)} />
    <p id={`${inputId}-help`} className="mw-manifest-help">{t("servers.mods.asaManifest.help")}</p>
    <p className="mw-manifest-help" role="status">{t("servers.mods.manifest.parsed", { count: parsed.ids.length, duplicates: parsed.duplicateCount })}</p>
    {parsed.tooLarge ? <ActivityNotice tone="error">{t("servers.mods.manifest.tooLarge")}</ActivityNotice> : null}
    {parsed.invalidTokens.length ? <ActivityNotice tone="error">{t("servers.mods.asaManifest.invalid", {
      values: parsed.invalidTokens.slice(0, 8).map((token) => token.slice(0, 80)).join("、")
    })}</ActivityNotice> : null}
    {props.blockedNote ? <ActivityNotice tone="warning">{props.blockedNote}</ActivityNotice> : null}
    {error ? <ActivityNotice tone="error">{error}</ActivityNotice> : null}
    {message ? <ActivityNotice tone="success">{message}</ActivityNotice> : null}
    {parsed.ids.length ? <div className="mw-manifest-review">
      <table className="mw-manifest-table"><thead><tr><th>{t("servers.mods.asaManifest.input")}</th>
        <th>{t("servers.mods.asaManifest.status")}</th></tr></thead>
        <tbody>{parsed.ids.slice(0, 200).map((id) => <tr key={id}><td>{id}</td>
          <td>{t(activeIds.has(id) ? "servers.mods.asaManifest.current" : "servers.mods.asaManifest.nextStart")}</td></tr>)}</tbody>
      </table>
      {parsed.ids.length > 200 ? <p className="mw-manifest-help">{t("servers.mods.manifest.previewLimit", { total: parsed.ids.length })}</p> : null}
    </div> : null}
    <div className="mw-manifest-actions"><button type="button" className="mw-btn mw-btn--primary"
      disabled={!valid || applying || props.disabled || props.readOnly || Boolean(props.blockedNote)} onClick={() => void apply()}>
      {applying ? <ShellIcon name="loader" className="mw-btn-icon mw-btn-icon--spin" /> : null}
      {t(applying ? "servers.mods.asaManifest.saving" : "servers.mods.asaManifest.apply")}
    </button></div>
  </section>;
}
