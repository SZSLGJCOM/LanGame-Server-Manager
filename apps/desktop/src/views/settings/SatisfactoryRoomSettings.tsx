import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { authorizeSatisfactoryServer, loadSatisfactorySave, readSatisfactoryAdminPassword,
  setupSatisfactoryServer, writeSatisfactoryRoom, type SatisfactoryWorldSnapshot } from "../../satisfactory-world-settings";
import type { ConfigurationSpecializedRendererProps } from "./module-types";
import { SATISFACTORY_NATIVE_COPY as COPY, SatisfactoryNativeField as Field, SatisfactoryNativePanel } from "./SatisfactoryNativeFields";
import { useSatisfactoryWorldSettings } from "./SatisfactoryWorldSettingsContext";
import { useSatisfactoryNativeAutosave } from "./useSatisfactoryNativeAutosave";

interface RoomDraft { baseline: string; values: { server_name?: string; client_password?: string; auto_load_session_name?: string } }
function roomBaseline(snapshot: SatisfactoryWorldSnapshot) {
  return JSON.stringify([snapshot.server_name, snapshot.auto_load_session_name, snapshot.active_session_name, snapshot.is_game_running]);
}

export function SatisfactoryRoomSettings(props: ConfigurationSpecializedRendererProps) {
  const { t } = useI18n();
  const state = useSatisfactoryWorldSettings();
  const snapshot = state.snapshot;
  const [name, setName] = useState(props.details.summary.name);
  const [password, setPassword] = useState("");
  const [changePassword, setChangePassword] = useState(false);
  const [save, setSave] = useState("");
  const [confirmLoad, setConfirmLoad] = useState(false);
  const [draft, setDraft] = useState<RoomDraft | null>(null);
  const draftRef = useRef<RoomDraft | null>(null);
  const updateDraft = (next: RoomDraft | null) => { draftRef.current = next; setDraft(next); };
  const dirty = draft !== null;
  useEffect(() => { setPassword(""); setChangePassword(false); updateDraft(null); setSave(""); setConfirmLoad(false); }, [props.details.summary.id]);
  const disabled = props.disabled || state.loading || state.pending || snapshot?.connection_status !== "ready";
  const changed = (values: RoomDraft["values"]) => {
    if (!snapshot || disabled) return;
    const current = draftRef.current;
    updateDraft({ baseline: current?.baseline ?? roomBaseline(snapshot), values: { ...current?.values, ...values } });
  };
  const ready = snapshot?.connection_status === "ready";
  const hasWorldChanged = Boolean(draft && snapshot && draft.baseline !== roomBaseline(snapshot));
  const canSave = (current: RoomDraft, native = state.getSnapshot()) => Boolean(native && !props.disabled && !state.loading && !state.pending &&
    native.connection_status === "ready" && current.baseline === roomBaseline(native) &&
    (current.values.server_name === undefined || current.values.server_name.trim()) && current.values.client_password !== "");
  const autosave = useSatisfactoryNativeAutosave({ instanceId: props.details.summary.id,
    signature: JSON.stringify([draft, snapshot?.revision, disabled]), getDraft: () => draftRef.current, canSave,
    save: (current) => state.mutate((native) => {
      if (!native || !canSave(current, native)) throw new Error(t(`${COPY}.changedSinceEdit`));
      return writeSatisfactoryRoom({ instance_id: props.details.summary.id, expected_revision: native.revision,
        server_name: current.values.server_name ?? null, client_password: current.values.client_password ?? null,
        auto_load_session_name: current.values.auto_load_session_name ?? null });
    }), onSaved: (sent) => {
      const current = draftRef.current, native = state.getSnapshot();
      if (!current || !native) return;
      const values = Object.fromEntries(Object.entries(current.values).filter(([key, value]) => value !== sent.values[key as keyof RoomDraft["values"]]));
      updateDraft(Object.keys(values).length ? { baseline: roomBaseline(native), values } : null);
      if (sent.values.client_password !== undefined && !Object.prototype.hasOwnProperty.call(values, "client_password")) { setChangePassword(false); setPassword(""); }
    }
  });
  return <SatisfactoryNativePanel sectionId="room" dirty={dirty}>
    {snapshot?.connection_status === "unclaimed" ? <>
      <p className="form-note">{t(`${COPY}.setupHelp`)}</p>
      <div className="settings-schema-grid configuration-field-grid">
        <Field fieldKey="satisfactory_server_name" sectionId="room" title={t(`${COPY}.serverName`)} value={name}
          disabled={props.disabled || state.loading || state.busy} onChange={(value) => setName(String(value))} />
      </div>
      <button type="button" className="primary-button" disabled={props.disabled || state.loading || state.busy || !name.trim()}
        onClick={() => void state.mutate(() => setupSatisfactoryServer({ instance_id: props.details.summary.id,
          server_name: name.trim(), admin_password: null }))}>{t(`${COPY}.setup`)}</button>
    </> : null}
    {ready ? <>
      <div className="settings-schema-grid configuration-field-grid">
        <Field fieldKey="satisfactory_server_name" sectionId="room" title={t(`${COPY}.serverName`)} value={draft?.values.server_name ?? snapshot.server_name ?? props.details.summary.name}
          disabled={disabled} onChange={(value) => changed({ server_name: String(value) })} />
        <Field fieldKey="satisfactory_auto_load_session" sectionId="room" title={t(`${COPY}.autoLoadSession`)}
          description={t(`${COPY}.autoLoadSessionHelp`)} kind="select" value={draft?.values.auto_load_session_name ?? snapshot.auto_load_session_name} disabled={disabled}
          options={[{ value: "", label: t(`${COPY}.noAutoLoadSession`) },
            ...snapshot.auto_load_session_name && !snapshot.sessions.some((item) => item.session_name === snapshot.auto_load_session_name)
              ? [{ value: snapshot.auto_load_session_name, label: snapshot.auto_load_session_name }] : [],
            ...snapshot.sessions.map((item) => ({ value: item.session_name, label: item.session_name }))]}
          onChange={(value) => changed({ auto_load_session_name: String(value) })} />
        <Field fieldKey="satisfactory_change_join_password" sectionId="room" title={t(`${COPY}.changeJoinPassword`)}
          kind="boolean" value={changePassword} disabled={disabled}
          onChange={(value) => {
            setChangePassword(Boolean(value));
            if (!value) {
              setPassword("");
              const current = draftRef.current;
              if (current) { const { client_password: _password, ...values } = current.values; updateDraft(Object.keys(values).length ? { ...current, values } : null); }
            }
          }} />
        <Field fieldKey="satisfactory_join_password" sectionId="room" title={t(`${COPY}.joinPassword`)} kind="secret"
          description={t(`${COPY}.joinPasswordHelp`)} value={password} disabled={disabled || !changePassword}
          onChange={(value) => { setPassword(String(value)); changed({ client_password: String(value) }); }} />
      </div>
      <button type="button" className="ghost-button" disabled={disabled || state.busy || dirty || changePassword}
        onClick={() => void state.mutate((native) => {
          if (!native) throw new Error(t(`${COPY}.fieldUnavailable`));
          return writeSatisfactoryRoom({ instance_id: props.details.summary.id, expected_revision: native.revision,
            server_name: null, client_password: "", auto_load_session_name: null });
        })}>{t(`${COPY}.clearJoinPassword`)}</button>
      {hasWorldChanged ? <div className="configuration-workspace__notice" role="status">
        <span>{t(`${COPY}.changedSinceEdit`)}</span>
        <button type="button" className="secondary-button" disabled={state.busy}
          onClick={() => { updateDraft(null); setChangePassword(false); setPassword(""); }}>{t(`${COPY}.discardDraft`)}</button>
      </div> : null}
      {autosave.saving ? <p className="form-note" role="status">{t(`${COPY}.saving`)}</p> : null}
      {autosave.failed ? <div className="panel-head panel-head--compact panel-head--spread">
        <span className="form-note form-note--error" role="status">{t(`${COPY}.autosaveFailed`)}</span>
        <button type="button" className="secondary-button" disabled={!draft || !canSave(draft)} onClick={autosave.retry}>{t(`${COPY}.retryAutosave`)}</button>
      </div> : null}
      <section className="guided-field-group">
        <div className="guided-field-group-head"><h5 className="guided-field-group-title">{t(`${COPY}.loadSave`)}</h5></div>
        <p className="form-note">{t(`${COPY}.currentSession`, { name: snapshot.active_session_name || t(`${COPY}.noWorld`) })}</p>
        {snapshot.sessions.every((item) => item.saves.length === 0) ? <p className="form-note">{t(`${COPY}.noSaves`)}</p> :
          <div className="settings-schema-grid configuration-field-grid">
            <Field fieldKey="satisfactory_load_save" sectionId="room" title={t(`${COPY}.saveSelection`)} kind="select"
              value={save} disabled={disabled || state.busy || dirty || snapshot.connected_players > 0}
              options={[{ value: "", label: t(`${COPY}.chooseSave`) }, ...snapshot.sessions.flatMap((item) =>
                item.saves.map((entry) => ({ value: entry.save_name, label: `${item.session_name} · ${
                  entry.save_name.startsWith("LGSM_before_world_change_") ? t(`${COPY}.beforeChangeSave`, { time: entry.save_date_time }) :
                    entry.save_name.startsWith("LGSM_world_settings_") ? t(`${COPY}.rulesSave`, { time: entry.save_date_time }) : entry.save_name
                }` })))]}
              onChange={(value) => { setSave(String(value)); setConfirmLoad(false); }} />
            <Field fieldKey="satisfactory_confirm_load" sectionId="room" title={t(`${COPY}.confirmLoad`)} kind="boolean"
              value={confirmLoad} disabled={disabled || state.busy || !save || dirty || snapshot.connected_players > 0}
              onChange={(value) => setConfirmLoad(Boolean(value))} />
          </div>}
        {snapshot.connected_players > 0 ? <p className="form-note" role="status">{t(`${COPY}.playersPresent`)}</p> : null}
        <button type="button" className="secondary-button" disabled={disabled || state.busy || dirty || !save || !confirmLoad || snapshot.connected_players > 0}
          onClick={() => void state.mutate(() => loadSatisfactorySave({ instance_id: props.details.summary.id,
            expected_revision: snapshot.revision, save_name: save })).then((accepted) => { if (accepted) setConfirmLoad(false); })}>
          {t(`${COPY}.loadSelectedSave`)}
        </button>
        {state.result === "accepted" || state.result === "loaded" ? <p className="form-note" role="status">
          {t(`${COPY}.${state.result === "loaded" ? "loaded" : "accepted"}`)}</p> : null}
      </section>
    </> : null}
  </SatisfactoryNativePanel>;
}

