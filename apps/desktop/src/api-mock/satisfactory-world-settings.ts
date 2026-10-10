import catalog from "../../../../modules/satisfactory/world-settings.json";
import type { InstanceDetails } from "../types";
import type {
  AuthorizeSatisfactoryServerInput, CreateSatisfactoryWorldInput, LoadSatisfactorySaveInput,
  SatisfactoryRuleDefinition, SatisfactorySave, SatisfactoryWorldOperationResult,
  SatisfactoryWorldSnapshot, SetupSatisfactoryServerInput, WriteSatisfactoryRoomInput,
  WriteSatisfactoryWorldRulesInput
} from "../satisfactory-world-settings";

const definitions: SatisfactoryRuleDefinition[] = catalog.settings.map((entry) => {
  const scope = entry.scope;
  const kind = entry.kind;
  if ((scope !== "creation" && scope !== "world" && scope !== "player_defaults") ||
    (kind !== "boolean" && kind !== "integer" && kind !== "select")) {
    throw new Error("The Satisfactory preview catalog is unsupported.");
  }
  return { ...entry, scope, kind, options: "options" in entry ? entry.options ?? [] : [],
    minimum: "minimum" in entry ? entry.minimum ?? null : null,
    maximum: "maximum" in entry ? entry.maximum ?? null : null };
});

const defaultAdvanced = () => Object.fromEntries(definitions
  .filter((rule) => !rule.key.startsWith("FG.GameMode."))
  .map((rule) => [rule.key, rule.default_value]));

interface SavedWorld {
  session_name: string;
  creative_mode_enabled: boolean;
  advanced_game_settings: Record<string, string>;
  game_mode_settings: Record<string, string>;
}

interface MockServer {
  snapshot: SatisfactoryWorldSnapshot;
  claimed: boolean;
  authorized: boolean;
  admin_password: string | null;
  client_password: string | null;
  version: number;
  save_sequence: number;
  saved_worlds: Map<string, SavedWorld>;
  current_game_mode_settings: Record<string, string>;
  pending: SavedWorld | null;
}

function empty(instanceId: string): SatisfactoryWorldSnapshot {
  return { instance_id: instanceId, connection_status: "unclaimed", revision: "",
    server_name: null, active_session_name: "", auto_load_session_name: "",
    is_game_running: false, connected_players: 0, creative_mode_enabled: false,
    advanced_game_settings: {}, server_options: {}, pending_server_options: {}, sessions: [],
    rule_definitions: structuredClone(definitions), starting_locations: structuredClone(catalog.starting_locations) };
}

function validateText(value: string, allowEmpty = false) {
  if ([...value].length > 128 || /[\p{Cc}]/u.test(value) || (!allowEmpty && !value.trim())) {
    throw new Error("A name or password is empty, too long, or contains control characters.");
  }
}

function validateRules(values: Record<string, string>, creation: boolean, gameMode: boolean) {
  const entries = Object.entries(values);
  if (entries.length > 32) throw new Error("The settings patch exceeds the supported limits.");
  const result: Record<string, string> = {};
  for (const [key, value] of entries) {
    const rule = definitions.find((entry) => entry.key === key &&
      (creation || entry.scope !== "creation") && key.startsWith("FG.GameMode.") === gameMode);
    if (!rule || key.length > 128 || typeof value !== "string" || value.length > 64) {
      throw new Error("The selected Satisfactory setting is unsupported.");
    }
    if (rule.kind === "boolean" && /^(true|false)$/iu.test(value)) {
      result[key] = value.toLowerCase() === "true" ? "True" : "False";
    } else if (rule.kind === "select" && rule.options.some((option) => option.value === value)) {
      result[key] = value;
    } else if (rule.kind === "integer" && /^-?\d+$/u.test(value)) {
      const number = Number(value);
      if (!Number.isSafeInteger(number) || String(number) !== value ||
        (rule.minimum !== null && number < rule.minimum) || (rule.maximum !== null && number > rule.maximum)) {
        throw new Error("The selected Satisfactory integer is invalid.");
      }
      result[key] = value;
    } else throw new Error("The selected Satisfactory value is unsupported.");
  }
  return result;
}

/** Development-only external API boundary; no native credentials or files are used. */
export class MockSatisfactoryWorldSettings {
  private readonly servers = new Map<string, MockServer>();

  /** Explicit browser-demo fixture, never automatic evidence of an installed world. */
  seed(snapshot: SatisfactoryWorldSnapshot): void {
    const current = structuredClone(snapshot);
    current.rule_definitions = structuredClone(definitions);
    current.starting_locations = structuredClone(catalog.starting_locations);
    const claimed = snapshot.connection_status === "ready" || snapshot.connection_status === "authorization_required";
    const state: MockServer = { snapshot: current,
      claimed, authorized: snapshot.connection_status === "ready",
      admin_password: claimed ? "MOCK_ONLY_demo_admin_password" : null,
      client_password: null, version: 0, save_sequence: 0, saved_worlds: new Map(),
      current_game_mode_settings: {}, pending: null };
    for (const session of current.sessions) {
      for (const save of session.saves) {
        state.saved_worlds.set(save.save_name, { session_name: session.session_name,
          creative_mode_enabled: save.is_creative_mode_enabled,
          advanced_game_settings: session.session_name === current.active_session_name &&
            save.is_creative_mode_enabled === current.creative_mode_enabled
            ? structuredClone(current.advanced_game_settings) : defaultAdvanced(), game_mode_settings: {} });
      }
    }
    this.servers.set(snapshot.instance_id, state);
    this.bump(state);
  }

