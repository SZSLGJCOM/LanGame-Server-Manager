import type { SettingsObject } from "../settings-schema";

export const DONTSTARVE_SHARDS = ["master", "caves", "islands", "volcano"] as const;
export type DontStarveShard = typeof DONTSTARVE_SHARDS[number];

const STANDARD_SHARDS: readonly DontStarveShard[] = ["master", "caves"];
export const DONTSTARVE_SHARD_NAMES = {
  master: "Master", caves: "Caves", islands: "Islands", volcano: "Volcano"
} as const;

/** Layout controls which shard settings a synchronized Mod edit changes. */
export function getDontStarveLayoutShards(settings: Readonly<SettingsObject>): readonly DontStarveShard[] {
  return settings.shard_layout === "island_adventures" ? DONTSTARVE_SHARDS : STANDARD_SHARDS;
}

export function isDontStarveShardActive(settings: Readonly<SettingsObject>, shard: DontStarveShard): boolean {
  return shard === "master" || settings.shard_layout === "island_adventures" ||
    (shard === "caves" && settings.enable_caves === true);
}
