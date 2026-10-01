import { useState } from "react";
import { InlineConfirmAction } from "../../../components/InlineConfirmAction";
import { playerAccessObjectIdentityKeys } from "../../../domain/player-access";
import type { RuntimeLivePlayerEntry } from "../../../types";
import { resolveSelectedRosterTarget, type SelectedRosterTarget } from "./player-access-selected-target";
import { rosterFieldActionLabel } from "./player-access-roster-labels";
import { ShellIcon } from "../../../components/ShellIcon";
import { selectLocaleText, type LocaleCode } from "../../../i18n";
import {
  isRosterRecord,
  stringifyRosterEntry,
  type RosterEntry,
  type RosterField
} from "./player-access-roster-model";
import {
  buildObjectRosterDraft,
  defaultObjectRosterPropertyValue,
  normalizeObjectRosterDraft,
  readObjectRosterProperties,
  type ObjectRosterDraft
} from "./player-access-roster-validation";

export type RosterMutationHandler = (
  field: RosterField,
  operation: "add" | "remove",
  value: unknown
) => Promise<boolean>;

export interface PlayerAccessRosterEditorProps {
  field: RosterField;
  busy: boolean;
  disabled: boolean;
  locale: LocaleCode;
  onMutate: RosterMutationHandler;
  selectedEntry: RosterEntry | null;
  selectedPlayer?: RuntimeLivePlayerEntry | null;
  onCancel?: () => void;
  onSaved?: () => void;
}

function rosterConfirmation(field: RosterField, entry: string, operation: "add" | "remove", locale: LocaleCode): string {
  const targetText = entry.replace(/[\u0000-\u001f\u007f]/g, " ").trim();
  if (field.kind === "string-scalar") {
    return operation === "add"
      ? selectLocaleText(locale, `确认将“${field.title}”设置为 ${targetText}？`, `Set “${field.title}” to ${targetText}?`)
      : selectLocaleText(locale, `确认清除“${field.title}”？`, `Clear “${field.title}”?`);
  }
  return operation === "add"
    ? selectLocaleText(locale, `确认将 ${targetText} 添加到“${field.title}”？`, `Add ${targetText} to “${field.title}”?`)
    : selectLocaleText(locale, `确认将 ${targetText} 从“${field.title}”移除？`, `Remove ${targetText} from “${field.title}”?`);
}

function rosterConfirmationScope(field: RosterField, value: unknown, entry: RosterEntry | null = null): string {
  return JSON.stringify([field.key, field.currentValue, entry?.rawValue ?? null, value]);
}

interface ObjectRosterFieldEditorProps extends PlayerAccessRosterEditorProps {
  lockedIdentity?: SelectedRosterTarget;
  onCancel?: () => void;
  onSaved?: () => void;
  inputPrefix?: string;
}

