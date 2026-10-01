import type { SystemSnapshot } from "./types";

type PlayerCountSnapshot = Pick<SystemSnapshot,
  "running_instances" | "total_online_players" | "total_player_capacity"
  | "player_count_queried_instances" | "player_count_queryable_instances">;

export interface SystemPlayerCounts {
  state: "idle" | "unknown" | "partial" | "complete";
  runningInstances: number;
  queriedInstances: number | null;
  onlinePlayers: number | null;
  capacity: number | null;
}

function count(value: number | null | undefined): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : null;
}

export function resolveSystemPlayerCounts(
  snapshot: PlayerCountSnapshot,
  fallbackRunningInstances: number
): SystemPlayerCounts {
  const runningInstances = count(snapshot.running_instances) ?? count(fallbackRunningInstances) ?? 0;
  const reportedCapacity = count(snapshot.total_player_capacity);
  const capacity = reportedCapacity !== null && reportedCapacity > 0 ? reportedCapacity : null;
  if (runningInstances === 0) {
    // Counts can outlive a stopped instance in an asynchronously refreshed snapshot.
    return { state: "idle", runningInstances, queriedInstances: 0, onlinePlayers: 0, capacity: null };
  }

  const queried = count(snapshot.player_count_queried_instances);
  const queryable = count(snapshot.player_count_queryable_instances);
  const coverageValid = queried !== null && queryable !== null
    && queried <= queryable && queryable <= runningInstances;
  const queriedInstances = coverageValid ? queried : null;
  const onlinePlayers = count(snapshot.total_online_players);
  if (queriedInstances === null || queriedInstances === 0 || onlinePlayers === null) {
    return { state: "unknown", runningInstances, queriedInstances, onlinePlayers: null, capacity };
  }

  return {
    state: queriedInstances === runningInstances ? "complete" : "partial",
    runningInstances,
    queriedInstances,
    onlinePlayers,
    // A known count may cover servers whose capacity was not reported.
    capacity: capacity !== null && capacity >= onlinePlayers ? capacity : null
  };
}
