import { useEffect, useId, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import {
  buildScumCustomZoneDamageInput, isScumZoneHandling, readScumCustomZones, sameScumZoneDamage,
  SCUM_NPC_VEHICLE_DAMAGE_FIELDS, SCUM_ZONE_HANDLING_METHODS, scumZoneKey, writeScumCustomZoneDamage,
  type ScumCustomZoneConfiguration, type ScumCustomZonesSnapshot, type ScumNpcVehicleDamage
} from "../../scum-custom-zones";
import { useConfigurationFieldHelp } from "./ConfigurationFieldHelp";
import type { ConfigurationSpecializedRendererProps } from "./module-types";

const COPY = "scum.settings.customZones";

export function ScumCustomZoneDamageFields(props: {
  zone: ScumCustomZoneConfiguration;
  damage: ScumNpcVehicleDamage;
  disabled: boolean;
  onChange: (damage: ScumNpcVehicleDamage) => void;
}) {
  const { t } = useI18n();
  return <div className="settings-schema-grid configuration-field-grid">
    {SCUM_NPC_VEHICLE_DAMAGE_FIELDS.map((field) => <DamageField key={field} field={field}
      value={props.damage[field]} disabled={props.disabled}
      onChange={(value) => props.onChange({ ...props.damage, [field]: value })} />)}
    <p className="muted settings-schema-wide">{t(`${COPY}.playerDamageUnchanged`)}</p>
  </div>;
}

function DamageField(props: {
  field: typeof SCUM_NPC_VEHICLE_DAMAGE_FIELDS[number];
  value: ScumNpcVehicleDamage["melee"];
  disabled: boolean;
  onChange: (value: ScumNpcVehicleDamage["melee"]) => void;
}) {
  const { t } = useI18n();
  const id = useId();
  const label = t(`${COPY}.fields.${props.field}.title`);
  const help = useConfigurationFieldHelp(`${id}-help`, t(`${COPY}.fields.${props.field}.description`), label, t);
  return <div className="settings-schema-field" data-field-key={`custom_zones.${props.field}`}
    ref={help.anchorRef} {...help.interactionProps}>
    <label className="settings-field-label" htmlFor={id}>{label}</label>
    {help.helpNode}
    <select id={id} className="settings-schema-input settings-schema-select" value={props.value}
      aria-describedby={help.descriptionId} disabled={props.disabled} onChange={(event) => {
        if (isScumZoneHandling(event.target.value)) props.onChange(event.target.value);
      }}>
      {SCUM_ZONE_HANDLING_METHODS.map((value) => <option key={value} value={value}>{t(`${COPY}.handling.${value}`)}</option>)}
    </select>
  </div>;
}

export function ScumCustomZonesRenderer(props: ConfigurationSpecializedRendererProps) {
  const { t } = useI18n();
  const instanceId = props.details.summary.id;
  const stopped = props.details.summary.status === "Stopped" && props.details.summary.active_process_count === 0
    && props.details.active_run === null;
  const [snapshot, setSnapshot] = useState<ScumCustomZonesSnapshot | null>(null);
  const [selectedKey, setSelectedKey] = useState("");
  const [draft, setDraft] = useState<ScumNpcVehicleDamage | null>(null);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const generation = useRef(0);
  const selected = snapshot?.configurations.find((zone) => scumZoneKey(zone) === selectedKey);
  const dirty = !!selected && !!draft && !sameScumZoneDamage(draft, selected.damage);
  const disabled = props.disabled || !stopped || loading || saving || !snapshot?.writable;

  useEffect(() => {
    const current = ++generation.current;
    if (props.active === false) return;
    setLoading(true); setSaving(false); setError(null); setSaved(false); setSnapshot(null); setDraft(null);
    void readScumCustomZones(instanceId).then((result) => {
      if (generation.current !== current) return;
      if (result.instance_id !== instanceId) throw new Error(t(`${COPY}.invalidResponse`));
      const first = result.configurations[0];
      setSnapshot(result); setSelectedKey(first ? scumZoneKey(first) : "");
      setDraft(first ? { ...first.damage } : null);
    }).catch((reason: unknown) => {
      if (generation.current === current) setError(String(reason));
    }).finally(() => { if (generation.current === current) setLoading(false); });
    return () => { generation.current++; };
  }, [instanceId, stopped, props.active, refresh, t]);

  async function save() {
    if (!snapshot || !selected || !draft || disabled || !dirty) return;
    const current = generation.current;
    setSaving(true); setError(null); setSaved(false);
    try {
      const input = buildScumCustomZoneDamageInput(snapshot, selected, draft);
      const result = await writeScumCustomZoneDamage(input);
      if (generation.current !== current) return;
      const persisted = result.configurations.find((zone) => scumZoneKey(zone) === scumZoneKey(selected));
      if (result.instance_id !== instanceId || result.status !== "ready" || !persisted ||
        !sameScumZoneDamage(persisted.damage, input.damage)) throw new Error(t(`${COPY}.invalidResponse`));
      setSnapshot(result); setDraft({ ...persisted.damage }); setSaved(true);
    } catch (reason: unknown) {
      if (generation.current === current) setError(String(reason));
    } finally { if (generation.current === current) setSaving(false); }
  }

  const status = !stopped ? "running" : snapshot?.status;
  return <section className="settings-editor-stack scum-custom-zones" aria-label={t(`${COPY}.title`)} aria-busy={loading || saving}>
    <p className="muted">{t(`${COPY}.description`)}</p>
    <div className="scum-custom-zones__actions">
      <button type="button" className="secondary-button" disabled={loading || saving || props.disabled}
        onClick={() => setRefresh((value) => value + 1)}>
        {t(dirty ? `${COPY}.discardAndReload` : `${COPY}.reload`)}
      </button>
      {loading ? <span role="status">{t(`${COPY}.loading`)}</span> : null}
      {saving ? <span role="status">{t(`${COPY}.saving`)}</span> : null}
    </div>
    {error ? <p className="error-text" role="alert">{t(`${COPY}.failed`)} {error}</p> : null}
    {status && status !== "ready" ? <p className="muted" role="status">{t(`${COPY}.states.${status}`)}</p> : null}
    {status === "ready" && selected && draft ? <>
      {!snapshot?.writable ? <p className="muted" role="status">{t(`${COPY}.readOnly`)}</p> : null}
      <label className="settings-schema-field">
        <span className="settings-field-label">{t(`${COPY}.configuration`)}</span>
        <select className="settings-schema-input settings-schema-select" value={selectedKey}
          disabled={loading || saving || dirty || props.disabled} onChange={(event) => {
            const zone = snapshot?.configurations.find((entry) => scumZoneKey(entry) === event.target.value);
            if (!zone) return;
            setSelectedKey(scumZoneKey(zone)); setDraft({ ...zone.damage }); setSaved(false); setError(null);
          }}>
          {snapshot?.configurations.map((zone) => <option key={scumZoneKey(zone)} value={scumZoneKey(zone)}>
            {t(`${COPY}.map`, { id: zone.map_id })} · {zone.global ? t(`${COPY}.global`) : zone.name || t(`${COPY}.unnamed`, { id: zone.configuration_id })}
          </option>)}
        </select>
      </label>
      <p className="muted scum-custom-zones__regions">{selected.global ? t(`${COPY}.globalHelp`)
        : selected.regions.length ? t(`${COPY}.regions`, { names: selected.regions.join(", ") }) : t(`${COPY}.unassigned`)}</p>
      <ScumCustomZoneDamageFields zone={selected} damage={draft} disabled={disabled} onChange={(value) => {
        setDraft(value); setSaved(false); setError(null);
      }} />
      <div className="scum-custom-zones__actions">
        <button type="button" className="primary-button" disabled={disabled || !dirty} onClick={() => { void save(); }}>
          {t(`${COPY}.save`)}
        </button>
        <button type="button" className="secondary-button" disabled={saving || !dirty || props.disabled}
          onClick={() => { setDraft({ ...selected.damage }); setError(null); setSaved(false); }}>
          {t(`${COPY}.revert`)}
        </button>
        {dirty ? <span role="status">{t(`${COPY}.unsaved`)}</span> : null}
        {saved ? <span role="status">{t(`${COPY}.saved`)}</span> : null}
      </div>
    </> : null}
  </section>;
}
