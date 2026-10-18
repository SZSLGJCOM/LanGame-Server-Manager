import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ShellIcon } from "../../components/ShellIcon";
import { describeError } from "../../app-state";
import { useI18n } from "../../i18n";
import { useConfigurationFieldHelp } from "../settings/ConfigurationFieldHelp";
import { useInstanceSettingsSaveCoordinator } from "../settings/InstanceSettingsSaveContext";
import { InstanceSettingsDraftInvalidError } from "../settings/instance-settings-save-queue";
import { ManifestHelpTrigger } from "./ManifestHelpTrigger";
import { parseAsaModManifest } from "./asa-mod-manifest";

/** Typed lists wait for a pause before saving, like the other auto-saved instance settings. */
export const ASA_MANIFEST_SAVE_DELAY_MS = 650;

export interface AsaModManifestPanelProps {
  instanceId: string;
  /** The server's active Mod ID list (`mod_ids_csv`), in load order. */
  currentIds: string[];
  initialText?: string | null;
  onDraftChange: (text: string) => void;
  readOnly?: boolean;
  /** Why the list cannot be written right now (running instance, raw launch flags, another Mod change…). */
  blockedReason?: string | null;
  /** Replace the active list with exactly `ids`; rejects when the stored list no longer equals `expected`. */
  onSave: (ids: string[], expected: string[]) => Promise<void>;
}

type SaveState = "saved" | "dirty" | "saving" | "failed" | "blocked" | "conflict";
const listKey = (ids: readonly string[]) => ids.join("\n");

