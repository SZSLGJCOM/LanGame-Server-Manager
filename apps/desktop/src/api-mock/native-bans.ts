import type { ExecuteInstancePlayerActionResult, NativeBanEntry, NativeBanRoster, RuntimeLivePlayerEntry } from "../types";
import type { MockLivePlayerContext } from "./live-players";

/** Preview-only state, independent of the writable settings rosters. */
export class MockNativeBanStore {
  private readonly entries = new Map<string, NativeBanEntry[]>();

  delete(instanceId: string): void {
    this.entries.delete(instanceId);
  }

  read(context: MockLivePlayerContext): NativeBanRoster {
    return { instance_id: context.instance_id, module_id: context.module_id, observed_at_unix_ms: Date.now(),
      entries: structuredClone(this.entries.get(context.instance_id) ?? []) };
  }

  ban(context: MockLivePlayerContext, player: RuntimeLivePlayerEntry | undefined): void {
    const kind = context.player_list?.identity_kind;
    const identity = player?.identifiers.filter((entry) => entry.kind === kind);
    if (!kind || identity?.length !== 1 || !identity[0].stable || !player) return;
    const entries = this.entries.get(context.instance_id) ?? [];
    if (!entries.some((entry) => entry.account_id === identity[0].value)) {
      entries.push({ account_id: identity[0].value, display_name: player.display_name, identity_kind: kind });
      this.entries.set(context.instance_id, entries);
    }
  }

  unban(context: MockLivePlayerContext, accountId: string): ExecuteInstancePlayerActionResult {
    if (!context.running && context.module_id !== "squad") throw new Error("Start the server before unbanning a native account.");
    const entries = this.entries.get(context.instance_id) ?? [];
    if (!entries.some((entry) => entry.account_id === accountId)) throw new Error("The account is not in the native ban roster.");
    if (entries.some((entry) => entry.account_id === accountId && entry.identity_kind === "palworld_player_uid")) {
      throw new Error("This historical ban has only a player GUID; the native interface requires a user ID to unban.");
    }
    this.entries.set(context.instance_id, entries.filter((entry) => entry.account_id !== accountId));
    return { action_id: context.module_id === "rimworld" ? "unban_user" : "unban_player", status: "sent",
      executed_at_unix_ms: Date.now(), summary: context.module_id === "squad"
        ? "Preview persisted ban removed; a server restart is required to take effect." : "Preview native account unbanned." };
  }
}
