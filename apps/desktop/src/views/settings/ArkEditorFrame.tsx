import { useEffect, useState, type ReactNode } from "react";
import { useI18n, type TranslateFn } from "../../i18n";
import { ARK_EDITORS_EN } from "../../i18n/games/ark-editors.en";
import { buildConfigurationFieldIds } from "./ConfigurationField";
import type { ConfigurationSpecializedRendererProps } from "./module-types";
import { validateArkComplexField } from "./ark-complex-validation";

export function arkText(t: TranslateFn, key: string, values?: Record<string, string | number>): string {
  return t(`arkEditor.${key}`, values, ARK_EDITORS_EN[`arkEditor.${key}`] ?? key);
}
export interface ArkFieldEditorProps extends ConfigurationSpecializedRendererProps { settingKey: string }
export function arkFieldInputId(props: ArkFieldEditorProps): string {
  return buildConfigurationFieldIds(props.settingKey, `configuration-${props.moduleDetails.summary.id}`).inputId;
}

export function ArkEditorFrame(props: ArkFieldEditorProps & { children: ReactNode }) {
  const { t } = useI18n();
  const [native, setNative] = useState(false);
  const raw = String(props.settings[props.settingKey] ?? "");
  const error = validateArkComplexField(props.settingKey, raw);
  const title = t(`settings.schema.${props.moduleDetails.summary.id}.${props.settingKey}.title`, undefined, props.settingKey);
  const id = arkFieldInputId(props);
  return <section className="ark-editor" data-field-key={props.settingKey} aria-label={title}>
    <div className="ark-editor__heading"><h3>{title}</h3>
      <button id={`${id}-mode`} type="button" className="ghost-button" aria-pressed={native || Boolean(error)}
        onClick={() => setNative(!native)} disabled={props.disabled || Boolean(error)}>
        {arkText(t, native ? "table" : "native")}</button>
    </div>
    {error ? <p className="ark-editor__error" role="alert">{arkText(t, "rawNotice")} {arkText(t, "invalid")}</p> : null}
    {native || error ? <textarea id={id} className="settings-schema-input settings-schema-textarea" rows={8}
      aria-label={arkText(t, "nativeField", { name: title })} value={raw} disabled={props.disabled}
      aria-invalid={Boolean(error)} onChange={(event) => props.onPatch({ [props.settingKey]: event.target.value })} /> : props.children}
  </section>;
}

/** Keep partially typed numbers local until blur; a parent refresh cannot overwrite a dirty input. */
export function ArkCommitInput(props: { value: string; label: string; disabled?: boolean; placeholder?: string;
  id?: string; list?: string; numeric?: boolean; onCommit: (value: string) => void }) {
  const { t } = useI18n();
  const [draft, setDraft] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => { if (draft === props.value) setDraft(null); }, [draft, props.value]);
  return <div><input id={props.id} list={props.list} className="settings-schema-input" aria-label={props.label} disabled={props.disabled} aria-invalid={failed || undefined}
    inputMode={props.numeric ? "decimal" : undefined} value={draft ?? props.value} placeholder={props.placeholder}
    onChange={(event) => { setDraft(event.target.value); setFailed(false); }}
    onBlur={() => {
      if (draft === null) return;
      try { props.onCommit(draft); setDraft(null); setFailed(false); }
      catch { setFailed(true); }
    }}
    onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); }} />
    {failed ? <span role="alert" className="ark-editor__error">{arkText(t, "editError")}</span> : null}</div>;
}
