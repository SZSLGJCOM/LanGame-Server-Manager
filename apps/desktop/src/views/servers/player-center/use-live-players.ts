import { useEffect, useRef, useState } from "react";
import { describeError } from "../../../app-state";
import {
  readInstanceLivePlayers,
  refreshInstanceLivePlayers
} from "../../../api";
import { LivePlayerRefreshController } from "../../../domain/live-player-refresh";
import type { RuntimeLivePlayerSnapshot } from "../../../types";

export interface UseLivePlayersResult {
  snapshot: RuntimeLivePlayerSnapshot | null;
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
}

function shouldRefreshAfterRead(snapshot: RuntimeLivePlayerSnapshot): boolean {
  if (snapshot.status === "refreshing" || snapshot.status === "failed") {
    return true;
  }
  return snapshot.status === "ready"
    && (
      snapshot.stale
      || !snapshot.complete
      || snapshot.truncated
      || snapshot.expires_at_unix_ms === null
      || snapshot.expires_at_unix_ms <= Date.now()
    );
}

export function useLivePlayers(instanceId: string, visible: boolean, runtimeStatus?: string): UseLivePlayersResult {
  const [snapshot, setSnapshot] = useState<RuntimeLivePlayerSnapshot | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const controllerRef = useRef<LivePlayerRefreshController<number> | null>(null);
  const snapshotRef = useRef<RuntimeLivePlayerSnapshot | null>(null);

  useEffect(() => {
    setSnapshot(null);
    snapshotRef.current = null;
    setError(null);
  }, [instanceId]);

  useEffect(() => {
    let active = true;
    let pendingRequests = 0;
    const beginRequest = () => {
      pendingRequests += 1;
      if (active) {
        setLoading(true);
        setError(null);
      }
    };
    const endRequest = () => {
      pendingRequests = Math.max(0, pendingRequests - 1);
      if (active && pendingRequests === 0) {
        setLoading(false);
      }
    };
    setLoading(false);
    // An archived view has no live-player lifecycle, including visibility
    // listeners and imperative refresh requests.
    if (!visible) return;
    const controller = new LivePlayerRefreshController<number>({
      now: Date.now,
      schedule: (callback, delayMs) => window.setTimeout(callback, delayMs),
      cancel: (handle) => window.clearTimeout(handle),
      refresh: async (targetInstanceId) => {
        beginRequest();
        try {
          return await refreshInstanceLivePlayers(targetInstanceId);
        } finally {
          endRequest();
        }
      },
      onSnapshot: (nextSnapshot) => {
        if (active) {
          setSnapshot(nextSnapshot);
          setError(null);
        }
      },
      onError: (requestError) => {
        if (active) {
          setError(describeError(requestError));
        }
      }
    });
    controllerRef.current = controller;
    controller.setContext({ instanceId, visible: visible && document.visibilityState === "visible", snapshot: snapshotRef.current });

    const readCurrent = () => {
      beginRequest();
      void controller.read(async (targetInstanceId) => {
        try {
          const current = await readInstanceLivePlayers(targetInstanceId);
          return shouldRefreshAfterRead(current)
            ? await refreshInstanceLivePlayers(targetInstanceId)
            : current;
        } finally {
          endRequest();
        }
      });
    };

    const onVisibilityChange = () => {
      const pageVisible = visible && document.visibilityState === "visible";
      const currentSnapshot = snapshotRef.current;
      controller.setContext({
        instanceId,
        visible: pageVisible,
        snapshot: currentSnapshot
      });
      if (pageVisible && currentSnapshot && shouldRefreshAfterRead(currentSnapshot)) {
        void controller.refreshNow();
      } else if (pageVisible && !currentSnapshot) {
        readCurrent();
      }
    };
    document.addEventListener("visibilitychange", onVisibilityChange);

    if (visible && document.visibilityState === "visible") {
      readCurrent();
    }

    return () => {
      active = false;
      document.removeEventListener("visibilitychange", onVisibilityChange);
      controller.dispose();
      if (controllerRef.current === controller) {
        controllerRef.current = null;
      }
    };
  }, [instanceId, visible, runtimeStatus]);

  useEffect(() => {
    snapshotRef.current = snapshot;
    controllerRef.current?.setContext({
      instanceId,
      visible: visible && document.visibilityState === "visible",
      snapshot
    });
  }, [instanceId, visible, snapshot]);

  async function refresh() {
    if (!visible) return;
    await controllerRef.current?.refreshNow();
  }

  return { snapshot, loading, error, refresh };
}