  private server(details: InstanceDetails): MockServer {
    if (details.summary.module_id !== "satisfactory") throw new Error("Not a Satisfactory instance.");
    let state = this.servers.get(details.summary.id);
    if (!state) {
      state = { snapshot: empty(details.summary.id), claimed: false, authorized: false,
        admin_password: null, client_password: null, version: 0, save_sequence: 0,
        saved_worlds: new Map(), current_game_mode_settings: {}, pending: null };
      this.servers.set(details.summary.id, state);
      this.bump(state);
    }
    return state;
  }

  private bump(state: MockServer) {
    state.version += 1;
    state.snapshot.revision = `mock-satisfactory-${state.snapshot.instance_id}-${state.version}`;
  }

  private running(details: InstanceDetails) {
    if (details.summary.status !== "Running" && details.summary.status !== "Starting") {
      throw new Error("Start the Satisfactory server before using its world controls.");
    }
  }

  private ready(details: InstanceDetails, instanceId: string, expectedRevision: string): MockServer {
    const state = this.server(details);
    this.running(details);
    if (instanceId !== details.summary.id) throw new Error("The Satisfactory instance context changed.");
    if (!state.claimed || !state.authorized) throw new Error("Authorize Satisfactory server management before continuing.");
    if (state.pending) throw new Error("The Satisfactory world is loading. Refresh before continuing.");
    if (!expectedRevision || expectedRevision !== state.snapshot.revision) {
      throw new Error("The Satisfactory world changed. Refresh before saving.");
    }
    return state;
  }

  read(details: InstanceDetails): SatisfactoryWorldSnapshot {
    const state = this.server(details);
    if (details.summary.status !== "Running" && details.summary.status !== "Starting") {
      return { ...empty(details.summary.id), connection_status: "stopped" };
    }
    if (state.pending) {
      const next = state.pending;
      state.snapshot.active_session_name = next.session_name;
      state.snapshot.is_game_running = true;
      state.snapshot.creative_mode_enabled = next.creative_mode_enabled;
      state.snapshot.advanced_game_settings = structuredClone(next.advanced_game_settings);
      state.current_game_mode_settings = structuredClone(next.game_mode_settings);
      state.pending = null;
    }
    if (!state.claimed || !state.authorized) {
      return { ...empty(details.summary.id), server_name: state.snapshot.server_name,
        connection_status: state.claimed ? "authorization_required" : "unclaimed" };
    }
    return structuredClone({ ...state.snapshot, connection_status: "ready" });
  }

  setup(details: InstanceDetails, input: SetupSatisfactoryServerInput): SatisfactoryWorldSnapshot {
    const state = this.server(details);
    this.running(details);
    if (input.instance_id !== details.summary.id || state.claimed) throw new Error("This server cannot be claimed again.");
    validateText(input.server_name);
    const password = input.admin_password ?? `MOCK_ONLY_generated_admin_password_${state.version}`;
    validateText(password);
    state.admin_password = password;
    state.snapshot.server_name = input.server_name;
    state.snapshot.advanced_game_settings = defaultAdvanced();
    state.claimed = true;
    state.authorized = true;
    this.bump(state);
    return this.read(details);
  }

  authorize(details: InstanceDetails, input: AuthorizeSatisfactoryServerInput): SatisfactoryWorldSnapshot {
    const state = this.server(details);
    this.running(details);
    if (input.instance_id !== details.summary.id || !state.claimed) throw new Error("Claim the Satisfactory server first.");
    const password = input.admin_password ?? state.admin_password;
    if (password === null || password !== state.admin_password) throw new Error("The preview administrator password is incorrect.");
    state.authorized = true;
    return this.read(details);
  }

  readAdminPassword(details: InstanceDetails): string | null {
    return this.server(details).admin_password;
  }

  room(details: InstanceDetails, input: WriteSatisfactoryRoomInput): SatisfactoryWorldSnapshot {
    const state = this.ready(details, input.instance_id, input.expected_revision);
    if (input.server_name !== null) validateText(input.server_name);
    if (input.client_password !== null) validateText(input.client_password, true);
    if (input.auto_load_session_name !== null && input.auto_load_session_name !== "" &&
      !state.snapshot.sessions.some((session) => session.session_name === input.auto_load_session_name)) {
      throw new Error("The selected startup world is no longer in the native save collection.");
    }
    if (input.server_name !== null) state.snapshot.server_name = input.server_name;
    if (input.client_password !== null) state.client_password = input.client_password;
    if (input.auto_load_session_name !== null) state.snapshot.auto_load_session_name = input.auto_load_session_name;
    if (input.server_name !== null || input.client_password !== null || input.auto_load_session_name !== null) this.bump(state);
    return this.read(details);
  }

