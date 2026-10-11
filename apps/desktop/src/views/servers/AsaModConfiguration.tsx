import { useEffect, useId, useMemo, useRef, useState } from "react";
import { describeError } from "../../app-state";
import { openExternalUrl } from "../../api";
import { ActivityNotice } from "../../components/ActivityNotice";
import { InlineConfirmAction } from "../../components/InlineConfirmAction";
import { ShellIcon } from "../../components/ShellIcon";
import { useI18n } from "../../i18n";
import { ConfigurationHelp, useConfigurationFieldHelp } from "../settings/ConfigurationFieldHelp";
import type { SettingsObject } from "../settings/settings-schema";
import { canonicalAsaModId } from "./mod-workbench-asa";
import {
  ASA_MOD_AUTHOR_CATALOG, ASA_MOD_CONFIGURATION_TOTAL_LIMIT, ASA_MOD_INI_FILES, ASA_MOD_INI_LIMIT,
  AsaModConfigurationError, asaModParameterAssignments, asaModParameterValue, buildAsaModConfigurationSettings,
  decodeAsaModIniFile, findAsaModIniConflicts, localizedAsaModText, parseAsaModIni,
  readAsaModConfigurations, setAsaModIniParameter,
  type AsaModAuthorDefinition, type AsaModConfiguration, type AsaModConfigurationIssue, type AsaModIniFile, type AsaModParameterDefinition,
} from "./asa-mod-configuration";
import { asaModConfigurationText } from "./asa-mod-configuration-messages";
import "./asa-mod-configuration.css";

export interface AsaModConfigurationProps {
  instanceId: string;
  modId: string;
  settings: SettingsObject;
  disabled: boolean;
  readOnly?: boolean;
  installed?: boolean;
  authorDefinitions?: readonly AsaModAuthorDefinition[];
  onSave: (nextSettings: SettingsObject, expectedBase: SettingsObject) => Promise<SettingsObject | void>;
  onReload?: () => Promise<SettingsObject>;
}
type Draft = { text: string; base: string; revision: number };
type Drafts = Record<string, Draft>;
type Feedback = { error?: string; saved?: boolean; note?: string };

/** The instance boundary owns drafts; changing Mod/file selection does not remount them. */
export function AsaModConfiguration(props: AsaModConfigurationProps) {
  return <Editor key={props.instanceId} {...props} />;
}

function AuthorField(props: {
  parameter: AsaModParameterDefinition; text: string; locale: string; editable: boolean;
  onChange: (value: string | null) => void;
}) {
  const id = useId(), parameter = props.parameter;
  const label = localizedAsaModText(parameter.label, props.locale) || parameter.key;
  const help = useConfigurationFieldHelp(`${id}-help`, [label, localizedAsaModText(parameter.description, props.locale),
    `${ASA_MOD_INI_FILES[parameter.file]}\n[${parameter.section}] ${parameter.key}`].filter(Boolean).join("\n\n"), label, undefined, "instructions");
  const matches = asaModParameterAssignments(props.text, parameter);
  const value = matches.length === 1 ? matches[0].value : undefined;
  const configText = (key: Parameters<typeof asaModConfigurationText>[1]) => asaModConfigurationText(props.locale, key);
  const boolean = parameter.kind === "boolean";
  const choices = boolean ? [
    { value: parameter.trueValue ?? "True", label: configText("on") }, { value: parameter.falseValue ?? "False", label: configText("off") },
  ] : parameter.choices?.map((choice) => ({ value: choice.value, label: localizedAsaModText(choice.label, props.locale) })) ?? [];
  const editable = props.editable && matches.length <= 1;
  return <div className="dst-mod-spec-row asa-mod-config__field" data-asa-mod-parameter={parameter.key} ref={help.anchorRef} {...help.interactionProps}>
    <label className="dst-mod-spec-copy" htmlFor={id}><span className="dst-mod-spec-label">{label}</span></label>
    {boolean || parameter.kind === "select" ? <select id={id} className="settings-schema-input" disabled={!editable}
      aria-describedby={help.descriptionId} value={value === undefined ? "__default__" : value}
      onChange={(event) => props.onChange(event.target.value === "__default__" ? null : event.target.value)}>
      <option value="__default__">{configText("default")}{parameter.defaultValue !== undefined ? ` (${parameter.defaultValue})` : ""}</option>
      {value !== undefined && !choices.some((choice) => choice.value === value) ? <option value={value}>{value}</option> : null}
      {choices.map((choice) => <option key={choice.value} value={choice.value}>{choice.label}</option>)}
    </select> : <input id={id} className="settings-schema-input" type={parameter.kind === "number" ? "number" : "text"}
      step={parameter.kind === "number" ? "any" : undefined} value={value ?? ""} disabled={!editable}
      aria-describedby={help.descriptionId} placeholder={parameter.defaultValue ?? configText("default")}
      onChange={(event) => props.onChange(event.target.value || null)} />}
    {matches.length > 1 ? <small className="asa-mod-config__field-note">{configText("duplicate")}</small> : null}
    {help.helpNode}
  </div>;
}