export function SatisfactoryAuthorizationSettings(props: ConfigurationSpecializedRendererProps) {
  const { t } = useI18n();
  const state = useSatisfactoryWorldSettings();
  const [password, setPassword] = useState("");
  const [revealed, setRevealed] = useState<string | null>(null);
  const [credentialError, setCredentialError] = useState<string | null>(null);
  const [readingPassword, setReadingPassword] = useState(false);
  const credentialGeneration = useRef(0);
  useEffect(() => {
    credentialGeneration.current += 1;
    setPassword(""); setRevealed(null); setCredentialError(null); setReadingPassword(false);
    return () => { credentialGeneration.current += 1; };
  }, [props.active, props.details.summary.id]);
  const disabled = props.disabled || state.loading || state.busy || state.pending;
  async function showPassword() {
    const current = ++credentialGeneration.current;
    setReadingPassword(true); setCredentialError(null);
    try {
      const value = await readSatisfactoryAdminPassword(props.details.summary.id);
      if (current !== credentialGeneration.current) return;
      setRevealed(value);
      if (value === null) setCredentialError(t(`${COPY}.passwordNotStored`));
    } catch (error) { if (current === credentialGeneration.current) setCredentialError(error instanceof Error ? error.message : String(error)); }
    finally { if (current === credentialGeneration.current) setReadingPassword(false); }
  }
  return <SatisfactoryNativePanel sectionId="access">
    {state.snapshot?.connection_status === "authorization_required" ? <>
      <p className="form-note">{t(`${COPY}.authorizeHelp`)}</p>
      <div className="settings-schema-grid configuration-field-grid">
        <Field fieldKey="satisfactory_admin_authorization" sectionId="access" kind="secret" value={password}
          title={t(`${COPY}.adminPassword`)} disabled={disabled} onChange={(value) => setPassword(String(value))} />
      </div>
      <button type="button" className="primary-button" disabled={disabled || (!password && props.settings.allow_insecure_local_api !== true)}
        onClick={() => void state.mutate(() => authorizeSatisfactoryServer({ instance_id: props.details.summary.id,
          admin_password: password || null })).then((accepted) => { if (accepted) setPassword(""); })}>{t(`${COPY}.authorize`)}</button>
    </> : null}
    {state.snapshot?.connection_status === "ready" ? <>
      <p className="form-note" role="status">{t(`${COPY}.authorized`)}</p>
      {revealed !== null ? <div className="settings-schema-grid configuration-field-grid">
        <Field fieldKey="satisfactory_admin_password" sectionId="access" title={t(`${COPY}.adminPassword`)} kind="secret"
          value={revealed} readOnly onChange={() => {}} />
      </div> : null}
      <button type="button" className="secondary-button" disabled={disabled || readingPassword}
        onClick={() => { if (revealed !== null) setRevealed(null); else void showPassword(); }}>
        {revealed !== null ? t(`${COPY}.hideAdminPassword`) : t(`${COPY}.showAdminPassword`)}
      </button>
      {credentialError ? <p className="form-note form-note--error" role="alert">{credentialError}</p> : null}
    </> : null}
  </SatisfactoryNativePanel>;
}
