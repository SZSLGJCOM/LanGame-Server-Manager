import { selectLocaleText, type LocaleCode } from "../../../i18n";
import type { RosterField } from "./player-access-roster-model";

export function rosterFieldActionLabel(field: RosterField, locale: LocaleCode): string {
  if (field.kind === "string-scalar") {
    return field.entries.length > 0
      ? selectLocaleText(locale, `替换${field.title}`, `Replace ${field.title}`)
      : selectLocaleText(locale, `设置${field.title}`, `Set ${field.title}`);
  }
  if (field.lane === "admin") {
    if (/^owner(?:_|$)/i.test(field.key)) return selectLocaleText(locale, "授予所有者权限", "Grant owner");
    if (/^moderator(?:_|$)/i.test(field.key)) return selectLocaleText(locale, "设为协管员", "Grant moderator");
    return selectLocaleText(locale, "设为管理员", "Grant admin");
  }
  if (field.lane === "allow") return selectLocaleText(locale, "加入白名单", "Add to allowlist");
  if (field.lane === "block") return selectLocaleText(locale, "加入黑名单", "Add to blocklist");
  return selectLocaleText(locale, "加入优先队列", "Add priority");
}

export function rosterFieldRemoveLabel(field: RosterField, locale: LocaleCode): string {
  if (field.kind === "string-scalar") return selectLocaleText(locale, `清除${field.title}`, `Clear ${field.title}`);
  if (field.lane === "admin") {
    if (/^owner(?:_|$)/i.test(field.key)) return selectLocaleText(locale, "取消所有者权限", "Revoke owner");
    if (/^moderator(?:_|$)/i.test(field.key)) return selectLocaleText(locale, "取消协管员", "Revoke moderator");
    return selectLocaleText(locale, "取消管理员", "Remove admin");
  }
  if (field.lane === "allow") return selectLocaleText(locale, "移出白名单", "Remove from allowlist");
  if (field.lane === "block") return selectLocaleText(locale, "解除封禁", "Unban");
  return selectLocaleText(locale, "移出优先队列", "Remove priority");
}
