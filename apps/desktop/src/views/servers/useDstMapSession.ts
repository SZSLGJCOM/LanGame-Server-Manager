import { useEffect, useRef, useState } from "react";
import { readDstWorldMap } from "../../api-dst-map";
import { readDstMapCache, saveDstMapCache, dstMapCachedView, type DstMapCacheEntry, type DstMapCachedView } from "../../dst-map-cache";
import { mergeDstMapPositions, dstMapWorldKey } from "../../dst-map-session";
import type { DstWorldMap } from "../../dst-world-map";
import type { DontStarveShard } from "../settings/modules/dontstarve-shards";

interface MapState {
  scope: string; snapshot: DstWorldMap | null; terrainAt: number; initialView: DstMapCachedView | null;
  error: unknown; loading: boolean; live: boolean; cacheFailed: boolean;
}

/**
 * `freshFromRunId` invalidates maps of a replaced world: a cached or live map is
 * only shown when it came from that run or later (null accepts every run).
 */
export function useDstMapSession(instanceId: string, shard: DontStarveShard, runId: number | undefined, running: boolean,
  freshFromRunId: number | null = null) {
  const scope = JSON.stringify([instanceId, shard, runId ?? null, running, freshFromRunId]);
  const empty: MapState = { scope, snapshot: null, terrainAt: 0, initialView: null,
    error: null, loading: false, live: false, cacheFailed: false };
  const [state, setState] = useState<MapState>(empty);
  const refresh = useRef<(() => void) | null>(null);
  const viewChanged = useRef<((snapshot: DstWorldMap, view: DstMapCachedView) => void) | null>(null);

  useEffect(() => {
    let disposed = false, busy = false, forceTerrain = false;
    let timer: ReturnType<typeof setTimeout> | undefined, freshness: ReturnType<typeof setTimeout> | undefined;
    let latest: DstMapCacheEntry | null = null, saved: DstMapCacheEntry | null = null;
    let terrainReceivedAt = 0;
    const update = (value: Partial<MapState>) => {
      if (!disposed) setState(previous => ({ ...(previous.scope === scope ? previous : empty), ...value }));
    };
    const persist = (entry: DstMapCacheEntry) => {
      void saveDstMapCache(entry).then(ok => update({ cacheFailed: !ok }));
    };
    setState(empty);
    async function collect(force = false) {
      if (disposed || busy || !running || document.hidden) return;
      busy = true;
      update({ loading: true });
      const terrain = force || forceTerrain || !latest || Date.now() - terrainReceivedAt >= 60000;
      forceTerrain = false;
      try {
        const next = await readDstWorldMap(instanceId, shard, terrain);
        if (disposed) return;
        if (next.runId !== runId) throw new Error("DST_MAP_SHARD_CHANGED: The server run changed. Wait for its status to refresh.");
        // The world being replaced may still answer while it stops; it is never shown or cached again.
        if (freshFromRunId !== null && next.runId < freshFromRunId) { update({ snapshot: null, live: false, initialView: null }); return; }
        const merged = mergeDstMapPositions(latest?.snapshot ?? null, next);
        if (!merged) {
          // A regeneration or changed geometry must never inherit the old terrain.
          latest = null; saved = null; forceTerrain = true;
          update({ snapshot: null, live: false, initialView: null });
          return;
        }
        const view = latest ? dstMapCachedView(latest, merged) : saved ? dstMapCachedView(saved, merged) : null;
        latest = { snapshot: merged, terrainAt: terrain ? next.capturedAtUnixMs : latest?.terrainAt ?? next.capturedAtUnixMs, view };
        if (terrain) { terrainReceivedAt = Date.now(); persist(latest); }
        // Native capture can predate expensive background artwork decoding.
        const remaining = Math.max(0, Math.min(6000, 6000 - (Date.now() - next.capturedAtUnixMs)));
        update({ snapshot: merged, terrainAt: latest.terrainAt, initialView: view,
          live: !document.hidden && remaining > 0, error: null });
        clearTimeout(freshness);
        freshness = setTimeout(() => update({ live: false }), remaining);
      } catch (error) {
        update({ error, live: false });
      } finally {
        busy = false;
        if (!disposed) {
          update({ loading: false });
          if (!document.hidden) timer = setTimeout(() => void collect(), forceTerrain ? 0 : 3000);
        }
      }
    }
    refresh.current = () => { forceTerrain = true; clearTimeout(timer); void collect(true); };
    viewChanged.current = (snapshot, view) => {
      if (latest && dstMapWorldKey(latest.snapshot) === dstMapWorldKey(snapshot)) latest = { ...latest, view };
    };
    const visible = () => {
      if (document.hidden) { clearTimeout(timer); update({ live: false }); if (latest) persist(latest); }
      else if (!busy) { clearTimeout(timer); void collect(); }
    };
    document.addEventListener("visibilitychange", visible);
    void readDstMapCache(instanceId, shard).then(entry => {
      if (disposed) return;
      saved = entry;
      // A stopped shard may show its last map. A new native run must first
      // confirm its own session; old players are never imported into that run.
      if (entry && (!running || entry.snapshot.runId === runId) && (freshFromRunId === null || entry.snapshot.runId >= freshFromRunId)) {
        latest = entry;
        update({ snapshot: entry.snapshot, terrainAt: entry.terrainAt, initialView: entry.view });
      }
      void collect(true);
    });
    return () => {
      disposed = true; clearTimeout(timer); clearTimeout(freshness);
      if (latest) void saveDstMapCache(latest);
      refresh.current = null; viewChanged.current = null;
      document.removeEventListener("visibilitychange", visible);
    };
    // Every result, timer and view belongs to this instance/shard/run lifecycle.
  }, [scope]);

  return { ...(state.scope === scope ? state : empty),
    refresh: () => refresh.current?.(),
    rememberView: (snapshot: DstWorldMap, view: DstMapCachedView) => viewChanged.current?.(snapshot, view) };
}