function Editor(props: AsaModConfigurationProps) {
  const { locale } = useI18n(), id = useId();
  const configText = (key: Parameters<typeof asaModConfigurationText>[1], params?: Record<string, string | number>) => asaModConfigurationText(locale, key, params);
  const [drafts, setDrafts] = useState<Drafts>({});
  const draftsRef = useRef(drafts); draftsRef.current = drafts;
  const [selectedFiles, setSelectedFiles] = useState<Record<string, AsaModIniFile>>({});
  const [feedback, setFeedback] = useState<Record<string, Feedback>>({});
  const [savingKey, setSavingKey] = useState<string | null>(null);
  const [reloadingKey, setReloadingKey] = useState<string | null>(null);
  const [importing, setImporting] = useState(false);
  const propsSignature = useMemo(() => JSON.stringify(props.settings), [props.settings]);
  const propsSignatureRef = useRef(propsSignature); propsSignatureRef.current = propsSignature;
  // A fresh read/save remains authoritative while the parent still renders its
  // previous snapshot. New parent contents supersede it without erasing drafts.
  const [snapshot, setSnapshot] = useState<{ sourceProps: string; settings: SettingsObject } | null>(null);
  const settings = snapshot?.sourceProps === propsSignature ? snapshot.settings : props.settings;
  useEffect(() => {
    setSnapshot((previous) => previous && previous.sourceProps !== propsSignature ? null : previous);
  }, [propsSignature]);
  const currentProps = useRef(props.settings); currentProps.current = props.settings;
  const fileInput = useRef<HTMLInputElement>(null), mounted = useRef(true), inFlight = useRef(false), importRevision = useRef(0);
  const importOwner = useRef<number | null>(null);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; importRevision.current++; }; }, []);
  const validId = canonicalAsaModId(props.modId) === props.modId;
  const author = (props.authorDefinitions ?? ASA_MOD_AUTHOR_CATALOG).find((entry) => entry.modId === props.modId);
  const file = selectedFiles[props.modId] ?? author?.parameters[0]?.file ?? "game_ini";
  const key = `${props.modId}:${file}`;
  const loaded = useMemo<{ configs: Record<string, AsaModConfiguration>; error: unknown }>(() => {
    try { return { configs: readAsaModConfigurations(settings), error: null }; }
    catch (error) { return { configs: {}, error }; }
  }, [settings]);
  const saved = loaded.configs[props.modId]?.[file] ?? "";
  const draft = drafts[key] ?? { text: saved, base: saved, revision: 0 };
  const parsed = useMemo(() => parseAsaModIni(draft.text), [draft.text]);
  const issues = useMemo<AsaModConfigurationIssue[]>(() => {
    try { return [...parsed.issues, ...findAsaModIniConflicts(settings, props.modId, file, parsed.entries)]; }
    catch (error) { return [...parsed.issues, error instanceof AsaModConfigurationError ? error.issue : { code: "shape", severity: "error" } as const]; }
  }, [parsed, settings, props.modId, file]);
  const dirty = draft.text !== draft.base;
  const editable = validId && !loaded.error && !props.disabled && !props.readOnly;
  const canSave = editable && dirty && !savingKey && !reloadingKey && !importing && !issues.some((issue) => issue.severity === "error");
  const current = useRef({ key, editable });
  current.current = { key, editable };
  useEffect(() => { importRevision.current++; importOwner.current = null; setImporting(false); }, [key]);
  const issueText = (issue: AsaModConfigurationIssue) => configText(`issue.${issue.code}`, {
    line: issue.line ?? "", section: issue.section ?? "", key: issue.key ?? "", owner: issue.owner ?? "",
  });
  const errorText = (error: unknown) => error instanceof AsaModConfigurationError ? issueText(error.issue) : describeError(error);

  function edit(text: string) {
    if (!current.current.editable) return;
    const next = { ...draftsRef.current, [key]: { text, base: draft.base, revision: draft.revision + 1 } };
    const size = Object.values(next).reduce((total, entry) => total + new TextEncoder().encode(entry.text).length, 0);
    if (size > ASA_MOD_CONFIGURATION_TOTAL_LIMIT) {
      setFeedback((previous) => ({ ...previous, [key]: { error: configText("draftLimit") } })); return;
    }
    draftsRef.current = next; setDrafts(next); setSelectedFiles((previous) => ({ ...previous, [props.modId]: file }));
    importRevision.current++; setFeedback((previous) => ({ ...previous, [key]: {} }));
  }
  async function reload() {
    if (!editable || inFlight.current) return;
    const target = key, revision = draft.revision, targetMod = props.modId, targetFile = file, sourceProps = propsSignature;
    inFlight.current = true; setReloadingKey(target);
    importRevision.current++;
    try {
      const fresh = structuredClone(props.onReload ? await props.onReload() : settings);
      const text = readAsaModConfigurations(fresh)[targetMod]?.[targetFile] ?? "";
      if (!mounted.current) return;
      if (propsSignatureRef.current !== sourceProps && JSON.stringify(fresh) !== JSON.stringify(currentProps.current)) {
        throw new AsaModConfigurationError({ code: "base-changed", severity: "error" });
      }
      setSnapshot({ sourceProps, settings: fresh });
      const existing = draftsRef.current[target];
      if ((existing?.revision ?? 0) === revision) {
        const next = { ...draftsRef.current, [target]: { text, base: text, revision: revision + 1 } };
        draftsRef.current = next; setDrafts(next);
        setFeedback((previous) => ({ ...previous, [target]: {} }));
      }
    } catch (error) {
      if (mounted.current) setFeedback((previous) => ({ ...previous, [target]: { error: errorText(error) } }));
      throw error;
    } finally {
      inFlight.current = false; if (mounted.current) setReloadingKey(null);
    }
  }
  async function importFile(selected: File | undefined) {
    if (!selected || !editable || importing) return;
    const request = ++importRevision.current, target = key;
    importOwner.current = request;
    setImporting(true);
    try {
      if (!/\.ini$/i.test(selected.name)) throw new AsaModConfigurationError({ code: "file-type", severity: "error" });
      if (selected.size > ASA_MOD_INI_LIMIT) throw new AsaModConfigurationError({ code: "size", severity: "error" });
      const text = decodeAsaModIniFile(await selected.arrayBuffer());
      if (mounted.current && request === importRevision.current && current.current.key === target && current.current.editable) edit(text);
    } catch (error) {
      if (mounted.current && request === importRevision.current && current.current.key === target) {
        setFeedback((previous) => ({ ...previous, [target]: { error: errorText(error) } }));
      }
    } finally {
      if (mounted.current && importOwner.current === request) { importOwner.current = null; setImporting(false); }
    }
  }
  async function save() {
    if (!canSave || inFlight.current) return;
    const target = key, revision = draft.revision, text = draft.text, expectedBase = structuredClone(settings), sourceProps = propsSignature;
    inFlight.current = true; setSavingKey(target); setFeedback((previous) => ({ ...previous, [target]: {} }));
    try {
      const next = buildAsaModConfigurationSettings(expectedBase, props.modId, file, text, draft.base);
      const persisted = await props.onSave(next, expectedBase);
      if (!mounted.current) return;
      const authoritative = structuredClone(persisted ?? next);
      const savedText = readAsaModConfigurations(authoritative)[props.modId]?.[file] ?? "";
      if (propsSignatureRef.current !== sourceProps && JSON.stringify(authoritative) !== JSON.stringify(currentProps.current)) {
        setFeedback((previous) => ({ ...previous, [target]: { saved: true, note: configText("savedChanged") } }));
        return;
      }
      setSnapshot({ sourceProps, settings: authoritative });
      setDrafts((previous) => {
        const existing = previous[target];
        if (!existing) return previous;
        const nextDrafts = { ...previous, [target]: { ...existing, base: savedText } };
        if (existing.revision === revision) nextDrafts[target].text = nextDrafts[target].base;
        draftsRef.current = nextDrafts; return nextDrafts;
      });
      setFeedback((previous) => ({ ...previous, [target]: { saved: true } }));
    } catch (error) {
      if (mounted.current) setFeedback((previous) => ({ ...previous, [target]: { error: errorText(error) } }));
    } finally {
      inFlight.current = false; if (mounted.current) setSavingKey(null);
    }
  }
  function updateParameter(parameter: AsaModParameterDefinition, value: string | null) {
    try { edit(setAsaModIniParameter(draft.text, parameter, value)); }
    catch (error) { setFeedback((previous) => ({ ...previous, [key]: { error: errorText(error) } })); }
  }
  const parameters = author?.parameters.filter((parameter) => parameter.file === file) ?? [];
  const sourceUrl = author?.sourceUrl;
  const headingHelp = [configText("shared"), !author ? configText("unknown") : "", props.installed === false ? configText("installedMissing") : ""].filter(Boolean).join("\n\n");
  const iniHelp = [configText("pasteHelp"), configText("nativeValidation"), configText("tupleValidation")].join("\n\n");
  const showReload = dirty || Boolean(feedback[key]?.error || feedback[key]?.note || reloadingKey);
  const dependencies = parameters.filter((parameter) => parameter.requires && asaModParameterValue(draft.text, parameter)?.toLowerCase()
    === (parameter.trueValue ?? "True").toLowerCase()).filter((parameter) => {
      const required = parameter.requires!;
      const otherKey = `${props.modId}:${required.file}`;
      const otherText = drafts[otherKey]?.text ?? loaded.configs[props.modId]?.[required.file] ?? "";
      return asaModParameterValue(otherText, { ...parameter, ...required })?.toLowerCase() !== required.value.toLowerCase();
    });
  return <section className="dst-mod-config-shell asa-mod-config" aria-busy={Boolean(savingKey || reloadingKey) || importing} data-asa-mod-config={props.modId}>
    <div className="dst-mod-section-head asa-mod-config__heading">
      <ConfigurationHelp description={headingHelp}>{(help) => <h3 tabIndex={0} className="asa-mod-config__help-label"
        ref={help.anchorRef} {...help.interactionProps} aria-describedby={help.descriptionId}>{author?.title ?? configText("title")}</h3>}</ConfigurationHelp>
      {sourceUrl && /^https:\/\//i.test(sourceUrl) ? <ConfigurationHelp description={configText("authorDocs")}>{(help) =>
        <button type="button" className="ghost-button asa-mod-config__author-link" aria-label={configText("authorDocs")}
          ref={help.anchorRef} {...help.interactionProps} aria-describedby={help.descriptionId}
          onClick={() => { void openExternalUrl(sourceUrl).catch((error) => setFeedback((previous) => ({ ...previous, [key]: { error: describeError(error) } }))); }}>
          <ShellIcon name="external-link" /></button>}</ConfigurationHelp> : null}
      {dirty || feedback[key]?.saved ? <span className="asa-mod-config__dirty" role="status">{dirty ? configText("unsaved") : configText("saved")}</span> : null}
    </div>
    {!validId || loaded.error ? <ActivityNotice tone="error">{!validId ? configText("issue.invalid-id") : errorText(loaded.error)}</ActivityNotice> : null}
    <div className="asa-mod-config__toolbar">
      <ConfigurationHelp description={configText("targetHelp")}>{(help) => <select id={`${id}-target`} className="settings-schema-input"
        aria-label={configText("target")} ref={help.anchorRef} {...help.interactionProps} aria-describedby={help.descriptionId}
        data-asa-mod-file value={file} onChange={(event) => setSelectedFiles((previous) => ({ ...previous, [props.modId]: event.target.value as AsaModIniFile }))}>
        {Object.entries(ASA_MOD_INI_FILES).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
      </select>}</ConfigurationHelp>
      <button type="button" className="ghost-button" disabled={!editable || importing} onClick={() => fileInput.current?.click()}>{configText("import")}</button>
      <input ref={fileInput} className="asa-mod-config__file-input" type="file" accept=".ini" tabIndex={-1}
        disabled={!editable || importing} aria-label={configText("import")} onChange={(event) => {
          const selected = event.target.files?.[0]; event.target.value = ""; void importFile(selected);
        }} />
    </div>
    {parameters.length ? <div className="dst-mod-spec-list">{parameters.map((parameter) => <AuthorField
      key={`${parameter.section}:${parameter.key}`} parameter={parameter} text={draft.text} locale={locale}
      editable={editable && !importing} onChange={(value) => updateParameter(parameter, value)} />)}</div> : null}
    {dependencies.map((parameter) => <p className="asa-mod-config__note" role="status" key={`${parameter.section}:${parameter.key}`}>
      {configText("dependency", { key: parameter.key, section: parameter.requires!.section, required: parameter.requires!.key, value: parameter.requires!.value })}</p>)}
    <ConfigurationHelp description={iniHelp}>{(help) => <label className="asa-mod-config__input-label asa-mod-config__help-label"
      tabIndex={0} htmlFor={`${id}-text`} ref={help.anchorRef} {...help.interactionProps}
      aria-describedby={help.descriptionId}>{configText("paste")}</label>}</ConfigurationHelp>
    <textarea id={`${id}-text`} className="mw-textarea asa-mod-config__input" value={draft.text}
      disabled={!editable || importing} spellCheck={false} maxLength={ASA_MOD_INI_LIMIT + 1}
      aria-invalid={issues.some((issue) => issue.severity === "error")}
      onChange={(event) => edit(event.target.value)} />
    {issues.slice(0, 12).map((issue, index) => <p className={`asa-mod-config__issue is-${issue.severity}`} role={issue.severity === "error" ? "alert" : "status"}
      key={`${issue.code}:${issue.line}:${issue.owner}:${index}`}>{issueText(issue)}</p>)}
    {feedback[key]?.error ? <ActivityNotice tone="error">{feedback[key].error!}</ActivityNotice> : null}
    {feedback[key]?.note ? <p className="asa-mod-config__note" role="status">{feedback[key].note}</p> : null}
    {parsed.isClear && dirty ? <p className="asa-mod-config__clear-review" role="status">{configText("clearReview", { id: props.modId, file: ASA_MOD_INI_FILES[file] })}</p> : null}
    {parsed.entries.length ? <details className="asa-mod-config__preview" key={key}><summary>{configText("preview")}</summary>
      <table><thead><tr><th>{configText("line")}</th><th>{configText("section")}</th><th>{configText("key")}</th><th>{configText("value")}</th></tr></thead>
        <tbody>{parsed.entries.slice(0, 200).map((entry) => <tr key={entry.line}><td>{entry.line}</td><td>{entry.section}</td><td>{entry.key}</td><td>{entry.value}</td></tr>)}</tbody></table>
        {parsed.entries.length > 200 ? <p className="asa-mod-config__note">{configText("previewLimit", { shown: 200, total: parsed.entries.length })}</p> : null}
    </details> : null}
    <div className="asa-mod-config__actions">
      <ConfigurationHelp description={props.disabled && !props.readOnly ? configText("stopped") : configText("saveHelp", { file: ASA_MOD_INI_FILES[file] })}>{(help) =>
        <span ref={help.anchorRef} {...help.interactionProps} tabIndex={!canSave ? 0 : undefined} aria-describedby={help.descriptionId}>
          <button type="button" className="mw-btn mw-btn--primary" data-asa-mod-save disabled={!canSave} onClick={() => void save()}>
            {savingKey === key ? configText("saving") : parsed.isClear && dirty ? configText("remove") : configText("save")}</button>
        </span>}</ConfigurationHelp>
      {showReload ? <InlineConfirmAction className="ghost-button" data-asa-mod-reload scopeKey={`${props.instanceId}:${key}`}
        disabled={!editable || (!dirty && !props.onReload) || Boolean(savingKey || reloadingKey)}
        confirmation={configText("resetConfirm", { file: ASA_MOD_INI_FILES[file] })} onConfirm={reload}>{configText("reload")}</InlineConfirmAction> : null}
    </div>
  </section>;
}
