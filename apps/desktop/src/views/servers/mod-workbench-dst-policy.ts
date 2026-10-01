import type { SettingsObject } from "../settings/settings-schema";
import { getDontStarveLayoutShards } from "../settings/modules/dontstarve-shards";

export const DST_RAW_MOD_WARNING = "Raw modoverrides.lua is active. Edit it in settings before using structured Mod controls.";

/** Synchronized controls must not modify a shard owned by custom Lua. */
export function hasDstRawModOverrides(settings: Readonly<SettingsObject>): boolean {
  return getDontStarveLayoutShards(settings).some((shard) => {
    const raw = settings[`${shard}_modoverrides_lua`];
    if (typeof raw !== "string") return false;
    const normalized = raw.replace(/\r\n?/gu, "\n").trim();
    return normalized !== "" && normalized !== "return {\n}";
  });
}
