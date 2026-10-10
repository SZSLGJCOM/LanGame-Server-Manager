import { useMemo, type ReactNode } from "react";
import {
  dragonwildsWorldSettingCopyKey, dragonwildsWorldSettingGroup, dragonwildsWorldSettingSection,
  dragonwildsWorldSettingValue, isDragonwildsWorldSettingEditable, isDragonwildsWorldSettingValueValid,
  type DragonwildsWorldMode, type DragonwildsWorldSettingDefinition
} from "../../dragonwilds-world-settings";
import { useI18n, type TranslateFn } from "../../i18n";
import { buildConfigurationFieldIds } from "./ConfigurationField";
import { useConfigurationFieldHelp } from "./ConfigurationFieldHelp";
import { ConfigurationSaveStatus } from "./ConfigurationSaveStatus";
import { useDragonwildsWorldSettings } from "./DragonwildsWorldSettingsContext";
import type { ConfigurationSpecializedRendererProps } from "./module-types";

const COPY = "runescapedragonwilds.settings.worldEditor";
const MODES: DragonwildsWorldMode[] = ["Normal", "Hard", "Creative", "Custom"];

function fieldTitle(t: TranslateFn, definition: DragonwildsWorldSettingDefinition): string {
  return t(`${COPY}.fields.${dragonwildsWorldSettingCopyKey(definition.tag)}.title`, undefined, t(`${COPY}.unknownSetting`));
}

function groupTitle(t: TranslateFn, group: string): string {
  return t(`${COPY}.groups.${group}`, undefined, t(`${COPY}.groups.environment`));
}

function Field(props: {
  definition: DragonwildsWorldSettingDefinition; title: string; description: string; t: TranslateFn;
  children: (inputId: string, descriptionId?: string) => ReactNode;
}) {
  const ids = buildConfigurationFieldIds(props.definition.tag, "configuration-runescapedragonwilds");
  const help = useConfigurationFieldHelp(ids.descriptionId, props.description, props.title, props.t);
  return <div className="configuration-field settings-schema-field" data-dragonwilds-setting={props.definition.tag}
    data-field-key={props.definition.tag} ref={help.anchorRef} {...help.interactionProps}>
    <div className="configuration-field-heading">
      <label className="detail-label settings-field-label" htmlFor={ids.inputId}>{props.title}</label>
    </div>
    {help.helpNode}
    <div className="configuration-field-control">{props.children(ids.inputId, help.descriptionId)}</div>
  </div>;
}

