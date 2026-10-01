import type { DstWorldStartPreview, DstWorldState, InstanceDetails } from "../types";
import { DONTSTARVE_SHARDS, DONTSTARVE_SHARD_NAMES, isDontStarveShardActive } from "../views/settings/modules/dontstarve-shards";
import type { SettingsObject } from "../views/settings/settings-schema";

type ShardStates = Record<typeof DONTSTARVE_SHARDS[number], DstWorldState>;

function statesFor(master: DstWorldState, other: DstWorldState): ShardStates {
  return { master, caves: other, islands: other, volcano: other };
}

function initialStates(): ShardStates {
  const fixture = typeof window === "undefined"
    ? null
    : new URLSearchParams(window.location.search).get("dstWorld");
  if (fixture === "new") return statesFor("new", "new");
  if (fixture === "mixed") return statesFor("existing", "new");
  if (fixture === "unrecognized") return statesFor("unrecognized", "unrecognized");
  return statesFor("existing", "existing");
}

export class MockDstWorldStateStore {
  private readonly states = new Map<string, ShardStates>();

  create(instanceId: string): void {
    this.states.set(instanceId, statesFor("new", "new"));
  }

  delete(instanceId: string): void {
    this.states.delete(instanceId);
  }

  start(details: InstanceDetails): void {
    if (details.summary.module_id !== "dontstarve") return;
    const previous = this.states.get(details.summary.id) ?? initialStates();
    const settings = JSON.parse(details.settings_json) as SettingsObject;
    this.states.set(details.summary.id, Object.fromEntries(DONTSTARVE_SHARDS.map((shard) =>
      [shard, isDontStarveShardActive(settings, shard) ? "existing" : previous[shard]])) as ShardStates);
  }

  validateConfirmation(details: InstanceDetails, expected: unknown): void {
    const current = this.preview(details);
    const matches = typeof expected === "object" && expected !== null
      && "instance_id" in expected && expected.instance_id === current.instance_id
      && "settings_json" in expected && expected.settings_json === current.settings_json
      && "shards" in expected && Array.isArray(expected.shards)
      && expected.shards.length === current.shards.length
      && expected.shards.every((shard: unknown, index: number) => {
        const actual = current.shards[index];
        return typeof shard === "object" && shard !== null
          && "shard" in shard && shard.shard === actual.shard
          && "state" in shard && shard.state === actual.state
          && "enabled" in shard && shard.enabled === actual.enabled;
      });
    if (!matches) {
      throw new Error(JSON.stringify({
        code: "dst_world_start_changed",
        message: "The saved settings or world data changed during startup preparation. Try starting the server again."
      }));
    }
  }

  preview(details: InstanceDetails): DstWorldStartPreview {
    if (details.summary.module_id !== "dontstarve") {
      throw new Error(`instance \`${details.summary.id}\` is not a Don't Starve Together server`);
    }
    const states = this.states.get(details.summary.id) ?? initialStates();
    const settings = JSON.parse(details.settings_json) as SettingsObject;
    return {
      instance_id: details.summary.id,
      settings_json: details.settings_json,
      shards: DONTSTARVE_SHARDS.filter((shard) => shard === "master" || shard === "caves" ||
        settings.shard_layout === "island_adventures").map((shard) => ({
        shard: DONTSTARVE_SHARD_NAMES[shard], state: states[shard], enabled: isDontStarveShardActive(settings, shard)
      }))
    };
  }
}
