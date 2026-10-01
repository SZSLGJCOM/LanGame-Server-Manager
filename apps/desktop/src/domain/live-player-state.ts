import type { RuntimeLivePlayerEntry, RuntimeLivePlayerSnapshot } from "../types";

export type LivePlayerPresentationKind =
  | "initial"
  | "ready"
  | "empty"
  | "refreshing-with-rows"
  | "refreshing-without-rows"
  | "stopped"
  | "unsupported"
  | "adapter-unavailable"
  | "count-only"
  | "misconfigured"
  | "failed-with-rows"
  | "failed-without-rows"
  | "truncated"
  | "incomplete";

export type LivePlayerRefreshReason =
  | "status"
  | "expired"
  | "stale"
  | "non-authoritative";

export interface LivePlayerPresentation {
  kind: LivePlayerPresentationKind;
  rows: RuntimeLivePlayerEntry[];
  tableVisible: boolean;
  stateVisible: boolean;
  actionPanelVisible: boolean;
  actionsEnabled: boolean;
  authoritative: boolean;
  authoritativeEmpty: boolean;
  refreshReason: LivePlayerRefreshReason | null;
  selectedPlayerKey: string | null;
  selectedPlayer: RuntimeLivePlayerEntry | null;
}

export type AuthoritativeLivePlayerSnapshot = RuntimeLivePlayerSnapshot & {
  status: "ready";
  expires_at_unix_ms: number;
  complete: true;
  truncated: false;
  stale: false;
};

export interface PlayerCenterState {
  instanceId: string;
  selectedPlayerKey: string | null;
  snapshot: RuntimeLivePlayerSnapshot | null;
  requestGeneration: number;
}

export interface PlayerCenterRequestToken {
  instanceId: string;
  generation: number;
}

export interface PlayerCenterViewModel {
  livePlayers: LivePlayerPresentation;
}

export type PlayerCenterEvent =
  | {
      type: "instance-changed";
      instanceId: string;
      snapshot?: RuntimeLivePlayerSnapshot | null;
    }
  | {
      type: "player-selected";
      playerKey: string | null;
    }
  | {
      type: "snapshot-received";
      snapshot: RuntimeLivePlayerSnapshot;
    };

export interface CreatePlayerCenterStateInput {
  instanceId: string;
  snapshot?: RuntimeLivePlayerSnapshot | null;
}

function snapshotBelongsToInstance(
  snapshot: RuntimeLivePlayerSnapshot | null | undefined,
  instanceId: string
): snapshot is RuntimeLivePlayerSnapshot {
  return snapshot?.instance_id === instanceId;
}

function playerKeyExists(snapshot: RuntimeLivePlayerSnapshot | null, playerKey: string | null): boolean {
  return Boolean(
    playerKey
      && snapshot?.entries.some((entry) => entry.player_key === playerKey)
  );
}

function reconcileSelectedPlayerKey(
  snapshot: RuntimeLivePlayerSnapshot | null,
  selectedPlayerKey: string | null
): string | null {
  return playerKeyExists(snapshot, selectedPlayerKey) ? selectedPlayerKey : null;
}

export function createPlayerCenterState(input: CreatePlayerCenterStateInput): PlayerCenterState {
  const snapshot = snapshotBelongsToInstance(input.snapshot, input.instanceId)
    ? input.snapshot
    : null;

  return {
    instanceId: input.instanceId,
    selectedPlayerKey: null,
    snapshot,
    requestGeneration: 0
  };
}

export function reducePlayerCenterState(
  state: PlayerCenterState,
  event: PlayerCenterEvent
): PlayerCenterState {
  switch (event.type) {
    case "instance-changed": {
      const nextSnapshot = snapshotBelongsToInstance(event.snapshot, event.instanceId)
        ? event.snapshot
        : null;
      return {
        instanceId: event.instanceId,
        selectedPlayerKey: null,
        snapshot: nextSnapshot,
        requestGeneration: state.requestGeneration + 1
      };
    }
    case "player-selected": {
      const nextPlayerKey = reconcileSelectedPlayerKey(state.snapshot, event.playerKey);
      if (nextPlayerKey === state.selectedPlayerKey) {
        return state;
      }
      return {
        ...state,
        selectedPlayerKey: nextPlayerKey
      };
    }
    case "snapshot-received":
      if (!snapshotBelongsToInstance(event.snapshot, state.instanceId)) {
        return state;
      }
      return {
        ...state,
        snapshot: event.snapshot,
        selectedPlayerKey: reconcileSelectedPlayerKey(event.snapshot, state.selectedPlayerKey)
      };
  }
}

export function beginPlayerCenterRequest(state: PlayerCenterState): {
  state: PlayerCenterState;
  token: PlayerCenterRequestToken;
} {
  const generation = state.requestGeneration + 1;
  return {
    state: {
      ...state,
      requestGeneration: generation
    },
    token: {
      instanceId: state.instanceId,
      generation
    }
  };
}

