import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { readAstroneerSaveCatalog, type AstroneerSaveCatalog, type AstroneerSaveEntry } from "../../astroneer-saves";
import { useI18n } from "../../i18n";
import { ConfigurationField, type ConfigurationSpecializedRendererProps as FieldRendererProps } from "./ConfigurationField";
import { buildConfigurationFieldCopy } from "./GuidedSettingsForm";
import { parseGuidedSettingsSchema, readGuidedFieldValue } from "./guided-settings";
import type { ConfigurationSpecializedRendererProps } from "./module-types";

type CatalogState = { instanceId: string } & (
  | { status: "loading" }
  | { status: "ready"; catalog: AstroneerSaveCatalog }
  | { status: "error"; error: string }
);

const SaveControlContext = createContext<{
  visible: CatalogState;
  entries: AstroneerSaveEntry[];
  name: string;
  present: boolean;
  selectionDisabled: boolean;
  refresh(): Promise<void>;
} | null>(null);

function SaveControl(control: FieldRendererProps) {
  const context = useContext(SaveControlContext);
  const { t } = useI18n();
  if (!context) throw new Error("Missing ASTRONEER save control context.");
  const { visible, entries, name, present, selectionDisabled, refresh } = context;
  return <div className="settings-editor-stack" aria-busy={visible.status === "loading"}>
    <select id={control.inputId} className="settings-schema-input settings-schema-select"
      value={name} disabled={selectionDisabled} aria-describedby={control.descriptionId}
      onChange={(event) => {
        if (selectionDisabled || !entries.some((entry) => entry.descriptive_name === event.target.value)) return;
        control.onPatch({ active_save_file_name: event.target.value });
      }}>
      {!present ? <option value={name} disabled>
        {t("astroneer.settings.saves.configuredOption", { name: name || t("astroneer.settings.saves.emptyName") })}
      </option> : null}
      {entries.map((entry) => <option key={entry.descriptive_name} value={entry.descriptive_name}>
        {entry.descriptive_name}
      </option>)}
    </select>
    {visible.status === "loading" ? <p className="form-note" role="status">
      {t("astroneer.settings.saves.loading")}
    </p> : visible.status === "error" ? <div>
      <p className="form-note form-note--error" role="alert">
        {t("astroneer.settings.saves.readError", { error: visible.error })}
      </p>
      <button type="button" className="secondary-button" disabled={control.disabled}
        onClick={() => { if (!control.disabled) void refresh(); }}>{t("common.retry", undefined, "Retry")}</button>
    </div> : <>
      {entries.length === 0 ? <p className="form-note" role="status">{t("astroneer.settings.saves.empty")}</p>
        : !present ? <p className="form-note" role="status">{t("astroneer.settings.saves.unmatched")}</p> : null}
      <button type="button" className="secondary-button" disabled={control.disabled}
        onClick={() => { if (!control.disabled) void refresh(); }}>{t("astroneer.settings.saves.refresh")}</button>
    </>}
  </div>;
}

export function AstroneerSaveSelect(props: ConfigurationSpecializedRendererProps) {
  const { locale, t } = useI18n();
  const instanceId = props.details.summary.id;
  const generation = useRef(0);
  const [state, setState] = useState<CatalogState>({ instanceId, status: "loading" });
  const refresh = useCallback(async () => {
    const request = ++generation.current;
    setState({ instanceId, status: "loading" });
    try {
      const catalog = await readAstroneerSaveCatalog(instanceId);
      if (generation.current === request) setState({ instanceId, status: "ready", catalog });
    } catch (error) {
      if (generation.current === request) setState({ instanceId, status: "error", error: String(error) });
    }
  }, [instanceId]);
  useEffect(() => {
    void refresh();
    return () => { generation.current += 1; };
  }, [refresh]);
  const schema = useMemo(() => parseGuidedSettingsSchema(props.moduleDetails, locale, t),
    [locale, props.moduleDetails, t]);
  const field = schema.fields.find((candidate) => candidate.key === "active_save_file_name");
  if (!field) return null;
  const visible: CatalogState = state.instanceId === instanceId ? state : { instanceId, status: "loading" };
  const entries = visible.status === "ready" ? visible.catalog.entries : [];
  const value = readGuidedFieldValue(field, props.settings);
  const name = typeof value === "string" ? value : String(value ?? "");
  const present = entries.some((entry) => entry.descriptive_name === name);
  const selectionDisabled = props.disabled || visible.status !== "ready" || entries.length === 0;

  return <SaveControlContext.Provider value={{ visible, entries, name, present, selectionDisabled, refresh }}>
    <div className="settings-schema-grid configuration-field-grid">
      <ConfigurationField field={field} value={name} settings={props.settings} copy={buildConfigurationFieldCopy(t)}
        disabled={props.disabled} onPatch={props.onPatch} idPrefix="configuration-astroneer" t={t}
        renderSpecialized={SaveControl} />
    </div>
  </SaveControlContext.Provider>;
}