export function DragonwildsWorldSettingsPanel(props: ConfigurationSpecializedRendererProps) {
  const { t } = useI18n();
  const editor = useDragonwildsWorldSettings();
  const { snapshot, mode } = editor;
  const saving = editor.status.state === "saving";
  const disabled = props.disabled || !editor.stopped || !snapshot?.writable || editor.loading;
  const grouped = useMemo(() => {
    const groups = new Map<string, DragonwildsWorldSettingDefinition[]>();
    for (const definition of snapshot?.definitions ?? []) {
      if (!isDragonwildsWorldSettingEditable(definition) || dragonwildsWorldSettingSection(definition.tag) !== props.sectionId) continue;
      const group = dragonwildsWorldSettingGroup(definition.tag);
      groups.set(group, [...(groups.get(group) ?? []), definition]);
    }
    return [...groups.entries()];
  }, [props.sectionId, snapshot]);

  function renderControl(definition: DragonwildsWorldSettingDefinition, inputId: string, descriptionId?: string) {
    if (!snapshot || !mode) return null;
    const raw = dragonwildsWorldSettingValue(snapshot, mode, definition, editor.values);
    const valid = isDragonwildsWorldSettingValueValid(definition, raw);
    const title = fieldTitle(t, definition);
    if (definition.kind === "boolean") return <label className="settings-toggle-card" htmlFor={inputId}>
      <span className="settings-toggle-copy">{t(Number(raw) === 1 ? "common.enabled" : "common.disabled")}</span>
      <input id={inputId} type="checkbox" checked={Number(raw) === 1} disabled={disabled || raw === ""}
        aria-describedby={descriptionId} aria-label={title}
        onChange={(event) => { if (!disabled) editor.changeValue(definition, event.target.checked ? "1" : "0"); }} />
    </label>;
    const step = 10 ** -definition.decimal_places;
    return <>
      <input id={inputId} className="settings-schema-input" type="number" inputMode="decimal"
        min={definition.minimum} max={definition.maximum} step={step} value={raw} disabled={disabled}
        aria-describedby={descriptionId} aria-label={title} aria-invalid={!valid || undefined}
        onChange={(event) => { if (!disabled) editor.changeValue(definition, event.target.value); }} />
      {valid ? <input type="range" min={definition.minimum} max={definition.maximum} step={step}
        value={Number(raw)} disabled={disabled} aria-label={t(`${COPY}.sliderLabel`, { title })}
        aria-describedby={descriptionId} style={{ width: "100%", margin: 0, minWidth: 0, accentColor: "var(--accent)" }}
        onChange={(event) => { if (!disabled) editor.changeValue(definition, event.target.value); }} /> : null}
      {!valid ? <span className="form-note form-note--error" role="alert">{t(`${COPY}.invalidValue`)}</span> : null}
    </>;
  }

  return <section className="settings-editor-stack" data-dragonwilds-world-editor={props.sectionId}
    aria-label={t(`${COPY}.title`)} aria-busy={editor.loading || saving}>
    <div className="panel-head panel-head--compact panel-head--spread">
      <h4 className="settings-section-title">{snapshot?.world_name ?? t(`${COPY}.title`)}</h4>
      <button type="button" className="secondary-button" disabled={editor.loading || saving}
        onClick={() => void editor.refresh()}>{t(editor.dirty ? `${COPY}.discardAndRefresh` : `${COPY}.refresh`)}</button>
    </div>
    {editor.error ? <p className="form-note form-note--error" role="alert">{editor.error}</p> : null}
    {editor.loading ? <p className="form-note" role="status">{t(`${COPY}.loading`)}</p> : null}
    {!editor.loading && snapshot?.status === "empty" ? <p className="form-note" role="status">{t(`${COPY}.empty`)}</p> : null}
    {snapshot?.status === "ready" && !editor.stopped ? <p className="form-note" role="status">{t(`${COPY}.stopRequired`)}</p> : null}
    {snapshot?.status === "ready" && editor.stopped && !snapshot.writable ? <p className="form-note" role="status">{snapshot.message ?? t(`${COPY}.refreshRequired`)}</p> : null}
    {snapshot?.status === "ready" && mode ? <>
      {props.sectionId === "world" ? <div className="settings-schema-grid configuration-field-grid">
        <label className="settings-schema-field" data-field-key="world_settings">
          <span className="detail-label settings-field-label">{t(`${COPY}.mode`)}</span>
          <select id="configuration-runescapedragonwilds-world-settings-input" className="settings-schema-input settings-schema-select"
            value={mode} disabled={disabled} onChange={(event) => {
              const selected = MODES.find((value) => value === event.target.value);
              if (selected && !disabled) editor.changeMode(selected);
            }}>
            {MODES.map((value) => <option key={value} value={value}>{t(`${COPY}.modes.${value}`)}</option>)}
          </select>
        </label>
      </div> : null}
      <div className="guided-field-groups">
        {grouped.map(([group, definitions]) => <section className="guided-field-group" key={group} aria-label={groupTitle(t, group)}>
          {grouped.length > 1 || props.sectionId === "world" ? <div className="guided-field-group-head"><h5 className="guided-field-group-title">{groupTitle(t, group)}</h5></div> : null}
          <div className="settings-schema-grid configuration-field-grid">
            {definitions.map((definition) => <Field key={definition.tag} definition={definition} t={t}
              title={fieldTitle(t, definition)} description={t(`${COPY}.fields.${dragonwildsWorldSettingCopyKey(definition.tag)}.description`, undefined, "")}>
              {(inputId, descriptionId) => renderControl(definition, inputId, descriptionId)}
            </Field>)}
          </div>
        </section>)}
      </div>
      <ConfigurationSaveStatus status={editor.status} validationBlocked={editor.invalid} t={t} />
    </> : null}
  </section>;
}
