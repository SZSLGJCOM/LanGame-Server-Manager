import type {
  ExecuteInstancePlayerActionInput,
  ExecuteInstancePlayerActionResult,
  ModulePlayerListCodec,
  ModulePlayerListScope,
  ModulePlayerListSource,
  ModulePlayerListSpec,
  RuntimeLivePlayerEntry,
  RuntimeLivePlayerIssue,
  RuntimeLivePlayerSnapshot,
  RuntimePlayerIdentityKind
} from "../types";
import {
  mockModuleTomlById,
  readMockTomlInteger,
  readMockTomlString,
  readMockTomlStringArray,
  readMockTomlTable
} from "./module-assets";

export interface MockLivePlayerContext {
  instance_id: string;
  module_id: string;
  running: boolean;
  player_list: ModulePlayerListSpec | null;
  current_players: number | null;
  max_players: number | null;
  settings: Record<string, unknown>;
}

export function mockSteamQueryVisibilityIssue(moduleId: string, settings: Record<string, unknown>): RuntimeLivePlayerIssue | null {
  const restriction = moduleId === "valheim" && settings.public_server === 0
    ? { key: "public_server", summary: "Valheim private servers do not expose the Steam player query. Public visibility is required for this query." }
    : moduleId === "vrising" && settings.list_on_steam === false
      ? { key: "list_on_steam", summary: "V Rising does not expose Steam player queries while List On Steam is disabled. Direct game connections remain available." }
      : moduleId === "abioticfactor" && settings.lan_only === true
        ? { key: "lan_only", summary: "Abiotic Factor does not expose Steam player queries in LAN Only mode. LAN game discovery remains available." }
      : null;
  return restriction ? { code: "query_unavailable", setting_keys: [restriction.key], summary: restriction.summary } : null;
}

export interface MockLivePlayerCollection {
  status: "ready" | "failed";
  entries: RuntimeLivePlayerEntry[];
  complete: boolean;
  truncated: boolean;
  issue: RuntimeLivePlayerIssue | null;
  current_players?: number | null;
}

export type MockLivePlayerCollector = (context: MockLivePlayerContext) => MockLivePlayerCollection;

const SAFE_ACTION_INPUT_KEYS = ["action_id", "instance_id", "player_key", "snapshot_id"];

function clone<T>(value: T): T {
  return typeof structuredClone === "function"
    ? structuredClone(value)
    : JSON.parse(JSON.stringify(value)) as T;
}

function withoutActions(entries: RuntimeLivePlayerEntry[]): RuntimeLivePlayerEntry[] {
  return clone(entries).map((entry) => ({ ...entry, available_action_ids: [] }));
}

function parseScope(value: string | null): ModulePlayerListScope | null {
  return value === "online" ? value : null;
}

function parseSource(value: string | null): ModulePlayerListSource | null {
  return value === "runtime_action" || value === "structured_log"
    || value === "http_api" || value === "server_query" || value === "console_log"
    || value === "tcp_console" || value === "native_console" || value === "file_ipc" ? value : null;
}

function parseCodec(value: string | null): ModulePlayerListCodec | null {
  switch (value) {
    case "dst_client_table_v1":
    case "rust_player_list":
    case "ark_list_players":
    case "conan_list_players":
    case "humanitz_players":
    case "zomboid_players":
    case "seven_days_players":
    case "squad_list_players":
    case "palworld_players":
    case "minecraft_players":
    case "nightingale_players":
    case "necesse_players":
    case "romestead_players":
    case "terraria_players":
    case "astroneer_players":
    case "soulmask_players":
    case "satisfactory_frm_players":
    case "barotrauma_players":
    case "return_to_moria_players":
    case "windrose_players":
    case "dragonwilds_players":
    case "scum_players":
    case "a2s_players":
      return value;
    default: return null;
  }
}

function parseIdentityKind(value: string | null): RuntimePlayerIdentityKind | null {
  switch (value) {
    case "klei_user_id":
    case "steam_id":
    case "ark_account_id":
    case "conan_user_id":
    case "eos_id":
    case "player_name":
    case "session_id":
    case "palworld_user_id":
    case "minecraft_uuid":
    case "astroneer_guid":
      return value;
    default: return null;
  }
}

