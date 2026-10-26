import { useEffect, useMemo, useRef, useState } from "react";
import { arkToolsHostAvailable, readArkMap, readArkToolsStatus, type ArkToolsStatus } from "../../api-ark-tools";
import { acceptArkMapSnapshot, ARK_MAP_POLL_MS, ARK_MAP_STALE_MS, arkMapScopeKey, type ArkMapScope, type ArkMapSnapshot } from "../../ark-world-map";
import { describeError } from "../../app-state";

interface State { key: string; snapshot: ArkMapSnapshot | null; extension: ArkToolsStatus | null; error: string | null; loading: boolean }
export function useArkMapSession(input: ArkMapScope | null, running: boolean, expectedMap = "") {
  const key = `${input ? arkMapScopeKey(input) : ""}:${expectedMap}`;
  const scope = useMemo(() => input, [key]);
  const [state, setState] = useState<State>({ key: "", snapshot: null, extension: null, error: null, loading: false });
  const [now, setNow] = useState(Date.now);
  const [visible, setVisible] = useState(() => !document.hidden);
  const inFlight = useRef(false);
  const latestPoll = useRef<(() => void) | null>(null);
  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let connectedExtension: ArkToolsStatus | null = null;
    const available = running && scope && arkToolsHostAvailable();
    setState(previous => ({ key, snapshot: previous.key === key ? previous.snapshot : null,
      extension: null, error: null, loading: Boolean(available) }));
    async function poll() {
      if (!active || !available || !scope || document.hidden || inFlight.current) return;
      clearTimeout(timer);
      inFlight.current = true;
      try {
        const extension = connectedExtension ?? await readArkToolsStatus(scope.instanceId, scope.processKey);
        if (!active) return;
        connectedExtension = extension.connected ? extension : null;
        const snapshot = extension.connected ? await readArkMap(scope) : null;
        if (!active) return;
        if (snapshot && expectedMap && snapshot.mapName.toLowerCase() !== expectedMap.toLowerCase()) throw new Error("ARK map does not match the selected configured map.");
        if (snapshot && snapshot.sampledAtUnixMs > Date.now() + 30000) throw new Error("ARK map sample time is in the future.");
        setState(previous => ({ key, extension, error: null, loading: false,
          snapshot: snapshot ? acceptArkMapSnapshot(previous.key === key ? previous.snapshot : null, snapshot, scope)
            : previous.key === key ? previous.snapshot : null }));
        setNow(Date.now());
      } catch (error) {
        connectedExtension = null;
        if (active) setState(previous => ({ ...previous, key, error: describeError(error), loading: false }));
      } finally {
        inFlight.current = false;
        if (active && !document.hidden) timer = setTimeout(() => void poll(), ARK_MAP_POLL_MS);
        else if (!active) latestPoll.current?.();
      }
    }
    function visibility() {
      setVisible(!document.hidden); setNow(Date.now());
      if (document.hidden) clearTimeout(timer); else void poll();
    }
    latestPoll.current = () => void poll();
    document.addEventListener("visibilitychange", visibility);
    const clock = setInterval(() => { if (!document.hidden) setNow(Date.now()); }, 1000);
    void poll();
    return () => { active = false; clearTimeout(timer); clearInterval(clock); latestPoll.current = null; document.removeEventListener("visibilitychange", visibility); };
  }, [scope, key, running, expectedMap]);
  const current = state.key === key ? state : { snapshot: null, extension: null, error: null, loading: running };
  const live = Boolean(running && visible && current.extension?.connected && !current.error && current.snapshot
    && now - current.snapshot.sampledAtUnixMs <= ARK_MAP_STALE_MS);
  return { ...current, live, host: arkToolsHostAvailable(), refresh: () => latestPoll.current?.() };
}
