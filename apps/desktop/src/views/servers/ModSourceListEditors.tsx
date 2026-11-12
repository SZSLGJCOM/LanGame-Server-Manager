import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { describeError } from "../../app-state";
import { ShellIcon } from "../../components/ShellIcon";
import { useI18n, type TranslateFn } from "../../i18n";
import type { ModuleDetails } from "../../types";
import { ConfigurationField } from "../settings/ConfigurationField";
import { buildConfigurationFieldCopy } from "../settings/GuidedSettingsForm";
import { parseGuidedSettingsSchema, readGuidedFieldValue, validateGuidedSettingsObject, writeGuidedFieldValue } from "../settings/guided-settings";
import { useInstanceSettingsSaveCoordinator } from "../settings/InstanceSettingsSaveContext";
import { MOD_SOURCE_LIST_RENDERER, resolveSettingsModuleDefinition } from "../settings/module-registry";
import { AbioticOfficialModSource } from "../settings/modules/AbioticOfficialModSource";
import { validateAbioticModField } from "../settings/modules/abioticfactor-mods";
import type { GuidedSettingsField, SettingsObject } from "../settings/settings-schema";
import { parseAsaModManifest } from "./asa-mod-manifest";
import { changedSettingKeys } from "./mod-settings-patch";
import { ASA_MEMBERSHIP_KEYS, buildAsaLoadListPlan, hasAsaRawModFlags } from "./mod-workbench-asa";
import "./workshop-collection-removal-dialog.css";

interface ModSourceListEditorsProps {
  instanceId: string;
  moduleDetails?: ModuleDetails | null;
  settings: SettingsObject;
  disabled: boolean;
  readOnly: boolean;
  onSave: (next: SettingsObject, expected: SettingsObject, validateCurrent: (latest: SettingsObject) => void) => Promise<unknown>;
}

interface SourceFieldProps extends ModSourceListEditorsProps {
  moduleId: string;
  field: GuidedSettingsField;
  onPendingChange: (key: string, pending: boolean) => void;
}

function validateSourceField(moduleId: string, field: GuidedSettingsField, settings: SettingsObject,
  locale: string, t: TranslateFn): string | undefined {
  const definition = resolveSettingsModuleDefinition(moduleId);
  const value = readGuidedFieldValue(field, settings);
  return validateGuidedSettingsObject({ title: field.title, sections: [], fields: [field] }, settings, undefined, t)[0]?.message
    ?? definition?.getFieldValidationMessage?.({ field, value, settings, locale, t })
    ?? definition?.getSettingsValidationIssues?.(settings, { locale, t }).find((issue) => issue.fieldKey === field.key)?.message
    ?? (moduleId === "abioticfactor" && (field.key === "mod_ids" || field.key === "sideload_mods")
      ? validateAbioticModField("mods_directory", settings.mods_directory, settings, t) : undefined);
}

function ModSourceField({ field, moduleId, onPendingChange, ...props }: SourceFieldProps) {
  const { locale, t } = useI18n();
  const coordinator = useInstanceSettingsSaveCoordinator();
  const [draft, setDraft] = useState<{ base: SettingsObject; value: unknown } | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const inFlight = useRef<Promise<void> | null>(null);
  const mounted = useRef(true);
  const rawFile = moduleId === "dontstarve";
  const passive = moduleId === "arksurvivalascended";
  const base = draft?.base ?? props.settings;
  const current = draft ? writeGuidedFieldValue(base, field, draft.value) : props.settings;
  const dirty = draft !== null && changedSettingKeys(base, current, [field.key]).length > 0;
  const pending = useRef(false);
  pending.current = dirty;
  const definition = resolveSettingsModuleDefinition(moduleId);
  const rawFlags = passive && hasAsaRawModFlags(props.settings);
  const disabled = props.disabled || props.readOnly || saving || rawFlags || definition?.isFieldDisabled?.(field, props.settings);
  const nativeReferences = useMemo(() => passive && typeof draft?.value === "string"
    ? parseAsaModManifest(draft.value, true) : null, [passive, draft]);
  const validation = nativeReferences?.tooLarge ? t("servers.mods.manifest.tooLarge")
    : nativeReferences?.invalidTokens.length ? t("servers.mods.asaManifest.invalid", {
      values: nativeReferences.invalidTokens.slice(0, 8).map((entry) => entry.slice(0, 80)).join("、")
    }) : validateSourceField(moduleId, field, current, locale, t);

  useLayoutEffect(() => {
    onPendingChange(field.key, dirty || saving);
    return () => onPendingChange(field.key, false);
  }, [field.key, dirty, saving, onPendingChange]);

  // A full-file replacement requires the operator's confirmation, including when another control requests a start.
  useLayoutEffect(() => {
    mounted.current = true;
    const registration = coordinator.register(props.instanceId, async () => {
      if (inFlight.current) await inFlight.current;
      else if (pending.current) throw new Error(t("settings.configuration.save.unsaved"));
    });
    return () => {
      mounted.current = false;
      registration.detach(inFlight.current ?? Promise.resolve());
    };
  }, [coordinator, props.instanceId, t]);

  function edit(patch: SettingsObject) {
    if (disabled || inFlight.current) return;
    setDraft({ base, value: patch[field.key] });
    setConfirming(false);
    setError(null);
  }

  function save() {
    if (!draft || !dirty || disabled || validation || inFlight.current) return;
    if (rawFile && !confirming) { setConfirming(true); return; }
    const validateCurrent = (latest: SettingsObject) => {
      const keys = passive ? ASA_MEMBERSHIP_KEYS : [field.key];
      if (changedSettingKeys(base, latest, keys).length || definition?.isFieldDisabled?.(field, latest)) {
        throw new Error(t("servers.mods.membershipChanged"));
      }
      const issue = validateSourceField(moduleId, field, writeGuidedFieldValue(latest, field, draft.value), locale, t);
      if (issue) throw new Error(issue);
    };
    setSaving(true); setError(null);
    const operation = Promise.resolve().then(async () => {
      validateCurrent(props.settings);
      const next = nativeReferences ? buildAsaLoadListPlan(base, nativeReferences.ids, "passive") : current;
      await props.onSave(next, base, validateCurrent);
      pending.current = false;
      if (mounted.current) { setDraft(null); setConfirming(false); }
    }).catch((cause: unknown) => {
      if (mounted.current) setError(describeError(cause));
      throw cause;
    }).finally(() => {
      inFlight.current = null;
      if (mounted.current) setSaving(false);
    });
    inFlight.current = operation;
    void operation.catch(() => undefined); // The field reports the error, and a waiting start receives the rejected operation.
  }

  return <div className="mw-source-field" aria-busy={saving}>
    <ConfigurationField copy={buildConfigurationFieldCopy(t)}
      field={{ ...field, presentation: { ...field.presentation, state: "editable" } }}
      idPrefix={`mod-source-${props.instanceId}`} settings={current}
      value={props.readOnly ? props.settings[field.key] : readGuidedFieldValue(field, current)}
      readOnly={props.readOnly} disabled={disabled} onPatch={edit} t={t} validationMessage={props.readOnly ? undefined : validation} />
    {rawFlags ? <p className="form-note">{t("servers.mods.asaRawFlagsWarning")}</p> : null}
    {error ? <p className="mw-notice mw-notice--error" role="alert">{error}</p> : null}
    {dirty && !props.readOnly ? <div className="mw-manifest-toolbar">
      {confirming ? <span role="status">{t("servers.mods.sourceLists.confirmRaw")}</span> : null}
      <button type="button" className="mw-btn mw-btn--primary" disabled={disabled || Boolean(validation)}
        onClick={save}>{t(confirming ? "common.confirm" : "common.save")}</button>
      <button type="button" className="mw-ghost-btn" disabled={saving} onClick={() => {
        setDraft(null); setError(null); setConfirming(false);
      }}>{t("common.cancel")}</button>
    </div> : null}
  </div>;
}

