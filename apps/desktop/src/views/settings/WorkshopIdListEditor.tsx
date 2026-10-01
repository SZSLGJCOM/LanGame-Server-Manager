import { useEffect, useMemo, useState } from "react";
import { useI18n } from "../../i18n";
import {
  parseWorkshopIdList,
  serializeWorkshopIdList
} from "./guided-setting-values";
import type { GuidedSettingsField } from "./settings-schema";
import { ConfigurationHelp } from "./ConfigurationFieldHelp";

interface WorkshopIdListEditorProps {
  ariaDescribedBy?: string;
  ariaErrorMessage?: string;
  ariaInvalid?: boolean;
  field: GuidedSettingsField;
  inputId?: string;
  value: unknown;
  disabled?: boolean;
  onChange: (value: string) => void;
}

export function WorkshopIdListEditor(props: WorkshopIdListEditorProps) {
  const { t } = useI18n();
  const [draft, setDraft] = useState("");
  const ids = useMemo(() => parseWorkshopIdList(props.value), [props.value]);
  const pendingIds = useMemo(() => parseWorkshopIdList(draft), [draft]);
  const newIds = useMemo(() => pendingIds.filter((id) => !ids.includes(id)), [ids, pendingIds]);

  useEffect(() => {
    setDraft("");
  }, [props.field.key, props.value]);

  function commit(nextIds: string[]) {
    props.onChange(serializeWorkshopIdList(nextIds));
  }

  function addDraftIds() {
    if (newIds.length === 0) {
      return;
    }

    commit([...ids, ...newIds]);
    setDraft("");
  }

  function removeId(id: string) {
    commit(ids.filter((current) => current !== id));
  }

  function clearAll() {
    commit([]);
    setDraft("");
  }

  return (
    <div className="workshop-list-editor">
      {ids.length > 0 ? (
        <div className="workshop-id-chip-grid">
          {ids.map((id) => (
            <span key={id} className="workshop-id-chip">
              <span className="workshop-id-chip-value">{id}</span>
              <ConfigurationHelp description={t("settings.guided.workshop.remove", { id })}>{(help) => <button
                type="button"
                className="workshop-id-chip-remove"
                disabled={props.disabled}
                onClick={() => removeId(id)}
                aria-label={t("settings.guided.workshop.remove", { id })}
                ref={help.anchorRef} {...help.interactionProps} aria-describedby={help.descriptionId}
              >
                <span aria-hidden="true">{String.fromCharCode(215)}</span>
              </button>}</ConfigurationHelp>
            </span>
          ))}
        </div>
      ) : null}

      <textarea
        id={props.inputId}
        className="settings-schema-input settings-schema-textarea workshop-list-editor-input"
        aria-describedby={props.ariaDescribedBy}
        aria-errormessage={props.ariaErrorMessage}
        aria-invalid={props.ariaInvalid}
        rows={3}
        value={draft}
        disabled={props.disabled}
        placeholder={t("settings.guided.workshop.placeholder")}
        onChange={(event) => setDraft(event.target.value)}
      />

      <div className="settings-editor-toolbar workshop-list-editor-toolbar">
        <button
          type="button"
          className="secondary-button"
          disabled={props.disabled || newIds.length === 0}
          onClick={addDraftIds}
        >
          {t("settings.guided.workshop.add")}
        </button>
        <button
          type="button"
          className="ghost-button"
          disabled={props.disabled || ids.length === 0}
          onClick={clearAll}
        >
          {t("settings.guided.workshop.clear")}
        </button>
        {newIds.length > 0 ? (
          <span className="page-chip">{t("settings.guided.workshop.pending", { count: newIds.length })}</span>
        ) : null}
      </div>
    </div>
  );
}
