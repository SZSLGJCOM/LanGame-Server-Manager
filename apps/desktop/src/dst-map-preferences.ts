import { DONTSTARVE_SHARDS, type DontStarveShard } from "./views/settings/modules/dontstarve-shards";

const KEY = "langame.dst-map.shards";
function entries(): [string, DontStarveShard][] {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw || raw.length > 16384) return [];
    const values: unknown = JSON.parse(raw);
    if (!Array.isArray(values)) return [];
    return values.slice(-32).filter((value): value is [string, DontStarveShard] =>
      Array.isArray(value) && value.length === 2 && typeof value[0] === "string" && value[0].length <= 256
      && DONTSTARVE_SHARDS.includes(value[1]));
  } catch { return []; }
}

export function readDstMapSelectedShard(instanceId: string): DontStarveShard {
  return entries().find(([id]) => id === instanceId)?.[1] ?? "master";
}

export function rememberDstMapSelectedShard(instanceId: string, shard: DontStarveShard): void {
  if (instanceId.length > 256 || !DONTSTARVE_SHARDS.includes(shard)) return;
  try { localStorage.setItem(KEY, JSON.stringify([...entries().filter(([id]) => id !== instanceId), [instanceId, shard]].slice(-32))); }
  catch { /* Optional navigation preference; the current selection still works. */ }
}