export function parseMockPlayerListFromModuleToml(moduleId: string): ModulePlayerListSpec | null {
  const toml = mockModuleTomlById[moduleId];
  const table = toml ? readMockTomlTable(toml, "runtime.player_list") : "";
  if (!table) {
    return null;
  }

  const scope = parseScope(readMockTomlString(table, "scope") ?? "online");
  const source = parseSource(readMockTomlString(table, "source"));
  const responseCodec = parseCodec(readMockTomlString(table, "response_codec"));
  const identityKind = parseIdentityKind(readMockTomlString(table, "identity_kind"));
  if (!scope || !source || !responseCodec || !identityKind) {
    return null;
  }

  return {
    scope,
    source,
    action_id: readMockTomlString(table, "action_id"),
    player_action_ids: readMockTomlStringArray(table, "player_action_ids"),
    response_codec: responseCodec,
    identity_kind: identityKind,
    refresh_interval_ms: readMockTomlInteger(table, "refresh_interval_ms") ?? 30_000
  };
}

function declaredRowActions(context: MockLivePlayerContext): string[] {
  const capability = context.player_list;
  return !capability || capability.source === "server_query" || capability.source === "console_log"
    ? [] : capability.player_action_ids;
}

function sampleEntries(context: MockLivePlayerContext): RuntimeLivePlayerEntry[] {
  const identityKind = context.player_list?.identity_kind ?? "player_name";
  const actionIds = declaredRowActions(context);
  const namesOnly = identityKind === "player_name" && actionIds.length === 0;
  const stableIdentity = identityKind !== "session_id" && identityKind !== "player_name";
  // These are deterministic preview fixtures, never observations from a running server.
  const sampleIdentifiers: Record<RuntimePlayerIdentityKind, [string, string]> = {
    klei_user_id: ["KU_demo_admin", "KU_demo_friend"],
    steam_id: ["76561198000000001", "76561198000000002"],
    ark_account_id: ["1000000000000000001", "1000000000000000002"],
    conan_user_id: ["00000000000000000000000000000001", "00000000000000000000000000000002"],
    eos_id: ["00000000000000000000000000000001", "00000000000000000000000000000002"],
    session_id: ["1", "2"],
    palworld_user_id: ["steam_76561198000000001", "steam_76561198000000002"],
    minecraft_uuid: ["00000000-0000-4000-8000-000000000001", "00000000-0000-4000-8000-000000000002"],
    astroneer_guid: ["00000000-0000-4000-8000-000000000001", "00000000-0000-4000-8000-000000000002"],
    player_name: ["HostAlice", "Bob"]
  };
  const samples: RuntimeLivePlayerEntry[] = [
    {
      player_key: "unbound:1",
      display_name: "HostAlice",
      identifiers: namesOnly ? [] : [{ kind: identityKind, value: sampleIdentifiers[identityKind][0], stable: stableIdentity }],
      available_action_ids: [...actionIds],
      ping_ms: null,
      session_started_at_unix_ms: null,
      role: "admin",
      attributes: identityKind === "klei_user_id" ? [{ key: "prefab", value: "wilson" }] : []
    },
    {
      player_key: "unbound:2",
      display_name: "Bob",
      identifiers: namesOnly ? [] : [{ kind: identityKind, value: sampleIdentifiers[identityKind][1], stable: stableIdentity }],
      available_action_ids: [...actionIds],
      ping_ms: null,
      session_started_at_unix_ms: null,
      role: null,
      attributes: identityKind === "klei_user_id" ? [{ key: "prefab", value: "wendy" }] : []
    }
  ];
  const requestedCount = Math.max(0, Math.floor(context.current_players ?? samples.length));
  return samples.slice(0, requestedCount);
}

function defaultCollector(context: MockLivePlayerContext): MockLivePlayerCollection {
  const entries = sampleEntries(context);
  const requestedCount = Math.max(0, Math.floor(context.current_players ?? entries.length));
  if (requestedCount > entries.length) {
    return {
      status: "failed",
      entries: [],
      complete: false,
      truncated: false,
      issue: {
        code: "protocol_incomplete",
        setting_keys: [],
        summary: "The mock structured player capture did not contain every counted player."
      },
      current_players: requestedCount
    };
  }

  return {
    status: "ready",
    entries,
    complete: true,
    truncated: false,
    issue: null,
    current_players: requestedCount
  };
}

function capabilitySupported(context: MockLivePlayerContext): boolean {
  return context.player_list?.scope === "online"
    && parseSource(context.player_list.source) !== null
    && parseCodec(context.player_list.response_codec) !== null
    && parseIdentityKind(context.player_list.identity_kind) !== null;
}

