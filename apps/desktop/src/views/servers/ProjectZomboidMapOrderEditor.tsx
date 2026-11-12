import { ActivityNotice, ActivityNoticeTarget } from "../../components/ActivityNotice";
import { useEffect, useId, useRef, useState } from "react";
import { describeError } from "../../app-state";
import { useI18n, type TranslateFn } from "../../i18n";
import { projectZomboidSettingsDefinition } from "../settings/modules/projectzomboid";
import { ConfigurationHelp } from "../settings/ConfigurationFieldHelp";
import type { GuidedSettingsField } from "../settings/settings-schema";
import { parseLineOrSemicolonEntries } from "./mod-workbench-model";
import "./projectzomboid-map-order.css";
import "./workshop-collection-removal-dialog.css";

const VANILLA_MAP = "Muldraugh, KY";
const MAP_FIELD: GuidedSettingsField = {
  key: "map_name", title: "Map load order", sectionId: "mods", type: "string", control: "textarea", required: true,
  presentation: { state: "specialized", owner: "mods", sectionId: "mods", rendererId: "mod-workbench-map-order" }
};

export function validateProjectZomboidMapOrder(value: string, locale: string, t: TranslateFn): string | undefined {
  return projectZomboidSettingsDefinition.getFieldValidationMessage?.({
    field: MAP_FIELD, value, settings: { map_name: value }, locale, t
  });
}

interface ProjectZomboidMapOrderEditorProps {
  open: boolean;
  onClose: () => void;
  value: unknown;
  disabled: boolean;
  readOnly?: boolean;
  hidden?: boolean;
  onSave: (value: string, expectedValue: string | undefined) => Promise<void>;
  onReload: () => Promise<string | undefined>;
}

interface MapOrderDraft {
  baseValue: string | undefined;
  entries: string[];
  savedAgainst?: string | undefined;
  saved: boolean;
}

function entriesFromValue(value: string | undefined): string[] {
  return parseLineOrSemicolonEntries(value ?? VANILLA_MAP);
}