  writeRules(details: InstanceDetails, input: WriteSatisfactoryWorldRulesInput): SatisfactoryWorldSnapshot {
    const patch = validateRules(input.advanced_game_settings, false, false);
    const state = this.ready(details, input.instance_id, input.expected_revision);
    if (!state.snapshot.is_game_running) throw new Error("Load a Satisfactory world before changing its rules.");
    if (Object.entries(patch).every(([key, value]) => state.snapshot.advanced_game_settings[key] === value)) return this.read(details);
    if (!state.snapshot.creative_mode_enabled && !input.acknowledge_enable_advanced_settings) {
      throw new Error("Confirm enabling Advanced Game Settings permanently for this world.");
    }
    if (Object.keys(patch).some((key) => !Object.prototype.hasOwnProperty.call(state.snapshot.advanced_game_settings, key))) {
      throw new Error("The running Satisfactory build does not provide one of the selected world rules.");
    }
    this.saveCurrent(state);
    state.snapshot.advanced_game_settings = { ...state.snapshot.advanced_game_settings, ...patch };
    state.snapshot.creative_mode_enabled = true;
    this.bump(state);
    return this.read(details);
  }

  private noPlayers(state: MockServer) {
    if (state.snapshot.connected_players > 0) throw new Error("Wait until all players disconnect before creating or loading a world.");
  }

  private saveCurrent(state: MockServer) {
    if (!state.snapshot.is_game_running) return;
    const world: SavedWorld = { session_name: state.snapshot.active_session_name,
      creative_mode_enabled: state.snapshot.creative_mode_enabled,
      advanced_game_settings: structuredClone(state.snapshot.advanced_game_settings),
      game_mode_settings: structuredClone(state.current_game_mode_settings) };
    this.addSave(state, world);
  }

  private addSave(state: MockServer, world: SavedWorld) {
    let session = state.snapshot.sessions.find((entry) => entry.session_name === world.session_name);
    if (!session) { session = { session_name: world.session_name, saves: [] }; state.snapshot.sessions.push(session); }
    let name: string;
    do { state.save_sequence += 1; name = `MOCK_save_${state.save_sequence}`; } while (state.saved_worlds.has(name));
    const save: SatisfactorySave = { save_name: name, save_date_time: new Date().toISOString(),
      play_duration_seconds: 0, is_creative_mode_enabled: world.creative_mode_enabled };
    session.saves.unshift(save);
    state.saved_worlds.set(name, structuredClone(world));
  }

  create(details: InstanceDetails, input: CreateSatisfactoryWorldInput): SatisfactoryWorldOperationResult {
    validateText(input.session_name);
    if (input.session_name.trim() !== input.session_name || /[?#/\\:*"<>|]/u.test(input.session_name)) {
      throw new Error("The world name contains characters that cannot be used in a saved session.");
    }
    if (!input.skip_onboarding || !catalog.starting_locations.some((option) => option.value === input.starting_location)) {
      throw new Error("The dedicated-server starting location or onboarding choice is unsupported.");
    }
    const gameMode = validateRules(input.game_mode_settings, true, true);
    const advanced = validateRules(input.advanced_game_settings, true, false);
    for (const rule of definitions) if (advanced[rule.key] === rule.default_value) delete advanced[rule.key];
    if (Object.keys(advanced).length > 0 && !input.acknowledge_enable_advanced_settings) {
      throw new Error("Confirm enabling Advanced Game Settings permanently for the new world.");
    }
    const state = this.ready(details, input.instance_id, input.expected_revision);
    this.noPlayers(state);
    if (state.snapshot.sessions.some((session) => session.session_name.toLowerCase() === input.session_name.toLowerCase()) ||
      (state.snapshot.is_game_running && state.snapshot.active_session_name.toLowerCase() === input.session_name.toLowerCase())) {
      throw new Error("A Satisfactory world already uses that name. Choose a new name.");
    }
    this.saveCurrent(state);
    const next: SavedWorld = { session_name: input.session_name,
      creative_mode_enabled: Object.keys(advanced).length > 0,
      advanced_game_settings: { ...defaultAdvanced(), ...advanced }, game_mode_settings: gameMode };
    this.addSave(state, next);
    state.pending = next;
    this.bump(state);
    return { instance_id: input.instance_id, accepted: true, session_name: input.session_name };
  }

  load(details: InstanceDetails, input: LoadSatisfactorySaveInput): SatisfactoryWorldOperationResult {
    const state = this.ready(details, input.instance_id, input.expected_revision);
    this.noPlayers(state);
    const next = state.saved_worlds.get(input.save_name);
    if (!next) throw new Error("The selected Satisfactory save is no longer in the native save collection.");
    this.saveCurrent(state);
    state.pending = structuredClone(next);
    this.bump(state);
    return { instance_id: input.instance_id, accepted: true, session_name: next.session_name };
  }
}