export class MockLivePlayerStore {
  private readonly snapshots = new Map<string, RuntimeLivePlayerSnapshot>();
  private readonly lastSuccessfulSnapshots = new Map<string, RuntimeLivePlayerSnapshot>();
  private readonly sequenceByInstance = new Map<string, number>();
  private readonly collector: MockLivePlayerCollector;
  private readonly now: () => number;

  constructor(options: { collector?: MockLivePlayerCollector; now?: () => number } = {}) {
    this.collector = options.collector ?? defaultCollector;
    this.now = options.now ?? Date.now;
  }

  read(context: MockLivePlayerContext): RuntimeLivePlayerSnapshot {
    const capabilityState = this.readCapabilityState(context);
    if (capabilityState) {
      return capabilityState;
    }

    const cached = this.snapshots.get(context.instance_id);
    if (cached) {
      if (
        cached.status === "ready"
        && cached.expires_at_unix_ms !== null
        && this.now() >= cached.expires_at_unix_ms
      ) {
        return {
          ...clone(cached),
          stale: true,
          entries: withoutActions(cached.entries)
        };
      }
      return clone(cached);
    }
    return this.buildSnapshot(context, {
      status: "refreshing",
      observedAt: null,
      expiresAt: null,
      complete: false,
      truncated: false,
      stale: false,
      currentPlayers: context.current_players,
      entries: [],
      issue: null
    });
  }

  refresh(context: MockLivePlayerContext): RuntimeLivePlayerSnapshot {
    const capabilityState = this.readCapabilityState(context);
    if (capabilityState) {
      return capabilityState;
    }

    const collected = this.collector(clone(context));
    const observedAt = this.nextObservedAt(context.instance_id);
    const completeSuccess = collected.status === "ready" && collected.complete && !collected.truncated;
    if (!completeSuccess) {
      const previous = this.lastSuccessfulSnapshots.get(context.instance_id);
      const issue = collected.issue ?? {
        code: collected.truncated ? "capture_limit" : "protocol_incomplete",
        setting_keys: [],
        summary: collected.truncated
          ? "The mock player capture reached its bounded size limit."
          : "The mock player capture was incomplete."
      };
      const failed = this.buildSnapshot(context, {
        status: "failed",
        observedAt,
        expiresAt: observedAt + (context.player_list?.refresh_interval_ms ?? 30_000),
        complete: false,
        truncated: collected.truncated,
        stale: Boolean(previous),
        currentPlayers: collected.current_players ?? context.current_players,
        entries: withoutActions(previous?.entries ?? []),
        issue
      });
      this.snapshots.set(context.instance_id, clone(failed));
      return clone(failed);
    }

    const ready = this.buildSnapshot(context, {
      status: "ready",
      observedAt,
      expiresAt: observedAt + (context.player_list?.refresh_interval_ms ?? 30_000),
      complete: true,
      truncated: false,
      stale: false,
      currentPlayers: collected.current_players ?? collected.entries.length,
      entries: collected.entries,
      issue: null
    });
    ready.entries = ready.entries.map((entry, index) => ({
      ...entry,
      player_key: `player:${ready.snapshot_id}:${index + 1}`,
      available_action_ids: entry.available_action_ids.filter((id) => declaredRowActions(context).includes(id))
    }));
    this.snapshots.set(context.instance_id, clone(ready));
    this.lastSuccessfulSnapshots.set(context.instance_id, clone(ready));
    return clone(ready);
  }

