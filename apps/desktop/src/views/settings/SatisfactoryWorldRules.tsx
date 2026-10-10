import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { buildSatisfactoryRulesPatch, createSatisfactoryWorld, isSatisfactoryRuleValueValid,
  SATISFACTORY_RULES, SATISFACTORY_STARTING_LOCATIONS, splitSatisfactoryCreationRules,
  writeSatisfactoryWorldRules, type SatisfactoryWorldSnapshot } from "../../satisfactory-world-settings";
import type { ConfigurationSpecializedRendererProps } from "./module-types";
import { SATISFACTORY_NATIVE_COPY as COPY, SatisfactoryNativeField as Field, SatisfactoryNativePanel, SatisfactoryRuleField } from "./SatisfactoryNativeFields";
import { useSatisfactoryWorldSettings } from "./SatisfactoryWorldSettingsContext";
import { useSatisfactoryNativeAutosave } from "./useSatisfactoryNativeAutosave";

interface WorldDraft { baseline: string; values: Record<string, string> }
function worldBaseline(snapshot: SatisfactoryWorldSnapshot) {
  return JSON.stringify([snapshot.active_session_name, snapshot.is_game_running, snapshot.creative_mode_enabled,
    Object.entries(snapshot.advanced_game_settings).sort(([left], [right]) => left.localeCompare(right))]);
}

export function SatisfactoryWorldRules(props: ConfigurationSpecializedRendererProps) {
  const { t } = useI18n();
  const state = useSatisfactoryWorldSettings();
  const snapshot = state.snapshot;
  const [draft, setDraft] = useState<WorldDraft | null>(null);
  const draftRef = useRef<WorldDraft | null>(null);
  const updateDraft = (next: WorldDraft | null) => { draftRef.current = next; setDraft(next); };
  const [consent, setConsent] = useState(false);
  useEffect(() => { updateDraft(null); setConsent(false); }, [props.details.summary.id]);
  const unavailable = props.disabled || state.pending || state.loading || snapshot?.connection_status !== "ready" || !snapshot.is_game_running;
  const stale = Boolean(draft && snapshot && draft.baseline !== worldBaseline(snapshot));
  const rules = SATISFACTORY_RULES.filter((rule) => rule.scope !== "creation");
  const valueOf = (key: string, fallback: string) => draft?.values[key] ?? snapshot?.advanced_game_settings[key] ?? fallback;
  const invalid = rules.some((rule) => !isSatisfactoryRuleValueValid(rule, valueOf(rule.key, rule.default_value)));
  const dirty = draft !== null;
  function edit(key: string, value: string) {
    if (!snapshot || unavailable) return;
    const current = draftRef.current;
    updateDraft({ baseline: current?.baseline ?? worldBaseline(snapshot), values: { ...current?.values, [key]: value } });
  }
  const canSave = (current: WorldDraft, native = state.getSnapshot()) => Boolean(native && !props.disabled && !state.pending && !state.loading &&
    native.connection_status === "ready" && native.is_game_running && current.baseline === worldBaseline(native) &&
    (native.creative_mode_enabled || consent) && rules.every((rule) => isSatisfactoryRuleValueValid(rule,
      current.values[rule.key] ?? native.advanced_game_settings[rule.key] ?? rule.default_value)));
  const autosave = useSatisfactoryNativeAutosave({ instanceId: props.details.summary.id,
    signature: JSON.stringify([draft, consent, snapshot?.revision, unavailable]), getDraft: () => draftRef.current, canSave,
    save: (current) => state.mutate((native) => {
      if (!native || !canSave(current, native)) throw new Error(t(`${COPY}.changedSinceEdit`));
      return writeSatisfactoryWorldRules({ instance_id: props.details.summary.id, expected_revision: native.revision,
        acknowledge_enable_advanced_settings: consent, advanced_game_settings: buildSatisfactoryRulesPatch(native, current.values) });
    }),
    onSaved: (sent) => {
      const current = draftRef.current, native = state.getSnapshot();
      if (!current || !native) return;
      const values = Object.fromEntries(Object.entries(current.values).filter(([key, value]) => value !== sent.values[key]));
      updateDraft(Object.keys(values).length ? { baseline: worldBaseline(native), values } : null);
      setConsent(false);
    }
  });
  useEffect(() => {
    if (draft && snapshot && !stale && !invalid && !Object.keys(buildSatisfactoryRulesPatch(snapshot, draft.values)).length) updateDraft(null);
  }, [draft, snapshot, stale, invalid]);
  return <SatisfactoryNativePanel sectionId="creative_rules" dirty={dirty}>
    <p className="form-note">{t(`${COPY}.creativeHelp`)}</p>
    {snapshot?.connection_status === "ready" && !snapshot.is_game_running ?
      <p className="form-note" role="status">{t(`${COPY}.worldRequired`)}</p> : null}
    <div className="settings-schema-grid configuration-field-grid">
      <Field fieldKey="satisfactory_creative_consent" sectionId="creative_rules" kind="boolean"
        title={t(`${COPY}.${snapshot?.creative_mode_enabled ? "creativeEnabled" : "creativeConsent"}`)}
        description={t(`${COPY}.creativePermanent`)} value={snapshot?.creative_mode_enabled || consent}
        disabled={unavailable || snapshot?.creative_mode_enabled} onChange={(value) => setConsent(Boolean(value))} />
    </div>
    <div className="guided-field-groups">
      {(["world", "player_defaults"] as const).map((scope) => <section key={scope} className="guided-field-group">
        <div className="guided-field-group-head"><h5 className="guided-field-group-title">{t(`${COPY}.groups.${scope}`)}</h5></div>
        {scope === "player_defaults" ? <p className="form-note">{t(`${COPY}.playerDefaultsHelp`)}</p> : null}
        <div className="settings-schema-grid configuration-field-grid">
          {rules.filter((rule) => rule.scope === scope).map((rule) => <SatisfactoryRuleField key={rule.key}
            rule={rule} value={valueOf(rule.key, rule.default_value)} sectionId="creative_rules" disabled={unavailable}
            validation={!isSatisfactoryRuleValueValid(rule, valueOf(rule.key, rule.default_value)) ? t(`${COPY}.invalidRule`) : undefined}
            onChange={(value) => edit(rule.key, value)} />)}
        </div>
      </section>)}
    </div>
    {stale ? <div className="configuration-workspace__notice" role="status">
      <span>{t(`${COPY}.changedSinceEdit`)}</span>
      <button type="button" className="secondary-button" disabled={state.busy}
        onClick={() => { updateDraft(null); setConsent(false); }}>{t(`${COPY}.discardDraft`)}</button>
    </div> : null}
    {autosave.saving ? <p className="form-note" role="status">{t(`${COPY}.saving`)}</p> : null}
    {autosave.failed ? <div className="panel-head panel-head--compact panel-head--spread">
      <span className="form-note form-note--error" role="status">{t(`${COPY}.autosaveFailed`)}</span>
      <button type="button" className="secondary-button" disabled={!draft || !canSave(draft)} onClick={autosave.retry}>{t(`${COPY}.retryAutosave`)}</button>
    </div> : null}
  </SatisfactoryNativePanel>;
}

