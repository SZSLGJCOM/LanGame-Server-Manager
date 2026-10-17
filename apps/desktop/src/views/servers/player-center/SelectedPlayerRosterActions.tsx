import { useState } from "react";
import { isValheimPlatformAccountTarget, parsePlayerAccessCodec } from "../../../domain/player-access";
import { InlineConfirmAction } from "../../../components/InlineConfirmAction";
import { selectLocaleText, type LocaleCode } from "../../../i18n";
import type { RuntimeLivePlayerEntry } from "../../../types";
import { ObjectRosterFieldEditor, PlayerAccessRosterEditor, type RosterMutationHandler } from "./PlayerAccessRosterEditor";
import { rosterFieldActionLabel, rosterFieldRemoveLabel } from "./player-access-roster-labels";
import type { RosterEntry, RosterField } from "./player-access-roster-model";
import type { SelectedRosterTarget } from "./player-access-selected-target";
import { resolveRosterActionTarget, type SelectedRosterEntry } from "./player-center-selection";
import { ConfigurationHelp } from "../../settings/ConfigurationFieldHelp";

interface SelectedPlayerRosterActionsProps {
  selectedPlayer: RuntimeLivePlayerEntry | null;
  selectedRoster?: SelectedRosterEntry | null;
  fields: RosterField[];
  suppressedAddFieldKeys?: readonly string[];
  createFieldKey?: string;
  disabled: boolean;
  disabledReason?: string | null;
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
  function unavailableReason(field: RosterField, operation: "add" | "remove", target: SelectedRosterTarget | null, entry: RosterEntry | undefined): string | null {
    if (props.busyFieldKey !== null) return selectLocaleText(props.locale, "名单正在保存，请等待完成。", "A roster is being saved. Wait for it to finish.");
    if (props.disabled) return props.disabledReason ?? selectLocaleText(props.locale, "当前无法保存名单。", "Rosters cannot be saved in the current workspace.");
    if (field.property.readOnly === true) return selectLocaleText(props.locale, `${field.title}为只读名单。`, `${field.title} is read-only.`);
    if (!target) {
      if (!props.selectedPlayer && !selectedRoster) return selectLocaleText(props.locale, "请先选择玩家或名单条目。", "Select a player or roster entry first.");
      if (parsePlayerAccessCodec(field.property) === "uint64") return selectLocaleText(props.locale,
        "此名单需要原生 accountId，当前玩家数据未提供该身份；Steam ID 不能替代。可在名单中手动添加。",
        "This roster requires native accountId values, which are missing from the player data. A Steam ID cannot replace one. Add a known accountId in the roster.");
      if (parsePlayerAccessCodec(field.property) === "valheim_platform_id") return selectLocaleText(props.locale,
        "当前未能自动取得此玩家唯一、已核实的 Platform User ID。此名单需要平台身份；可从游戏 F2 面板取得后在名单中手动添加。",
        "Automatic collection has not provided one verified Platform User ID for this player. This roster requires a platform identity; obtain it from the game's F2 panel and add it manually in the roster.");
      if (parsePlayerAccessCodec(field.property) === "valheim_ban") return selectLocaleText(props.locale,
        "当前未能取得唯一、已核实的平台身份或可用的原生角色名，不能使用显示名称或会话 ID 代替。可在封禁名单中手动添加。",
        "A unique verified platform identity or usable native character name is unavailable. A display label or session ID cannot replace it. Add a known target manually in the ban roster.");
      return selectLocaleText(props.locale, `当前所选记录未提供「${field.title}」所需的身份。`, `The selected record does not provide the identity required by ${field.title}.`);
    }
    if (operation === "add" && entry) return selectLocaleText(props.locale, `此玩家已在「${field.title}」中。`, `This player is already in ${field.title}.`);
    if (operation === "remove" && !entry) return selectLocaleText(props.locale, `此玩家不在「${field.title}」中。`, `This player is not in ${field.title}.`);
    return null;
  }
  const editReason = disabled ? props.disabledReason ?? selectLocaleText(props.locale, "名单正在保存，请等待完成。", "A roster is being saved. Wait for it to finish.")
    : !selectedRoster ? selectLocaleText(props.locale, "请先选择名单条目。", "Select a roster entry first.")
      : selectedRoster.field.property.readOnly === true ? selectLocaleText(props.locale, "此名单只读。", "This roster is read-only.")
        : selectedRoster.field.kind !== "object-list" ? selectLocaleText(props.locale, "此名单条目不含可编辑字段；可移除后重新添加。", "This entry has no editable fields. Remove it and add a replacement.") : null;

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
        const valheimBan = parsePlayerAccessCodec(field.property) === "valheim_ban";
        const platformBan = valheimBan && typeof target?.rawValue === "string" && isValheimPlatformAccountTarget(target.rawValue);
        const nameBan = valheimBan && !platformBan;
        const nameBanHelp = selectLocaleText(props.locale,
          "角色名可重名或更改。名单保存后服务端可能自动重读；本次操作未核验在线踢出结果，也不等同于账号封禁。",
          "Character names can be shared or changed. The server may reload the roster automatically after saving. This operation does not verify whether an online player was kicked and is not an account ban.");
        const allowlistHelp = field.key === "permitted_list" && parsePlayerAccessCodec(field.property) === "valheim_platform_id"
          ? selectLocaleText(props.locale,
            "名单非空时，仅名单内账号可进入；移除最后一项会取消白名单限制，封禁规则继续有效。",
            "When the allowlist is non-empty, only listed accounts can enter. Removing its last entry disables the allowlist restriction; bans still apply.") : null;
        return <div key={field.key} className="player-access-selected-row" data-roster-field={field.key}>
          {(["add", "remove"] as const).filter((operation) => operation !== "add" || !props.suppressedAddFieldKeys?.includes(field.key)).map((operation) => {
            const label = nameBan ? operation === "add" ? selectLocaleText(props.locale, "封禁角色名", "Ban character name")
              : selectLocaleText(props.locale, "解除角色名封禁", "Unban character name") : actionLabel(field, operation, props.locale);
            const unavailable = disabled || field.property.readOnly === true || !target || (operation === "add" ? Boolean(entry) : !entry);
            const helpText = [unavailableReason(field, operation, target, entry), nameBan ? nameBanHelp : null, allowlistHelp].filter(Boolean).join(" ");
            const rawValue = operation === "remove" ? entry?.rawValue : target?.rawValue;
            const displayName = entry?.label ?? props.selectedPlayer?.display_name ?? selectedRoster?.entry.label ?? "";
            const playerLabel = `${displayName}${target ? ` (${target.identity})` : ""}`.replace(/[\u0000-\u001f\u007f]/g, " ").trim();
            const confirmation = nameBan ? selectLocaleText(props.locale,
              `确认对角色名「${String(target?.rawValue ?? "")}」执行「${label}」？${nameBanHelp}`,
              `Run “${label}” for character name “${String(target?.rawValue ?? "")}”? ${nameBanHelp}`)
              : selectLocaleText(props.locale, `确认对「${playerLabel}」执行「${label}」？`, `Run “${label}” for “${playerLabel}”?`)
                + (allowlistHelp ? ` ${allowlistHelp}` : "");
            // Adding to a block/ban roster is a moderation action and uses the same danger variant as kick and ban.
            const className = `secondary-button player-center-action-button player-access-selected-action${operation === "add" && field.lane === "block" ? " danger" : ""}`;
            return <ConfigurationHelp key={operation} description={helpText}>{(help) => <div className="player-access-total-action" data-roster-operation={operation}
              ref={help.anchorRef} {...help.interactionProps} tabIndex={unavailable ? 0 : undefined}
              role={unavailable ? "group" : undefined} aria-label={unavailable ? `${label} · ${field.title}` : undefined}
              aria-disabled={unavailable || undefined} aria-describedby={help.descriptionId}>
              {operation === "add" && field.kind === "object-list" ? <button type="button" className={className}
                data-operation={operation} disabled={unavailable} title={field.title} aria-label={`${label} · ${field.title}`}
                aria-describedby={help.descriptionId}
                onClick={() => target && setEditor({ fieldKey: field.key, entry: null, identity: target })}>{label}</button>
                : <InlineConfirmAction className={className} data-operation={operation} disabled={unavailable} title={field.title} aria-label={`${label} · ${field.title}`}
                  aria-describedby={help.descriptionId}
                  scopeKey={JSON.stringify([field.key, field.currentValue, operation, rawValue])} confirmation={confirmation}
                  onConfirm={async () => { if (!unavailable) await mutate(field, operation, rawValue); }}>{label}</InlineConfirmAction>}
            </div>}</ConfigurationHelp>;
          })}
        </div>;
      })}
      <ConfigurationHelp description={editReason}>{(help) => <div className="player-access-total-action" ref={help.anchorRef} {...help.interactionProps}
        tabIndex={editReason ? 0 : undefined} role={editReason ? "group" : undefined} aria-disabled={Boolean(editReason) || undefined}
        aria-label={editReason ? selectLocaleText(props.locale, "编辑当前条目", "Edit selected entry") : undefined} aria-describedby={help.descriptionId}>
      <button type="button" className="secondary-button player-center-action-button player-access-edit-selected" aria-describedby={help.descriptionId} disabled={disabled || !selectedRoster
        || selectedRoster.field.kind !== "object-list" || selectedRoster.field.property.readOnly === true}
        onClick={() => selectedRoster && setEditor({ fieldKey: selectedRoster.field.key, entry: selectedRoster.entry })}>
        {selectLocaleText(props.locale, "编辑当前条目", "Edit selected entry")}
      </button>
      </div>}</ConfigurationHelp>
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