/** Source-specific settings share one secondary entry, leaving the Mod list unobstructed. */
export function ModSourceListEditors(props: ModSourceListEditorsProps) {
  const { locale, t } = useI18n();
  const titleId = useId();
  const dialog = useRef<HTMLDialogElement>(null);
  const closeButton = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const [pendingFields, setPendingFields] = useState<ReadonlySet<string>>(() => new Set());
  const [closeBlocked, setCloseBlocked] = useState(false);
  const onPendingChange = useCallback((key: string, pending: boolean) => {
    setPendingFields((current) => {
      if (current.has(key) === pending) return current;
      const next = new Set(current);
      if (pending) next.add(key); else next.delete(key);
      return next;
    });
  }, []);
  const moduleId = props.moduleDetails?.summary.id;
  const fields = useMemo(() => {
    if (!resolveSettingsModuleDefinition(moduleId)?.specializedRenderers?.[MOD_SOURCE_LIST_RENDERER]) return [];
    return parseGuidedSettingsSchema(props.moduleDetails, locale, t, { surface: "mods" }).fields
      .filter((field) => field.presentation.rendererId === MOD_SOURCE_LIST_RENDERER);
  }, [moduleId, props.moduleDetails, locale, t]);

  useEffect(() => {
    const node = dialog.current;
    if (!node || !open) return;
    const previous = document.activeElement;
    node.showModal();
    closeButton.current?.focus();
    return () => {
      node.close();
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, [open]);

  useEffect(() => {
    if (!open || !pendingFields.size) return;
    const preserveDraft = (event: KeyboardEvent) => {
      if (event.key === "Escape" && dialog.current?.open) {
        event.preventDefault();
        setCloseBlocked(true);
      }
    };
    document.addEventListener("keydown", preserveDraft, true);
    return () => document.removeEventListener("keydown", preserveDraft, true);
  }, [open, pendingFields.size]);

  function close() {
    if (pendingFields.size) { setCloseBlocked(true); return; }
    setCloseBlocked(false);
    setOpen(false);
  }

  if (!moduleId || !fields.length) return null;
  return <>
    <button type="button" className="mw-ghost-btn mw-source-settings-button" aria-haspopup="dialog"
      onClick={() => setOpen(true)}>
      <ShellIcon name="settings" className="mw-btn-icon" />{t("servers.mods.sourceLists.open")}
    </button>
    <dialog ref={dialog} className="mw-collection-removal-dialog mw-source-settings-dialog"
      aria-labelledby={titleId} data-no-window-drag="true"
      onCancel={(event) => { event.preventDefault(); close(); }}>
      <header className="mw-collection-removal-header"><h2 id={titleId}>{t("servers.mods.sourceLists.title")}</h2></header>
      <div className="mw-collection-removal-body">
        {moduleId === "abioticfactor" ? <AbioticOfficialModSource /> : null}
        {fields.map((field) => <ModSourceField key={field.key} {...props} moduleId={moduleId} field={field}
          onPendingChange={onPendingChange} />)}
      </div>
      <footer className="mw-collection-removal-footer">
        {closeBlocked && pendingFields.size > 0 ? <p role="alert" className="mw-source-settings-unsaved">
          {t("servers.mods.sourceLists.unsaved")}
        </p> : null}
        <div className="mw-collection-removal-actions">
          <button ref={closeButton} type="button" className="mw-ghost-btn mw-source-settings-close" onClick={close}>{t("common.closeWindow")}</button>
        </div>
      </footer>
    </dialog>
  </>;
}