export function ObjectRosterFieldEditor(props: ObjectRosterFieldEditorProps) {
  const properties = readObjectRosterProperties(props.field);
  const [draft, setDraft] = useState<ObjectRosterDraft>(() => buildObjectRosterDraft(props.field,
    props.selectedEntry?.rawValue ?? props.lockedIdentity?.rawValue));
  // Preserve refreshed read-only metadata while keeping the user's editable draft intact.
  const editableDraft = props.selectedEntry && isRosterRecord(props.selectedEntry.rawValue)
    ? { ...props.selectedEntry.rawValue, ...Object.fromEntries(properties.map((property) => [property.key, draft[property.key]])) }
    : draft;
  const identityValues = props.lockedIdentity && isRosterRecord(props.lockedIdentity.rawValue)
    ? props.lockedIdentity.rawValue : null;
  const normalizedDraft = normalizeObjectRosterDraft(props.field, { ...editableDraft, ...identityValues });
  const selectedTarget = resolveSelectedRosterTarget(props.selectedPlayer ?? null, props.field);
  const alreadyListed = selectedTarget !== null && props.field.entries.some((entry) => entry.key === selectedTarget.identity);
  const inputPrefix = props.inputPrefix ?? `player-access-roster-${props.field.key.replace(/[^a-z0-9_-]/gi, "-")}`;
  const disabled = props.disabled || props.busy;

  function fillSelectedIdentity() {
    const value = selectedTarget?.rawValue;
    if (!value || typeof value === "string" || props.selectedEntry || alreadyListed || disabled) return;
    const identityKeys = playerAccessObjectIdentityKeys(props.field.property);
    // Filling is explicit and leaves permission levels and other hand-edited metadata intact.
    setDraft((current) => ({ ...current, ...Object.fromEntries(identityKeys.map((key) => [key, value[key]])) }));
  }

  async function saveDraft() {
    if (disabled) return;
    if (await props.onMutate(props.field, "add", normalizedDraft)) {
      if (props.onSaved) { props.onSaved(); return; }
      setDraft(props.selectedEntry ? normalizedDraft : buildObjectRosterDraft(props.field));
    }
  }

  const confirmation = props.selectedEntry
    ? selectLocaleText(props.locale, `确认保存“${props.field.title}”中 ${props.selectedEntry.label} 的修改？`,
      `Save changes to ${props.selectedEntry.label} in “${props.field.title}”?`)
    : rosterConfirmation(props.field, stringifyRosterEntry(normalizedDraft) || props.field.title, "add", props.locale);

  return (
    <div className="player-access-object-roster-field">
      {!props.lockedIdentity && !props.selectedEntry && props.selectedPlayer && selectedTarget ? <div className="player-access-roster-selected">
        <button type="button" className="secondary-button player-access-roster-selected-fill"
          disabled={disabled || alreadyListed} title={selectedTarget.identity} onClick={fillSelectedIdentity}>
          {alreadyListed ? selectLocaleText(props.locale, "已在当前名单", "Already in this roster")
            : selectLocaleText(props.locale, `填入 ${props.selectedPlayer.display_name} 的身份`, `Fill identity from ${props.selectedPlayer.display_name}`)}
        </button>
        <span className="form-note">{selectLocaleText(props.locale,
          "核对下方完整条目后保存。", "Review the full entry below before saving.")}</span>
      </div> : null}
      <form className="player-access-roster-editor player-access-object-roster-editor" onSubmit={(event) => event.preventDefault()}>
        <div className="player-access-object-roster-grid">
          {properties.map((property) => {
            const inputId = `${inputPrefix}-${property.key.replace(/[^a-z0-9_-]/gi, "-")}`;
            const rawValue = identityValues?.[property.key] ?? draft[property.key] ?? defaultObjectRosterPropertyValue(property);
            const identityLocked = Boolean((props.selectedEntry || props.lockedIdentity) && property.identity);
            return (
              <label key={property.key} htmlFor={inputId}
                className={`form-field player-access-object-roster-property${property.kind === "boolean" ? " is-boolean" : ""}`}>
                <span className="detail-label">{property.title}{property.required ? " *" : ""}</span>
                {property.kind === "boolean" ? (
                  <input id={inputId} type="checkbox" checked={rawValue === true}
                    onChange={(event) => setDraft((current) => ({ ...current, [property.key]: event.target.checked }))}
                    disabled={disabled || identityLocked} />
                ) : property.enumValues.length > 0 ? (
                  <select id={inputId} className="text-input" value={String(rawValue)}
                    onChange={(event) => {
                      const selected = property.enumValues.find((value) => String(value) === event.target.value);
                      setDraft((current) => ({ ...current, [property.key]: selected ?? event.target.value }));
                    }} disabled={disabled || identityLocked}>
                    {property.enumValues.map((value) => <option key={String(value)} value={String(value)}>{String(value)}</option>)}
                  </select>
                ) : (
                  <input id={inputId} type={property.kind === "integer" ? "number" : property.property.format === "date" ? "date" : "text"}
                    step={property.kind === "integer" ? 1 : undefined} className="text-input"
                    value={typeof rawValue === "string" || typeof rawValue === "number" ? rawValue : ""}
                    onChange={(event) => setDraft((current) => ({ ...current, [property.key]: event.target.value }))}
                    disabled={disabled || identityLocked} />
                )}
              </label>
            );
          })}
        </div>
        <div className="player-access-roster-action-pair">
          <InlineConfirmAction type="submit" className={props.field.lane === "block" ? "ghost-button danger" : "secondary-button"}
            disabled={disabled || properties.length === 0}
            scopeKey={rosterConfirmationScope(props.field, normalizedDraft, props.selectedEntry)}
            confirmation={confirmation} onConfirm={saveDraft}>
            {props.busy ? selectLocaleText(props.locale, "保存中", "Saving")
              : props.selectedEntry ? selectLocaleText(props.locale, "保存修改", "Save changes")
                : rosterFieldActionLabel(props.field, props.locale)}
          </InlineConfirmAction>
          <button type="button" className="ghost-button" disabled={disabled}
            onClick={() => props.onCancel ? props.onCancel() : setDraft(buildObjectRosterDraft(props.field, props.selectedEntry?.rawValue))}>
            {props.onCancel ? selectLocaleText(props.locale, "取消", "Cancel") : props.selectedEntry ? selectLocaleText(props.locale, "重置修改", "Reset changes")
              : selectLocaleText(props.locale, "清空", "Clear")}
          </button>
        </div>
      </form>
    </div>
  );
}

function SimpleRosterFieldEditor(props: PlayerAccessRosterEditorProps) {
  const [draft, setDraft] = useState("");
  const inputId = `player-access-roster-${props.field.key.replace(/[^a-z0-9_-]/gi, "-")}`;
  const disabled = props.disabled || props.busy;

  async function addDraft() {
    const value = draft.trim();
    if (!disabled && value && await props.onMutate(props.field, "add", value)) {
      setDraft("");
      props.onSaved?.();
    }
  }

  return <form className="player-access-roster-editor" onSubmit={(event) => event.preventDefault()}>
    <label htmlFor={inputId} className="sr-only">{props.field.title}</label>
    <input id={inputId} type="text" className="text-input player-access-roster-input" value={draft}
      onChange={(event) => setDraft(event.target.value)}
      placeholder={selectLocaleText(props.locale, `输入${props.field.title}`, `Enter ${props.field.title}`)} disabled={disabled} />
    <InlineConfirmAction type="submit"
      className={`player-access-roster-add-button ${props.field.lane === "block" ? "ghost-button danger" : "secondary-button"}`}
      disabled={disabled || !draft.trim()} aria-label={`${rosterFieldActionLabel(props.field, props.locale)} · ${props.field.title}`}
      scopeKey={rosterConfirmationScope(props.field, draft.trim())}
      confirmation={rosterConfirmation(props.field, draft, "add", props.locale)} onConfirm={addDraft}>
      <ShellIcon name="plus" className="player-access-roster-add-icon" />
      {props.busy ? selectLocaleText(props.locale, "保存中", "Saving") : rosterFieldActionLabel(props.field, props.locale)}
    </InlineConfirmAction>
    {props.onCancel ? <button type="button" className="ghost-button" disabled={disabled} onClick={props.onCancel}>
      {selectLocaleText(props.locale, "取消", "Cancel")}
    </button> : null}
  </form>;
}

export function PlayerAccessRosterEditor(props: PlayerAccessRosterEditorProps) {
  return props.field.kind === "object-list" ? <ObjectRosterFieldEditor {...props} />
    : <SimpleRosterFieldEditor {...props} />;
}