export function ProjectZomboidMapOrderEditor(props: ProjectZomboidMapOrderEditorProps) {
  const { locale, t } = useI18n();
  const id = useId();
  const dialog = useRef<HTMLDialogElement>(null);
  const closeButton = useRef<HTMLButtonElement>(null);
  const value = typeof props.value === "string" ? props.value : undefined;
  const latestValue = useRef(value);
  latestValue.current = value;
  const active = useRef(true);
  const savingRef = useRef(false);
  const [draft, setDraft] = useState<MapOrderDraft | null>(null);
  const [pending, setPending] = useState("");
  const [saving, setSaving] = useState(false);
  const [reloading, setReloading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [closeBlocked, setCloseBlocked] = useState(false);
  const entries = draft?.entries ?? (props.readOnly && value === undefined ? [] : entriesFromValue(value));
  const nextEntries = parseLineOrSemicolonEntries([...entries, pending].join("\n"));
  const nextValue = nextEntries.join("\n");
  const baseValue = draft ? draft.baseValue : value;
  const dirty = (draft !== null || pending.length > 0) && nextValue !== entriesFromValue(baseValue).join("\n");
  const validation = validateProjectZomboidMapOrder(nextValue, locale, t);
  const disabled = props.readOnly || props.disabled || saving || reloading;
  const canAddVanilla = !nextEntries.some((entry) => entry.toLowerCase() === VANILLA_MAP.toLowerCase());
  const pendingChanges = dirty || pending.length > 0 || saving || reloading;

  useEffect(() => {
    const node = dialog.current;
    if (!node || !props.open || props.hidden) return;
    const previous = document.activeElement;
    node.showModal();
    closeButton.current?.focus();
    return () => {
      node.close();
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, [props.open, props.hidden]);

  useEffect(() => {
    if (!props.open || props.hidden || !pendingChanges) return;
    const preserveDraft = (event: KeyboardEvent) => {
      if (event.key === "Escape" && dialog.current?.open) {
        event.preventDefault();
        setCloseBlocked(true);
      }
    };
    document.addEventListener("keydown", preserveDraft, true);
    return () => document.removeEventListener("keydown", preserveDraft, true);
  }, [props.open, props.hidden, pendingChanges]);

  function close() {
    if (pendingChanges || savingRef.current) { setCloseBlocked(true); return; }
    setCloseBlocked(false);
    props.onClose();
  }

  useEffect(() => {
    active.current = true;
    return () => { active.current = false; };
  }, []);

  useEffect(() => {
    // A successful write may be observed before its updated parent props arrive.
    if (draft?.saved && (value === draft.baseValue || value !== draft.savedAgainst)) setDraft(null);
  }, [draft, value]);

  function edit(next: string[]) {
    if (disabled || savingRef.current) return;
    setDraft({ baseValue, entries: next, saved: false });
    setError(null);
    setSaved(false);
  }

  function move(index: number, offset: number) {
    const target = index + offset;
    if (target < 0 || target >= entries.length) return;
    const next = [...entries];
    [next[index], next[target]] = [next[target], next[index]];
    edit(next);
  }

  async function save() {
    if (disabled || savingRef.current || !dirty || validation) return;
    savingRef.current = true;
    setSaving(true);
    setError(null);
    try {
      await props.onSave(nextValue, baseValue);
      if (!active.current) return;
      setDraft({ baseValue: nextValue, entries: nextEntries, savedAgainst: latestValue.current, saved: true });
      setPending("");
      setSaved(true);
    } catch (cause) {
      if (active.current) setError(describeError(cause));
    } finally {
      savingRef.current = false;
      if (active.current) setSaving(false);
    }
  }

  async function reload() {
    if (disabled || savingRef.current) return;
    savingRef.current = true;
    setReloading(true);
    setError(null);
    try {
      const latest = await props.onReload();
      if (!active.current) return;
      setDraft({ baseValue: latest, entries: entriesFromValue(latest), saved: false });
      setPending("");
      setSaved(false);
    } catch (cause) {
      if (active.current) setError(describeError(cause));
    } finally {
      savingRef.current = false;
      if (active.current) setReloading(false);
    }
  }

  return <dialog ref={dialog} className="mw-collection-removal-dialog pz-map-order-dialog" hidden={props.hidden}
    aria-labelledby={`${id}-title`} data-field-key="map_name" aria-busy={saving || reloading} data-no-window-drag="true"
    onCancel={(event) => { event.preventDefault(); close(); }}>
    <header className="mw-collection-removal-header">
      <h2 id={`${id}-title`}>{t("projectzomboid.mods.maps.title", undefined, "Map load order")}</h2>
    </header>
    <div className="mw-collection-removal-body pz-map-order-editor">
    <div className="pz-map-order-content">
      <ol className="pz-map-order-list" aria-label={t("projectzomboid.mods.maps.title", undefined, "Map load order")}>
        {entries.map((entry, index) => <li key={entry}>
          <div className="pz-map-order-row">
            <span>{entry}</span>
            <div className="pz-map-order-row-actions">
              <button type="button" className="ghost-button" disabled={disabled || index === 0}
                aria-label={t("projectzomboid.mods.maps.moveUp", { name: entry }, "Move {name} up")} onClick={() => move(index, -1)}>↑</button>
              <button type="button" className="ghost-button" disabled={disabled || index === entries.length - 1}
                aria-label={t("projectzomboid.mods.maps.moveDown", { name: entry }, "Move {name} down")} onClick={() => move(index, 1)}>↓</button>
              <button type="button" className="ghost-button" disabled={disabled}
                aria-label={t("projectzomboid.mods.maps.remove", { name: entry }, "Remove {name}")}
                onClick={() => edit(entries.filter((_, itemIndex) => itemIndex !== index))}>×</button>
            </div>
          </div>
        </li>)}
      </ol>
      <div className="pz-map-order-add">
        <label htmlFor={`${id}-names`}>{t("projectzomboid.mods.maps.addLabel", undefined, "Add map names")}</label>
        <ConfigurationHelp description={t("projectzomboid.mods.maps.description", undefined,
          "Maps load from top to bottom. Enter map folder names, including Muldraugh, KY for the vanilla map.")}>{(help) =>
        <textarea id={`${id}-names`} className="settings-schema-input settings-schema-textarea" rows={2}
          value={pending} disabled={disabled} ref={help.anchorRef} {...help.interactionProps}
          aria-describedby={[help.descriptionId, validation ? `${id}-validation` : undefined].filter(Boolean).join(" ") || undefined}
          aria-invalid={Boolean(validation)} placeholder={t("projectzomboid.mods.maps.placeholder", undefined, "One map name per line")}
          onChange={(event) => {
            if (disabled || savingRef.current) return;
            edit(entries);
            setPending(event.target.value);
          }} />}
        </ConfigurationHelp>
        <div className="settings-editor-toolbar">
          <button type="button" className="secondary-button" disabled={disabled || nextEntries.length === entries.length}
            onClick={() => { edit(nextEntries); setPending(""); }}>{t("projectzomboid.mods.maps.add", undefined, "Add maps")}</button>
          <button type="button" className="ghost-button" disabled={disabled || !canAddVanilla}
            onClick={() => { edit(parseLineOrSemicolonEntries([...nextEntries, VANILLA_MAP].join("\n"))); setPending(""); }}>
            {t("projectzomboid.mods.maps.addVanilla", undefined, "Add vanilla map")}</button>
        </div>
      </div>
    </div>
    </div>
    <footer className="mw-collection-removal-footer pz-map-order-footer">
      {props.readOnly && value === undefined ? <p className="form-note" role="status">{t("servers.archives.configuration.notSaved")}</p> : null}
      {!props.readOnly && validation ? <p className="form-note error" id={`${id}-validation`} role="alert">{validation}</p> : null}
      {error ? <ActivityNoticeTarget.Provider value={undefined}>
        <ActivityNotice tone="error">{error}</ActivityNotice>
      </ActivityNoticeTarget.Provider> : null}
      {closeBlocked && pendingChanges ? <p role="alert" className="mw-collection-removal-error">{t("servers.mods.sourceLists.unsaved")}</p> : null}
      <div className="settings-editor-toolbar">
        <button type="button" className="secondary-button" disabled={disabled || !dirty || Boolean(validation)} onClick={() => void save()}>
          {saving ? t("projectzomboid.mods.maps.saving", undefined, "Saving map order…") : t("projectzomboid.mods.maps.save", undefined, "Save map order")}</button>
        <button type="button" className="ghost-button" disabled={disabled || (!draft && !pending)}
          onClick={() => void reload()}>
          {reloading ? t("projectzomboid.mods.maps.reloading", undefined, "Reloading map order…")
            : t("projectzomboid.mods.maps.reset", undefined, "Reload saved order")}</button>
        {dirty || pending ? <button type="button" className="ghost-button" disabled={saving || reloading} onClick={() => {
          if (savingRef.current) return;
          setDraft(null); setPending(""); setError(null); setSaved(false); setCloseBlocked(false);
        }}>{t("common.cancel")}</button> : null}
        <button ref={closeButton} type="button" className="ghost-button pz-map-order-close" onClick={close}>{t("common.closeWindow")}</button>
        <ActivityNoticeTarget.Provider value={undefined}>
          <ActivityNotice tone={saved && !dirty ? "success" : "info"}>{saved && !dirty
            ? t("projectzomboid.mods.maps.saved", undefined, "Map order saved")
            : dirty ? t("projectzomboid.mods.maps.unsaved", undefined, "Unsaved changes") : ""}</ActivityNotice>
        </ActivityNoticeTarget.Provider>
      </div>
    </footer>
  </dialog>;
}
