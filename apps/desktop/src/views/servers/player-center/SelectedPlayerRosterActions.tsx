import { useState } from "react";
import { InlineConfirmAction } from "../../../components/InlineConfirmAction";
import { selectLocaleText, type LocaleCode } from "../../../i18n";
import type { RuntimeLivePlayerEntry } from "../../../types";
import { ObjectRosterFieldEditor, PlayerAccessRosterEditor, type RosterMutationHandler } from "./PlayerAccessRosterEditor";
import { rosterFieldActionLabel, rosterFieldRemoveLabel } from "./player-access-roster-labels";
import type { RosterEntry, RosterField } from "./player-access-roster-model";
import type { SelectedRosterTarget } from "./player-access-selected-target";
import { resolveRosterActionTarget, type SelectedRosterEntry } from "./player-center-selection";

interface SelectedPlayerRosterActionsProps {
  selectedPlayer: RuntimeLivePlayerEntry | null;
  selectedRoster?: SelectedRosterEntry | null;
  fields: RosterField[];
  suppressedAddFieldKeys?: readonly string[];
  createFieldKey?: string;
  disabled: boolean;
  busyFieldKey: string | null;
  locale: LocaleCode;
  onMutate: RosterMutationHandler;
  onClearSelection?: () => void;
}
interface EditorTarget { fieldKey: string; entry: RosterEntry | null; identity?: SelectedRosterTarget }

function actionLabel(field: RosterField, operation: "add" | "remove", locale: LocaleCode): string {
  if (/(?:^|_)groups?(?:_|$)/i.test(field.key)) {
    const label = field.lane === "admin" ? selectLocaleText(locale, "管理员组", "admin group")
      : selectLocaleText(locale, "白名单组", "allowlist group");
    return operation === "add" ? selectLocaleText(locale, `添加${label}`, `Add ${label}`)
      : selectLocaleText(locale, `移除${label}`, `Remove ${label}`);
  }
  return operation === "add" ? rosterFieldActionLabel(field, locale) : rosterFieldRemoveLabel(field, locale);
}

/** The action inventory is fixed by the module. Selection only controls availability. */
export function SelectedPlayerRosterActions(props: SelectedPlayerRosterActionsProps) {
  const [editor, setEditor] = useState<EditorTarget | null>(() => props.createFieldKey ? { fieldKey: props.createFieldKey, entry: null } : null);
  const selectedRoster = props.selectedRoster ?? null;
  const disabled = props.disabled || props.busyFieldKey !== null;
  const editorField = props.fields.find((field) => field.key === editor?.fieldKey);
  const editorEntry = editor?.entry ? editorField?.entries.find((entry) => entry.key === editor.entry?.key) ?? null : null;

  async function mutate(field: RosterField, operation: "add" | "remove", rawValue: unknown) {
    const saved = await props.onMutate(field, operation, rawValue);
    if (saved && operation === "remove" && selectedRoster?.field.key === field.key) props.onClearSelection?.();
    return saved;
  }

  return <>
    <div className="player-access-selected-actions" role="group" aria-label={selectLocaleText(props.locale, "名单操作", "Roster actions")}>
      {props.fields.map((field) => {
        const target = resolveRosterActionTarget(props.selectedPlayer, selectedRoster, field);
        const entry = target ? field.entries.find((candidate) => candidate.key === target.identity) : undefined;
        return <div key={field.key} className="player-access-selected-row" data-roster-field={field.key}>
          {(["add", "remove"] as const).filter((operation) => operation !== "add" || !props.suppressedAddFieldKeys?.includes(field.key)).map((operation) => {
            const label = actionLabel(field, operation, props.locale);
            const unavailable = disabled || field.property.readOnly === true || !target || (operation === "add" ? Boolean(entry) : !entry);
            const rawValue = operation === "remove" ? entry?.rawValue : target?.rawValue;
            const displayName = entry?.label ?? props.selectedPlayer?.display_name ?? selectedRoster?.entry.label ?? "";
            const playerLabel = `${displayName}${target ? ` (${target.identity})` : ""}`.replace(/[\u0000-\u001f\u007f]/g, " ").trim();
            const confirmation = selectLocaleText(props.locale, `确认对「${playerLabel}」执行「${label}」？`, `Run “${label}” for “${playerLabel}”?`);
            const className = "secondary-button player-center-action-button player-access-selected-action";
            return <div key={operation} className="player-access-total-action" data-roster-operation={operation}>
              {operation === "add" && field.kind === "object-list" ? <button type="button" className={className}
                data-operation={operation} disabled={unavailable} title={field.title} aria-label={`${label} · ${field.title}`}
                onClick={() => target && setEditor({ fieldKey: field.key, entry: null, identity: target })}>{label}</button>
                : <InlineConfirmAction className={className} data-operation={operation} disabled={unavailable} title={field.title} aria-label={`${label} · ${field.title}`}
                  scopeKey={JSON.stringify([field.key, field.currentValue, operation, rawValue])} confirmation={confirmation}
                  onConfirm={async () => { if (!unavailable) await mutate(field, operation, rawValue); }}>{label}</InlineConfirmAction>}
            </div>;
          })}
        </div>;
      })}
      <button type="button" className="secondary-button player-center-action-button player-access-edit-selected" disabled={disabled || !selectedRoster
        || selectedRoster.field.kind !== "object-list" || selectedRoster.field.property.readOnly === true}
        onClick={() => selectedRoster && setEditor({ fieldKey: selectedRoster.field.key, entry: selectedRoster.entry })}>
        {selectLocaleText(props.locale, "编辑当前条目", "Edit selected entry")}
      </button>
    </div>
    {editor && editorField ? <div className="player-access-selected-editor" data-editor-field={editorField.key}>
      <strong>{editorField.title}</strong>
      {editor.identity ? <ObjectRosterFieldEditor key={`${editorField.key}:quick`} field={editorField} busy={props.busyFieldKey === editorField.key}
        disabled={disabled} locale={props.locale} onMutate={mutate} selectedEntry={null} lockedIdentity={editor.identity}
        inputPrefix={`player-access-quick-${editorField.key}`}
        onCancel={() => setEditor(null)} onSaved={() => setEditor(null)} />
        : <PlayerAccessRosterEditor key={`${editorField.key}:${editorEntry?.key ?? "create"}`} field={editorField}
          busy={props.busyFieldKey === editorField.key} disabled={disabled} locale={props.locale} onMutate={mutate}
          selectedEntry={editorEntry} selectedPlayer={props.selectedPlayer} onCancel={() => setEditor(null)} onSaved={() => setEditor(null)} />}
    </div> : null}
  </>;
}