  execute(
    context: MockLivePlayerContext,
    input: ExecuteInstancePlayerActionInput & Record<string, unknown>
  ): ExecuteInstancePlayerActionResult {
    const actualKeys = Object.keys(input).sort();
    if (actualKeys.length !== SAFE_ACTION_INPUT_KEYS.length
      || actualKeys.some((key, index) => key !== SAFE_ACTION_INPUT_KEYS[index])) {
      throw new Error("Live-player actions accept only the declared four-field input.");
    }
    if (input.instance_id !== context.instance_id) {
      throw new Error("The live-player action instance does not match the selected server.");
    }
    if (!context.running || !capabilitySupported(context)) {
      throw new Error("Live-player actions are unavailable for this server.");
    }

    const snapshot = this.snapshots.get(context.instance_id);
    if (!snapshot || snapshot.snapshot_id !== input.snapshot_id) {
      throw new Error("The live-player snapshot is no longer current.");
    }
    if (snapshot.status !== "ready" || !snapshot.complete || snapshot.stale || snapshot.truncated) {
      throw new Error("The live-player snapshot is not authoritative.");
    }
    if (snapshot.expires_at_unix_ms === null || this.now() >= snapshot.expires_at_unix_ms) {
      throw new Error("The live-player snapshot is expired.");
    }
    const player = snapshot.entries.find((entry) => entry.player_key === input.player_key);
    if (!player) {
      throw new Error("The selected player is no longer present.");
    }
    const declaredActions = context.player_list?.player_action_ids ?? [];
    if (!declaredActions.includes(input.action_id) || !player.available_action_ids.includes(input.action_id)) {
      throw new Error("The selected action is not declared for this player row.");
    }

    const executedAt = this.nextObservedAt(context.instance_id);
    this.lastSuccessfulSnapshots.delete(context.instance_id);
    this.snapshots.set(context.instance_id, {
      ...clone(snapshot),
      snapshot_id: this.nextSnapshotId(context.instance_id),
      status: "refreshing",
      expires_at_unix_ms: null,
      complete: false,
      stale: true,
      entries: [],
      issue: null
    });
    return {
      action_id: input.action_id,
      status: "sent",
      executed_at_unix_ms: executedAt,
      summary: "The declared player action was accepted."
    };
  }

  delete(instanceId: string): void {
    this.snapshots.delete(instanceId);
    this.lastSuccessfulSnapshots.delete(instanceId);
    this.sequenceByInstance.delete(instanceId);
  }

  private readCapabilityState(context: MockLivePlayerContext): RuntimeLivePlayerSnapshot | null {
    if (!capabilitySupported(context)) {
      this.snapshots.delete(context.instance_id);
      this.lastSuccessfulSnapshots.delete(context.instance_id);
      return this.buildSnapshot(context, {
        status: "unsupported",
        observedAt: null,
        expiresAt: null,
        complete: false,
        truncated: false,
        stale: false,
        currentPlayers: context.current_players,
        entries: [],
        issue: { code: "adapter_unavailable", setting_keys: [], summary: "No online player-list adapter is declared by this module." },
        source: null
      });
    }
    if (!context.running) {
      this.snapshots.delete(context.instance_id);
      this.lastSuccessfulSnapshots.delete(context.instance_id);
      return this.buildSnapshot(context, {
        status: "stopped",
        observedAt: null,
        expiresAt: null,
        complete: false,
        truncated: false,
        stale: false,
        currentPlayers: null,
        entries: [],
        issue: null
      });
    }
    const visibilityIssue = mockSteamQueryVisibilityIssue(context.module_id, context.settings);
    if (visibilityIssue) {
      this.snapshots.delete(context.instance_id);
      this.lastSuccessfulSnapshots.delete(context.instance_id);
      return this.buildSnapshot(context, {
        status: "unsupported", observedAt: null, expiresAt: null,
        complete: false, truncated: false, stale: false,
        currentPlayers: null, entries: [], issue: visibilityIssue
      });
    }
    return null;
  }

  private buildSnapshot(
    context: MockLivePlayerContext,
    state: {
      status: RuntimeLivePlayerSnapshot["status"];
      observedAt: number | null;
      expiresAt: number | null;
      complete: boolean;
      truncated: boolean;
      stale: boolean;
      currentPlayers: number | null;
      entries: RuntimeLivePlayerEntry[];
      issue: RuntimeLivePlayerIssue | null;
      source?: ModulePlayerListSource | null;
    }
  ): RuntimeLivePlayerSnapshot {
    return {
      snapshot_id: this.nextSnapshotId(context.instance_id),
      instance_id: context.instance_id,
      status: state.status,
      source: state.source === undefined ? context.player_list?.source ?? null : state.source,
      observed_at_unix_ms: state.observedAt,
      expires_at_unix_ms: state.expiresAt,
      complete: state.complete,
      truncated: state.truncated,
      stale: state.stale,
      current_players: state.currentPlayers,
      max_players: context.max_players,
      entries: clone(state.entries),
      issue: clone(state.issue)
    };
  }

  private nextSnapshotId(instanceId: string): string {
    const next = (this.sequenceByInstance.get(instanceId) ?? 0) + 1;
    this.sequenceByInstance.set(instanceId, next);
    return `${instanceId}:mock-live-players:${next}`;
  }

  private nextObservedAt(instanceId: string): number {
    const previous = this.snapshots.get(instanceId)?.observed_at_unix_ms ?? 0;
    return Math.max(this.now(), previous + 1);
  }
}
