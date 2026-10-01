import { ActivityNotice } from "../../components/ActivityNotice";
import { useEffect, useMemo, useRef, useState } from "react";
import { listInstancesFromStorage, readInstanceDetails } from "../../api";
import { useI18n } from "../../i18n";
import type { InstanceSummary } from "../../types";
import type { ConfigurationWorkspaceToolsProps } from "./module-types";
import type { SettingsObject } from "./settings-schema";
import { ARK_IMPORT_MAX_BYTES, createArkPreset, importArkIniDocuments, isPortableArkField, readArkPreset } from "./ark-configuration-transfer";

type TransferMode = "import" | "copy" | "export";
interface Preview { patch: SettingsObject; notes: string[]; issues: string[] }

export function ArkConfigurationTransfer(props: ConfigurationWorkspaceToolsProps) {
  const { t } = useI18n();
  const moduleId = props.details.summary.module_id;
  const dialog = useRef<HTMLDialogElement>(null);
  const request = useRef(0);
  const [mode, setMode] = useState<TransferMode | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<Preview | null>(null);
  const [instances, setInstances] = useState<InstanceSummary[]>([]);
  const [sourceId, setSourceId] = useState("");
  const [sections, setSections] = useState<string[]>([]);
  const [copied, setCopied] = useState(false);
  const fields = props.schema.presentationFields ?? props.schema.fields;
  const groups = props.schema.sections.filter((section) => fields.some((field) => field.sectionId === section.id && isPortableArkField(field)));
  const fieldMap = new Map(fields.map((field) => [field.key, field]));
  const properties = useMemo(() => {
    const schema: unknown = JSON.parse(props.moduleDetails.schema_json ?? "{}");
    if (!schema || typeof schema !== "object" || !("properties" in schema) || !schema.properties || typeof schema.properties !== "object") return {};
    return schema.properties as Record<string, Record<string, unknown>>;
  }, [props.moduleDetails.schema_json]);
  const preset = useMemo(() => createArkPreset(moduleId, props.settings, fields, sections), [moduleId, props.settings, fields, sections]);
  const presetText = JSON.stringify(preset, null, 2);

  useEffect(() => () => { request.current++; }, []);
  useEffect(() => {
    if (mode && !dialog.current?.open) dialog.current?.showModal();
    if (!mode && dialog.current?.open) dialog.current.close();
  }, [mode]);

  async function open(next: TransferMode) {
    const ticket = ++request.current;
    setMode(next); setError(null); setPreview(null); setCopied(false); setSourceId("");
    setSections(groups.some((group) => group.id === props.sectionId) ? [props.sectionId] : groups.map((group) => group.id));
    if (next !== "copy") { setBusy(false); return; }
    setBusy(true);
    try {
      const all = await listInstancesFromStorage();
      if (request.current === ticket) setInstances(all.filter((item) => item.module_id === moduleId && item.id !== props.details.summary.id));
    } catch { if (request.current === ticket) setError(t("ark.transfer.loadFailed")); }
    finally { if (request.current === ticket) setBusy(false); }
  }

  function close() { request.current++; setMode(null); setBusy(false); }

  async function importFiles(files: File[]) {
    if (!files.length) return;
    const ticket = ++request.current;
    setBusy(true); setError(null); setPreview(null);
    try {
      if (files.length > 2 || files.some((file) => file.size > ARK_IMPORT_MAX_BYTES)) throw new Error("size");
      const documents = await Promise.all(files.map(async (file) => ({ name: file.name, text: await file.text() })));
      let next: Preview;
      if (documents.length === 1 && documents[0].name.toLowerCase().endsWith(".json")) {
        next = { patch: readArkPreset(documents[0].text, moduleId, properties), notes: [], issues: [] };
      } else {
        const imported = importArkIniDocuments(properties, documents);
        next = {
          patch: imported.patch,
          notes: [
            ...(imported.unknownCount ? [t("ark.transfer.unknown", { count: imported.unknownCount })] : []),
            ...(imported.skippedKeys.length ? [t("ark.transfer.networkKept", { keys: imported.skippedKeys.join(", ") })] : [])
          ],
          issues: imported.issues.map((issue) => t("ark.transfer.lineError", { file: issue.document, line: issue.line, key: issue.key }))
        };
      }
      if (request.current === ticket) setPreview(next);
    } catch (cause) {
      if (request.current === ticket) setError(t(cause instanceof Error && /size/i.test(cause.message) ? "ark.transfer.sizeError" : "ark.transfer.invalidFile"));
    } finally { if (request.current === ticket) setBusy(false); }
  }

  async function previewCopy() {
    if (!sourceId || !sections.length) return;
    const ticket = ++request.current;
    setBusy(true); setError(null); setPreview(null);
    try {
      const source = await readInstanceDetails(sourceId);
      if (source.summary.module_id !== moduleId) throw new Error("edition");
      const parsed: unknown = JSON.parse(source.settings_json);
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) throw new Error("settings");
      const sourcePreset = createArkPreset(moduleId, parsed as SettingsObject, fields, sections);
      if (request.current === ticket) setPreview({ patch: readArkPreset(JSON.stringify(sourcePreset), moduleId, properties), notes: [], issues: [] });
    } catch { if (request.current === ticket) setError(t("ark.transfer.loadFailed")); }
    finally { if (request.current === ticket) setBusy(false); }
  }

  function chooseSection(id: string, checked: boolean) {
    setSections((previous) => checked ? [...previous, id] : previous.filter((item) => item !== id));
    setPreview(null); setCopied(false);
  }

  function applyPreview() {
    if (!preview) return;
    const patch = { ...preview.patch };
    // Merge against the latest draft only when applying, preserving edits made during file reads.
    for (const key of ["game_ini_extra", "game_user_settings_extra"]) {
      const previous = props.settings[key]; const incoming = patch[key];
      if (typeof previous === "string" && previous.trim() && typeof incoming === "string") {
        patch[key] = previous.includes(incoming) ? previous : `${previous.trimEnd()}\n${incoming}`;
      }
    }
    props.onPatch(patch); close();
  }

  function download() {
    const url = URL.createObjectURL(new Blob([presetText], { type: "application/json;charset=utf-8" }));
    const link = document.createElement("a");
    link.href = url; link.download = `${moduleId}-preset.json`; link.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  return <div className="ark-configuration-toolbar">
    <div className="settings-editor-toolbar">
      {(["import", "copy", "export"] as const).map((item) => <button key={item} type="button" className="ghost-button"
        disabled={props.disabled} onClick={() => void open(item)}>{t(`ark.transfer.${item}`)}</button>)}
    </div>
    <dialog ref={dialog} className="ark-transfer-dialog" aria-labelledby="ark-transfer-title" onCancel={close} onClose={() => { if (mode) close(); }}>
      <header><h2 id="ark-transfer-title">{mode ? t(`ark.transfer.${mode}`) : ""}</h2>
        <button type="button" className="ghost-button" aria-label={t("ark.transfer.close")} onClick={close}>×</button></header>
      <div className="ark-transfer-body">
        {mode === "import" ? <label className="ark-transfer-file">
          <span>{t("ark.transfer.chooseFiles")}</span>
          <input type="file" accept=".ini,.json" multiple disabled={busy} onChange={(event) => {
            void importFiles(Array.from(event.target.files ?? [])); event.target.value = "";
          }} />
        </label> : null}
        {mode === "copy" ? <label className="ark-transfer-source">{t("ark.transfer.source")}
          <select className="settings-schema-input" value={sourceId} disabled={busy} onChange={(event) => { setSourceId(event.target.value); setPreview(null); }}>
            <option value="">{t(instances.length ? "ark.transfer.chooseSource" : "ark.transfer.noSource")}</option>
            {instances.map((instance) => <option key={instance.id} value={instance.id}>{instance.name}</option>)}
          </select>
        </label> : null}
        {mode === "copy" || mode === "export" ? <>
          <p className="form-note">{t("ark.transfer.portableHelp")}</p>
          <fieldset className="ark-transfer-sections"><legend>{t("ark.transfer.sections")}</legend>
            {groups.map((group) => <label key={group.id}><input type="checkbox" checked={sections.includes(group.id)} disabled={busy}
              onChange={(event) => chooseSection(group.id, event.target.checked)} />{group.title}</label>)}
          </fieldset>
        </> : null}
        {mode === "copy" ? <button type="button" className="secondary-button" disabled={busy || !sourceId || !sections.length}
          onClick={() => void previewCopy()}>{t("ark.transfer.preview")}</button> : null}
        {mode === "export" ? <textarea className="settings-schema-input ark-transfer-json" readOnly value={presetText} aria-label={t("ark.transfer.presetContent")} /> : null}
        {busy ? <ActivityNotice>{t("ark.transfer.loading")}</ActivityNotice> : null}
        {error ? <ActivityNotice tone="error">{error}</ActivityNotice> : null}
        {preview ? <section aria-label={t("ark.transfer.preview")}>
          <p>{t("ark.transfer.changeCount", { count: Object.keys(preview.patch).length })}</p>
          {preview.notes.map((note) => <p className="form-note" key={note}>{note}</p>)}
          {preview.issues.map((issue, index) => <p className="form-note error" role="alert" key={index}>{issue}</p>)}
          <dl className="ark-transfer-preview">{Object.entries(preview.patch).map(([key, value]) => <div key={key}>
            <dt>{fieldMap.get(key)?.title ?? key}</dt>
            <dd>{value === undefined ? t("ark.transfer.gameDefault") : /password|secret|token|_extra$/i.test(key) ? t("ark.transfer.privateValue") : String(value).slice(0, 120)}</dd>
          </div>)}</dl>
        </section> : null}
      </div>
      <footer>
        <button type="button" className="ghost-button" onClick={close}>{t("ark.transfer.close")}</button>
        {mode === "export" ? <>
          <button type="button" className="ghost-button" disabled={!Object.keys(preset.settings).length && !preset.reset.length} onClick={() => {
            if (!navigator.clipboard?.writeText) { setError(t("ark.transfer.copyFailed")); return; }
            const ticket = request.current;
            void navigator.clipboard.writeText(presetText).then(() => {
              if (ticket === request.current) setCopied(true);
            }, () => { if (ticket === request.current) setError(t("ark.transfer.copyFailed")); });
          }}>{t(copied ? "ark.transfer.copied" : "ark.transfer.copyText")}</button>
          <button type="button" className="secondary-button" disabled={!Object.keys(preset.settings).length && !preset.reset.length} onClick={download}>{t("ark.transfer.download")}</button>
        </> : <button type="button" className="secondary-button" disabled={props.disabled || busy || !preview || preview.issues.length > 0 || !Object.keys(preview.patch).length}
          onClick={applyPreview}>{t("ark.transfer.apply")}</button>}
      </footer>
    </dialog>
  </div>;
}