/** The checklist is the server's authoritative active Mod list: what is written here is what ASA loads next start. */
export function AsaModManifestPanel(props: AsaModManifestPanelProps) {
  const { t } = useI18n();
  const inputId = useId();
  const coordinator = useInstanceSettingsSaveCoordinator();
  const [text, setText] = useState(() => props.initialText ?? props.currentIds.join("\n"));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const savedKey = listKey(props.currentIds);
  // Stored lists this draft is based on or has written; any other stored list changed elsewhere.
  const known = useRef(new Set([savedKey]));
  // The text last taken from the stored list; while it is untouched, external changes are followed.
  const synced = useRef(props.currentIds.join("\n"));
  const [, rerender] = useState(0);
  const parsed = useMemo(() => parseAsaModManifest(text), [text]);
  const valid = !parsed.tooLarge && parsed.invalidTokens.length === 0;
  const draftKey = valid ? listKey(parsed.ids) : null;
  const edited = draftKey !== savedKey;
  const conflict = edited && !known.current.has(savedKey);
  const blockedReason = props.readOnly ? null : props.blockedReason ?? null;
  const activeIds = useMemo(() => new Set(props.currentIds), [props.currentIds]);
  const state: SaveState = saving ? "saving" : !edited ? "saved" : !valid ? "blocked" : conflict ? "conflict"
    : error ? "failed" : blockedReason ? "blocked" : "dirty";

  const latest = useRef({ ids: parsed.ids, draftKey, savedIds: props.currentIds, savable: false, onSave: props.onSave,
    edited, valid, conflict, blockedReason, failed: false });
  latest.current = { ids: parsed.ids, draftKey, savedIds: props.currentIds, savable: state === "dirty", onSave: props.onSave,
    edited, valid, conflict, blockedReason, failed: Boolean(error) };
  const inFlight = useRef<Promise<void> | null>(null);
  const mounted = useRef(true);
  // The list just written stays the CAS baseline until the refreshed record arrives.
  const written = useRef<{ ids: string[]; over: string } | null>(null);
  if (written.current && written.current.over !== savedKey) written.current = null;
  const expected = () => written.current?.ids ?? latest.current.savedIds;

  /** One write of the current draft; failures are shown here and rethrown to a waiting start. */
  function write(request: typeof latest.current): Promise<void> {
    if (request.draftKey !== null) known.current.add(request.draftKey);
    setSaving(true); setError(null);
    const run = (async () => {
      try {
        await request.onSave(request.ids, expected());
        written.current = { ids: request.ids, over: listKey(latest.current.savedIds) };
      } catch (cause) {
        if (mounted.current) setError(describeError(cause));
        throw cause;
      } finally {
        inFlight.current = null;
        if (mounted.current) setSaving(false);
      }
    })();
    inFlight.current = run;
    return run;
  }
  async function save() {
    const request = latest.current;
    if (inFlight.current || !request.savable || request.draftKey === null) return;
    await write(request).catch(() => undefined);
  }

  /** Start barrier: wait for the pending list to save; an unsavable draft blocks the start with its reason. */
  async function flushForStart(): Promise<void> {
    for (let attempt = 0; attempt < 50; attempt += 1) {
      if (inFlight.current) { await inFlight.current; continue; }
      const request = latest.current;
      if (!request.edited || (written.current && listKey(written.current.ids) === request.draftKey)) return;
      if (!request.valid) throw new InstanceSettingsDraftInvalidError();
      if (request.conflict) throw new Error(t("servers.mods.asaManifest.conflict"));
      if (request.blockedReason) {
        // Another Mod change may still be finishing; persistent blocks (raw launch flags) stop the start.
        await new Promise((resolve) => window.setTimeout(resolve, 50));
        if (attempt < 40) continue;
        throw new Error(t("servers.mods.asaManifest.blocked", { reason: request.blockedReason }));
      }
      await write(request);
    }
  }
  const flushRef = useRef(flushForStart);
  flushRef.current = flushForStart;

  // Debounced auto-save; a newer draft written during a save is saved after it (latest edit wins).
  useEffect(() => {
    if (state !== "dirty") return;
    const timer = window.setTimeout(() => void save(), ASA_MANIFEST_SAVE_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [state, draftKey]);

  // A stored list that caught up with the draft ends the edit; untouched text follows stored changes.
  useEffect(() => {
    if (!edited) { known.current = new Set([savedKey]); synced.current = text; setError(null); return; }
    if (text === synced.current && !inFlight.current) {
      const current = props.currentIds.join("\n");
      synced.current = current; known.current = new Set([savedKey]); setText(current);
    }
  }, [savedKey, edited]);

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);

  // Same start barrier as the Maintenance editors: starting this instance waits for (or is refused by) this draft.
  useLayoutEffect(() => {
    const registration = coordinator.register(props.instanceId, () => flushRef.current(), {});
    return () => {
      // Leaving the checklist (or switching instances) inside the typing pause still writes the pending list,
      // and that write, or the reason it cannot happen, stays part of the instance's next start.
      const pending = latest.current;
      const alreadyWritten = written.current !== null && listKey(written.current.ids) === pending.draftKey;
      const completion = inFlight.current ? inFlight.current
        : !pending.edited || alreadyWritten ? Promise.resolve()
          : !pending.valid || pending.conflict ? Promise.reject(new InstanceSettingsDraftInvalidError())
            : pending.blockedReason ? Promise.resolve()
              : pending.onSave(pending.ids, expected());
      registration.detach(completion);
    };
  }, [coordinator, props.instanceId]);

  function change(next: string) {
    if (props.readOnly) return;
    setText(next); props.onDraftChange(next); setError(null);
  }
  function discard() {
    const current = props.currentIds.join("\n");
    synced.current = current; known.current = new Set([savedKey]);
    setText(current); props.onDraftChange(current); setError(null);
  }
  function keepMine() {
    known.current.add(savedKey); setError(null); rerender((value) => value + 1);
  }

  const help = useConfigurationFieldHelp(`${inputId}-help`, t("servers.mods.asaManifest.help"), undefined, undefined, "instructions");
  const previewHeadingId = `${inputId}-preview`;
  const newCount = parsed.ids.filter((id) => !activeIds.has(id)).length;
  const droppedCount = valid ? props.currentIds.filter((id) => !parsed.ids.includes(id)).length : 0;
  const statusText = state === "saving" ? t("servers.mods.asaManifest.saving")
    : state === "saved" ? t("servers.mods.asaManifest.saved")
      : state === "conflict" ? t("servers.mods.asaManifest.conflict")
        : state === "failed" ? t("servers.mods.asaManifest.saveFailed", { message: error ?? "" })
          : state === "blocked" ? (!valid ? t("servers.mods.asaManifest.fixInvalid") : t("servers.mods.asaManifest.blocked", { reason: blockedReason ?? "" }))
            : t("servers.mods.asaManifest.pending");
  const tone = state === "saved" ? "success" : state === "failed" || state === "conflict" ? "error" : state === "blocked" ? "warning" : "info";
  return <section className="mw-manifest" aria-busy={saving}>
    <div className="mw-manifest-pane mw-manifest-pane--input">
      <div className="mw-manifest-pane-head mw-manifest-input-head">
        <span className="mw-manifest-pane-title">
          <label htmlFor={inputId}>{t("servers.mods.asaManifest.input")}</label>
          <ManifestHelpTrigger binding={help} label={t("servers.mods.manifest.helpLabel")} />
        </span>
      </div>
      <div className="mw-manifest-pane-body">
        <textarea id={inputId} className="mw-textarea mw-manifest-input" value={text}
          readOnly={props.readOnly} spellCheck={false} maxLength={1_048_577} aria-describedby={help.descriptionId}
          aria-invalid={parsed.tooLarge || parsed.invalidTokens.length > 0 || undefined}
          placeholder={t("servers.mods.asaManifest.placeholder")} onChange={(event) => change(event.target.value)} />
        {parsed.tooLarge ? <div className="mw-notice mw-notice--error" role="alert">{t("servers.mods.manifest.tooLarge")}</div> : null}
        {parsed.invalidTokens.length ? <div className="mw-notice mw-notice--error" role="alert">{t("servers.mods.asaManifest.invalid", {
          values: parsed.invalidTokens.slice(0, 8).map((token) => token.slice(0, 80)).join("、")
        })}</div> : null}
      </div>
      <div className="mw-manifest-toolbar">
        <span className="mw-manifest-meta" role="status">{t("servers.mods.manifest.parsed", { count: parsed.ids.length, duplicates: parsed.duplicateCount })}</span>
      </div>
    </div>
    <div className="mw-manifest-pane mw-manifest-pane--review" role="region" aria-labelledby={previewHeadingId}>
      <div className="mw-manifest-pane-head">
        <span id={previewHeadingId} className="mw-manifest-pane-title">{t("servers.mods.asaManifest.previewTitle")}</span>
        {valid && edited ? <span className="mw-manifest-summary">{t("servers.mods.asaManifest.changeSummary", { added: newCount, removed: droppedCount })}</span> : null}
      </div>
      <div className="mw-manifest-pane-body">
        {parsed.ids.length ? <div className="mw-manifest-review">
          <table className="mw-manifest-table"><thead><tr><th scope="col">#</th><th scope="col">{t("servers.mods.asaManifest.input")}</th>
            <th scope="col">{t("servers.mods.asaManifest.status")}</th></tr></thead>
            <tbody>{parsed.ids.slice(0, 200).map((id, index) => <tr key={id}><td>{index + 1}</td><td>{id}</td>
              <td><span className={activeIds.has(id) ? "mw-chip" : "mw-chip mw-chip--installation-checking"}>
                {t(activeIds.has(id) ? "servers.mods.asaManifest.current" : "servers.mods.asaManifest.added")}
              </span></td></tr>)}</tbody>
          </table>
          {parsed.ids.length > 200 ? <p className="mw-manifest-note">{t("servers.mods.manifest.previewLimit", { total: parsed.ids.length })}</p> : null}
        </div> : <div className="mw-manifest-empty" role="status">
          <ShellIcon name="list" className="mw-btn-icon" /><span>{t("servers.mods.asaManifest.previewEmpty")}</span>
        </div>}
      </div>
      {props.readOnly ? null : <div className="mw-manifest-actions" data-save-state={state}>
        {/* Inline status: the save state belongs to this list, not to the global activity feed. */}
        <p className={tone === "error" || tone === "warning" ? "mw-manifest-note mw-manifest-note--error" : "mw-manifest-note"}
          role={tone === "error" ? "alert" : "status"}>
          {state === "saving" ? <ShellIcon name="loader" className="mw-btn-icon mw-btn-icon--spin" /> : null}{statusText}</p>
        <div className="mw-manifest-action-buttons">
          {state === "failed" ? <button type="button" className="mw-btn mw-btn--secondary" onClick={() => { setError(null); }}>
            {t("settings.configuration.save.retry", undefined, "Retry save")}</button> : null}
          {state === "conflict" ? <button type="button" className="mw-btn mw-btn--secondary" onClick={keepMine}>
            {t("servers.mods.asaManifest.keepMine")}</button> : null}
          {edited && (state === "failed" || state === "conflict" || state === "blocked") ? <button type="button" className="mw-ghost-btn" onClick={discard}>
            {t("servers.mods.asaManifest.discard")}</button> : null}
        </div>
      </div>}
    </div>
  </section>;
}