export function completePlayerCenterRequest(
  state: PlayerCenterState,
  token: PlayerCenterRequestToken,
  snapshot: RuntimeLivePlayerSnapshot
): PlayerCenterState {
  if (
    token.generation !== state.requestGeneration
    || token.instanceId !== state.instanceId
    || snapshot.instance_id !== state.instanceId
  ) {
    return state;
  }

  return reducePlayerCenterState(state, {
    type: "snapshot-received",
    snapshot
  });
}

function hasFutureExpiry(snapshot: RuntimeLivePlayerSnapshot, now: number): boolean {
  return typeof snapshot.expires_at_unix_ms === "number"
    && Number.isFinite(snapshot.expires_at_unix_ms)
    && snapshot.expires_at_unix_ms > now;
}

export function isAuthoritativeLivePlayerSnapshot(
  snapshot: RuntimeLivePlayerSnapshot | null,
  now: number
): snapshot is AuthoritativeLivePlayerSnapshot {
  return Boolean(
    snapshot
      && snapshot.status === "ready"
      && snapshot.complete
      && !snapshot.truncated
      && !snapshot.stale
      && hasFutureExpiry(snapshot, now)
  );
}

function refreshingReason(
  snapshot: RuntimeLivePlayerSnapshot,
  now: number
): LivePlayerRefreshReason {
  if (snapshot.status === "refreshing") {
    return "status";
  }
  if (
    typeof snapshot.expires_at_unix_ms === "number"
    && snapshot.expires_at_unix_ms <= now
  ) {
    return "expired";
  }
  if (snapshot.stale) {
    return "stale";
  }
  return "non-authoritative";
}

function presentationKind(
  snapshot: RuntimeLivePlayerSnapshot | null,
  now: number
): {
  kind: LivePlayerPresentationKind;
  refreshReason: LivePlayerRefreshReason | null;
} {
  if (!snapshot) {
    return { kind: "initial", refreshReason: null };
  }

  switch (snapshot.status) {
    case "stopped":
      return { kind: "stopped", refreshReason: null };
    case "unsupported":
      return {
        kind: snapshot.issue?.code === "adapter_unavailable"
          ? "adapter-unavailable"
          : snapshot.issue?.code === "names_unavailable" ? "count-only" : "unsupported",
        refreshReason: null
      };
    case "misconfigured":
      return { kind: "misconfigured", refreshReason: null };
    case "failed":
      return {
        kind: snapshot.entries.length > 0 ? "failed-with-rows" : "failed-without-rows",
        refreshReason: null
      };
    default:
      break;
  }

  if (snapshot.status === "ready" && snapshot.truncated) {
    return { kind: "truncated", refreshReason: null };
  }
  if (snapshot.status === "ready" && !snapshot.complete) {
    if (snapshot.entries.length === 0 && snapshot.current_players !== null && snapshot.issue?.code === "names_unavailable") {
      return { kind: "count-only", refreshReason: null };
    }
    return { kind: "incomplete", refreshReason: null };
  }
  if (!isAuthoritativeLivePlayerSnapshot(snapshot, now)) {
    return {
      kind: snapshot.entries.length > 0
        ? "refreshing-with-rows"
        : "refreshing-without-rows",
      refreshReason: refreshingReason(snapshot, now)
    };
  }
  if (snapshot.entries.length === 0
    && ((snapshot.current_players ?? 0) > 0 || snapshot.issue?.code === "names_unavailable")) {
    return { kind: "count-only", refreshReason: null };
  }
  return {
    kind: snapshot.entries.length > 0 ? "ready" : "empty",
    refreshReason: null
  };
}

export function deriveLivePlayerPresentation(
  snapshot: RuntimeLivePlayerSnapshot | null,
  now: number,
  selectedPlayerKey: string | null
): LivePlayerPresentation {
  const { kind, refreshReason } = presentationKind(snapshot, now);
  const rows = snapshot?.entries ?? [];
  const terminalWithoutRows = kind === "initial"
    || kind === "empty"
    || kind === "stopped"
    || kind === "unsupported"
    || kind === "adapter-unavailable"
    || kind === "count-only"
    || kind === "misconfigured"
    || kind === "failed-without-rows"
    || kind === "refreshing-without-rows";
  const tableVisible = !terminalWithoutRows && rows.length > 0;
  const stateVisible = kind !== "ready"
    && kind !== "refreshing-with-rows";
  const selectedPlayer = tableVisible
    ? rows.find((entry) => entry.player_key === selectedPlayerKey) ?? null
    : null;
  const authoritative = isAuthoritativeLivePlayerSnapshot(snapshot, now);

  return {
    kind,
    rows,
    tableVisible,
    stateVisible,
    actionPanelVisible: Boolean(selectedPlayer?.available_action_ids.length),
    actionsEnabled: Boolean(
      authoritative
        && selectedPlayer
        && selectedPlayer.available_action_ids.length > 0
    ),
    authoritative,
    authoritativeEmpty: authoritative && rows.length === 0 && kind === "empty",
    refreshReason,
    selectedPlayerKey: selectedPlayer?.player_key ?? null,
    selectedPlayer
  };
}

export function derivePlayerCenterViewModel(
  state: PlayerCenterState,
  now: number
): PlayerCenterViewModel {
  return {
    livePlayers: deriveLivePlayerPresentation(
      state.snapshot,
      now,
      state.selectedPlayerKey
    )
  };
}
