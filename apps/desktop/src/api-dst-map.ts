import { invokeOrMock } from "./api-transport";
import { parseDstWorldMap } from "./dst-world-map";
import type { DontStarveShard } from "./views/settings/modules/dontstarve-shards";

export async function readDstWorldMap(instanceId: string, shard: DontStarveShard, includeTerrain: boolean) {
  return parseDstWorldMap(await invokeOrMock<unknown>("read_dontstarve_world_map", {
    input: { instanceId, shard, includeTerrain }
  }), instanceId, shard, includeTerrain);
}
