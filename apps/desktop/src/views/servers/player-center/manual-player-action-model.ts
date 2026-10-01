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
  if (moduleId !== "humanitz" || !["kick_player", "ban_player", "unban_player"].includes(action.id)
    || isHumanitzNetId(target)) return null;
  return locale === "zh-CN"
    ? "请输入完整 NetID：EpicAccountId|ProductUserId 或 |ProductUserId，每个非空部分均为 32 位十六进制。短 ID、Steam64 ID 和玩家名不能用于该操作。"
    : "Enter the complete NetID: EpicAccountId|ProductUserId or |ProductUserId, with 32 hexadecimal digits per nonempty part. Short IDs, Steam64 IDs and player names cannot target this action.";
}
