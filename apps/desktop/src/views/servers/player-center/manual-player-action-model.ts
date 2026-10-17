import {
  buildPlayerAccessBindings,
  isHumanitzNetId,
  filterConsumedPlayerAccessActions
} from "../../../domain/player-access";
import type { ModuleDetails, ModulePlayerActionDetails } from "../../../types";
import type { PlayerAccessRosterCapability } from "./player-access-roster-model";

export function actionNeedsManualTarget(action: ModulePlayerActionDetails): boolean {
  return action.target_required === true || action.command_template.includes("{{target}}");
}

export function readManualPlayerActions(
  moduleDetails: ModuleDetails | null,
  rosterCapabilities: PlayerAccessRosterCapability[]
): ModulePlayerActionDetails[] {
  if (!moduleDetails || moduleDetails.runtime.player_management?.status === "pending_adapter") {
    return [];
  }
  const list = moduleDetails.runtime.player_list;
  const listActions = new Set([list?.action_id, ...(list?.player_action_ids ?? [])]);
  const declared = (moduleDetails.runtime.player_actions ?? [])
    .filter((action) => action.kind !== "broadcast" && !listActions.has(action.id));
  const bindings = buildPlayerAccessBindings(rosterCapabilities, declared);
  return filterConsumedPlayerAccessActions(declared, bindings)
    .filter(actionNeedsManualTarget);
}


export function manualPlayerActionTargetError(
  moduleId: string, action: ModulePlayerActionDetails, target: string, locale: string
): string | null {
  if (moduleId === "soulmask" && ["ban_player", "unban_player"].includes(action.id)) {
    const steamId = target.length === 17 && /^[0-9]{17}$/.test(target) && /[1-9]/.test(target);
    const epicId = target.length === 65 && /^[0-9a-f]{32}\|[0-9a-f]{32}$/i.test(target);
    if (steamId || epicId) return null;
    return locale === "zh-CN"
      ? "请输入完整账号 ID：17 位正数字 Steam ID，或 EpicAccountId|ProductUserId（两部分均为 32 位十六进制）。短 ID、单段 Epic ID 和玩家名不能用于封禁或解封。"
      : "Enter a complete account ID: a positive 17-digit Steam ID or EpicAccountId|ProductUserId (32 hexadecimal digits in each part). Short IDs, a single Epic ID and player names cannot be used to ban or unban.";
  }
  if (moduleId === "palworld" && action.id === "unban_player") {
    const candidate = target.replace(/^urn:uuid:/i, "").replace(/^\{(.*)\}$/, "$1");
    if (/^(?:[0-9a-f]{32}|[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$/i.test(candidate)) {
      return locale === "zh-CN" ? "请输入原生用户 ID（如 steam_76561198000000001）；玩家 GUID 不能用于当前解封接口。"
        : "Enter a native user ID (such as steam_76561198000000001); player GUIDs cannot target the current unban interface.";
    }
    if (!target.trim() || [...target].length > 128 || /[\u0000-\u001f\u007f-\u009f]/u.test(target)) {
      return locale === "zh-CN" ? "请输入完整原生用户 ID，最多 128 个字符，不能包含控制字符。"
        : "Enter a complete native user ID of at most 128 characters without control characters.";
    }
    return null;
  }
  if (moduleId === "runescapedragonwilds" && action.id === "unban_player") {
    if (/^[0-9a-f]{32}$/.test(target)) return null;
    return locale === "zh-CN"
      ? "请输入完整的 32 位小写十六进制 EOS 账号 ID，可从在线玩家详情复制。玩家名、Steam64 ID 和会话 ID 不能用于解封。"
      : "Enter the complete 32-digit lowercase hexadecimal EOS account ID, copied from player details. Player names, Steam64 IDs and session IDs cannot be used to unban.";
  }
  if (moduleId !== "humanitz" || !["kick_player", "ban_player", "unban_player"].includes(action.id)
    || isHumanitzNetId(target)) return null;
  return locale === "zh-CN"
    ? "请输入完整 NetID：EpicAccountId|ProductUserId 或 |ProductUserId，每个非空部分均为 32 位十六进制。短 ID、Steam64 ID 和玩家名不能用于该操作。"
    : "Enter the complete NetID: EpicAccountId|ProductUserId or |ProductUserId, with 32 hexadecimal digits per nonempty part. Short IDs, Steam64 IDs and player names cannot target this action.";
}
