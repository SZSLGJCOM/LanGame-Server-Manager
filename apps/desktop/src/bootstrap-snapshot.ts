import type { BootstrapResponse } from "./types";

/** Ordinary state reloads and late cache responses must not rewind host observations. */
export function mergeBootstrapSnapshot(current: BootstrapResponse, latest: BootstrapResponse, requested?: BootstrapResponse): BootstrapResponse {
  if (current.booted_at_unix_ms !== latest.booted_at_unix_ms) return latest;
  // A bootstrap request also waits for telemetry. Independent readers and
  // mutations may have published newer collections while that request waited.
  if (requested) latest = { ...latest, state: { ...latest.state,
    jobs: current.state.jobs !== requested.state.jobs ? current.state.jobs : latest.state.jobs,
    modules: current.state.modules !== requested.state.modules ? current.state.modules : latest.state.modules,
    instances: current.state.instances !== requested.state.instances ? current.state.instances : latest.state.instances
  } };
  const currentTime = current.state.snapshot.telemetry?.observed_at_unix_ms;
  const latestTime = latest.state.snapshot.telemetry?.observed_at_unix_ms;
  if (typeof currentTime !== "number" || !Number.isFinite(currentTime) || currentTime <= 0) return latest;
  if (typeof latestTime === "number" && Number.isFinite(latestTime) && latestTime >= currentTime) return latest;

  return {
    ...latest,
    state: {
      ...latest.state,
      snapshot: { ...current.state.snapshot, running_instances: latest.state.snapshot.running_instances }
    }
  };
}
