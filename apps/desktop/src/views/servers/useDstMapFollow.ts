import { useEffect, useRef, useState } from "react";
import { readDstWorldMap } from "../../api-dst-map";
import type { DstWorldMap } from "../../dst-world-map";
import type { DontStarveShard } from "../settings/modules/dontstarve-shards";

/** Search actual shard-local players only after the followed player leaves this shard. */
export function useDstMapFollow(input: {
  instanceId: string; runId: number | undefined; shard: DontStarveShard; runningShards: DontStarveShard[];
  selectedPlayer: string | null; current: DstWorldMap | null; live: boolean; onShard: (shard: DontStarveShard) => void;
}) {
  const callbacks = useRef(input);
  callbacks.current = input;
  const [error, setError] = useState<unknown>(null);
  const searching = Boolean(input.selectedPlayer && input.runId && input.runningShards.length
    && (!input.live || !input.current || !input.current.players.some(player => player.id === input.selectedPlayer)));
  const scope = JSON.stringify([input.instanceId, input.runId, input.shard, input.selectedPlayer, input.runningShards, searching]);
  useEffect(() => {
    setError(null);
    if (!searching) return;
    let disposed = false, busy = false, index = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const candidates = input.runningShards.filter(shard => shard !== input.shard);
    async function find() {
      if (disposed || busy || document.hidden || !candidates.length) return;
      busy = true;
      const shard = candidates[index++ % candidates.length];
      try {
        // One background position request at a time leaves the other native
        // collection slot available for the visible map. Never fetch its art.
        const next = await readDstWorldMap(input.instanceId, shard, false);
        if (disposed || document.hidden) return;
        if (next.runId === input.runId && Date.now() - next.capturedAtUnixMs < 6000
          && next.players.some(player => player.id === input.selectedPlayer)) {
          callbacks.current.onShard(shard);
          return;
        }
        setError(null);
      } catch (cause) { if (!disposed) setError(cause); }
      finally {
        busy = false;
        if (!disposed && !document.hidden) timer = setTimeout(() => void find(), 3000);
      }
    }
    const visible = () => { clearTimeout(timer); if (!document.hidden) void find(); };
    document.addEventListener("visibilitychange", visible);
    void find();
    return () => { disposed = true; clearTimeout(timer); document.removeEventListener("visibilitychange", visible); };
  }, [scope]);
  return { searching, error };
}
