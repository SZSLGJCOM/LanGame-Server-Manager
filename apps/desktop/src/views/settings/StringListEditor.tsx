import { useEffect, useMemo, useState } from "react";
import { useI18n } from "../../i18n";
import type { GuidedSettingsField } from "./settings-schema";
import { ConfigurationHelp } from "./ConfigurationFieldHelp";

interface StringListEditorProps {
  ariaDescribedBy?: string;
  ariaErrorMessage?: string;
  ariaInvalid?: boolean;
  field: GuidedSettingsField;
  inputId?: string;
  value: unknown;
  disabled?: boolean;
  onChange: (value: string) => void;
}

function parseStringList(value: unknown): string[] {
  if (typeof value !== "string" || value.trim().length === 0) {
    return [];
  }

  const seen = new Set<string>();
  const entries: string[] = [];

  for (const entry of value
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "\n")
    .split(/[\n;]+/)
    .map((item) => item.trim())
    .filter(Boolean)) {
    const normalizedKey = entry.toLocaleLowerCase();
    if (!seen.has(normalizedKey)) {
      seen.add(normalizedKey);
      entries.push(entry);
    }
  }

  return entries;
}

function serializeStringList(entries: string[]): string {
  return entries.join("\n");
}

export function StringListEditor(props: StringListEditorProps) {
  const { t } = useI18n();
  const [draft, setDraft] = useState("");
  const entries = useMemo(() => parseStringList(props.value), [props.value]);
  const pendingEntries = useMemo(() => parseStringList(draft), [draft]);
  const newEntries = useMemo(
    () => pendingEntries.filter((entry) => !entries.some((current) => current.toLocaleLowerCase() === entry.toLocaleLowerCase())),
    [entries, pendingEntries]
  );

  useEffect(() => {
    setDraft("");
  }, [props.field.key, props.value]);

  function commit(nextEntries: string[]) {
    props.onChange(serializeStringList(nextEntries));
  }

  function addDraftEntries() {
    if (newEntries.length === 0) {
      return;
    }

    commit([...entries, ...newEntries]);
    setDraft("");
  }

  function removeEntry(entry: string) {
    commit(entries.filter((current) => current !== entry));
  }

  function clearAll() {
    commit([]);
    setDraft("");
  }

  return (
    <div className="string-list-editor">
      {entries.length > 0 ? (
        <div className="string-list-chip-grid">
          {entries.map((entry) => (
            <span key={entry} className="string-list-chip">
              <span className="string-list-chip-value">{entry}</span>
              <ConfigurationHelp description={t("settings.guided.list.remove", { entry }, `Remove ${entry}`)}>{(help) => <button
                type="button"
                className="string-list-chip-remove"
                disabled={props.disabled}
                onClick={() => removeEntry(entry)}
                aria-label={t("settings.guided.list.remove", { entry }, `Remove ${entry}`)}
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
        className="settings-schema-input settings-schema-textarea string-list-editor-input"
        aria-describedby={props.ariaDescribedBy}
        aria-errormessage={props.ariaErrorMessage}
        aria-invalid={props.ariaInvalid}
        rows={3}
        value={draft}
        disabled={props.disabled}
        placeholder={t(
          "settings.guided.list.placeholder",
          undefined,
          "Paste one entry per line. Semicolon-separated lists also work."
        )}
        onChange={(event) => setDraft(event.target.value)}
      />

      <div className="settings-editor-toolbar string-list-editor-toolbar">
        <button
          type="button"
          className="secondary-button"
          disabled={props.disabled || newEntries.length === 0}
          onClick={addDraftEntries}
        >
          {t("settings.guided.list.add", undefined, "Add entries")}
        </button>
        <button
          type="button"
          className="ghost-button"
          disabled={props.disabled || entries.length === 0}
          onClick={clearAll}
        >
          {t("settings.guided.list.clear", undefined, "Clear all")}
        </button>
        {newEntries.length > 0 ? (
          <span className="page-chip">
            {t("settings.guided.list.pending", { count: newEntries.length }, `${newEntries.length} pending`)}
          </span>
        ) : null}
      </div>
    </div>
  );
}