export function SatisfactoryWorldCreation(props: ConfigurationSpecializedRendererProps) {
  const { t } = useI18n();
  const state = useSatisfactoryWorldSettings();
  const snapshot = state.snapshot;
  const [name, setName] = useState("");
  const [location, setLocation] = useState("Grass Fields");
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [consent, setConsent] = useState(false);
  const [confirmCreate, setConfirmCreate] = useState(false);
  useEffect(() => { setName(""); setDraft({}); setConsent(false); setConfirmCreate(false); }, [props.details.summary.id]);
  const rules = SATISFACTORY_RULES.filter((rule) => rule.scope === "creation");
  const invalid = rules.some((rule) => !isSatisfactoryRuleValueValid(rule, draft[rule.key] ?? rule.default_value));
  const hasCreative = rules.some((rule) => !rule.key.startsWith("FG.GameMode.") && (draft[rule.key] ?? rule.default_value) !== rule.default_value);
  const nameUsed = Boolean(snapshot?.sessions.some((session) => session.session_name.toLocaleLowerCase() === name.trim().toLocaleLowerCase()));
  const disabled = props.disabled || state.busy || state.pending || state.loading || snapshot?.connection_status !== "ready" || snapshot.connected_players > 0;
  const dirty = Boolean(name || Object.keys(draft).length);
  function edit(key: string, value: string) { setDraft((current) => ({ ...current, [key]: value })); setConfirmCreate(false); }
  async function create() {
    if (!snapshot || disabled || !confirmCreate || invalid || nameUsed || !name.trim() || (hasCreative && !consent)) return;
    const accepted = await state.mutate(() => createSatisfactoryWorld({ instance_id: props.details.summary.id,
      expected_revision: snapshot.revision, session_name: name.trim(), starting_location: location,
      skip_onboarding: true, acknowledge_enable_advanced_settings: consent, ...splitSatisfactoryCreationRules(draft) }));
    if (accepted) setConfirmCreate(false);
  }
  return <SatisfactoryNativePanel sectionId="world_generation" dirty={dirty}>
    <p className="form-note">{t(`${COPY}.creationHelp`)}</p>
    <div className="settings-schema-grid configuration-field-grid">
      <Field fieldKey="satisfactory_new_session_name" sectionId="world_generation" title={t(`${COPY}.newSessionName`)}
        value={name} disabled={disabled} validation={nameUsed ? t(`${COPY}.sessionExists`) : undefined}
        onChange={(value) => { setName(String(value)); setConfirmCreate(false); }} />
      <Field fieldKey="satisfactory_starting_location" sectionId="world_generation" title={t(`${COPY}.startingLocation`)}
        kind="select" value={location} disabled={disabled}
        options={SATISFACTORY_STARTING_LOCATIONS.map((option) => ({ value: option.value, label: t(`${COPY}.locations.${option.label_key}`) }))}
        onChange={(value) => { setLocation(String(value)); setConfirmCreate(false); }} />
    </div>
    <div className="guided-field-groups">
      {([true, false] as const).map((gameMode) => <section key={String(gameMode)} className="guided-field-group">
        <div className="guided-field-group-head"><h5 className="guided-field-group-title">{t(`${COPY}.groups.${gameMode ? "generation" : "startingProgress"}`)}</h5></div>
        <div className="settings-schema-grid configuration-field-grid">
          {rules.filter((rule) => rule.key.startsWith("FG.GameMode.") === gameMode && rule.key !== "FG.GameRules.GiveAllTiers").map((rule) => <SatisfactoryRuleField key={rule.key}
            rule={rule} value={draft[rule.key] ?? rule.default_value} sectionId="world_generation" disabled={disabled}
            validation={!isSatisfactoryRuleValueValid(rule, draft[rule.key] ?? rule.default_value) ? t(`${COPY}.invalidRule`) : undefined}
            onChange={(value) => {
              edit(rule.key, value);
              if (rule.key === "FG.GameRules.StartingTier") edit("FG.GameRules.GiveAllTiers", value === "10" ? "True" : "False");
            }} />)}
        </div>
        {gameMode ? <div className="panel-head panel-head--compact">
          <button type="button" className="secondary-button" disabled={disabled} onClick={() => {
            const values = new Int32Array(1); crypto.getRandomValues(values);
            edit("FG.GameMode.NodeRandomizationSeed", String(values[0] || 1));
          }}>{t(`${COPY}.generateSeed`)}</button>
          <button type="button" className="ghost-button" disabled={disabled}
            onClick={() => edit("FG.GameMode.NodeRandomizationSeed", "0")}>{t(`${COPY}.randomSeed`)}</button>
        </div> : null}
      </section>)}
    </div>
    <div className="settings-schema-grid configuration-field-grid">
      <Field fieldKey="satisfactory_creation_creative_consent" sectionId="world_generation" title={t(`${COPY}.creationCreativeConsent`)}
        kind="boolean" value={consent} disabled={disabled || !hasCreative} onChange={(value) => setConsent(Boolean(value))} />
      <Field fieldKey="satisfactory_confirm_create" sectionId="world_generation" title={t(`${COPY}.confirmCreate`)}
        kind="boolean" value={confirmCreate} disabled={disabled || invalid || nameUsed || !name.trim()}
        onChange={(value) => setConfirmCreate(Boolean(value))} />
    </div>
    {snapshot && snapshot.connected_players > 0 ? <p className="form-note" role="status">{t(`${COPY}.playersPresent`)}</p> : null}
    <button type="button" className="primary-button" disabled={disabled || !confirmCreate || invalid || nameUsed || !name.trim() || (hasCreative && !consent)}
      onClick={() => void create()}>{t(`${COPY}.createWorld`)}</button>
    {state.result === "accepted" || state.result === "loaded" ? <p className="form-note" role="status">
      {t(`${COPY}.${state.result === "loaded" ? "loaded" : "accepted"}`)}</p> : null}
  </SatisfactoryNativePanel>;
}
