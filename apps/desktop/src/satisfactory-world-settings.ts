import { invokeOrMock } from "./api-transport";
import nativeCatalog from "../../../modules/satisfactory/world-settings.json";

export interface SatisfactoryRuleOption { value: string; label_key: string }
export interface SatisfactoryRuleDefinition {
  key: string;
  scope: "creation" | "world" | "player_defaults";
  kind: "boolean" | "integer" | "select";
  default_value: string;
  options: SatisfactoryRuleOption[];
  minimum: number | null;
  maximum: number | null;
}
export interface SatisfactorySave {
  save_name: string; save_date_time: string; play_duration_seconds: number; is_creative_mode_enabled: boolean;
}
export interface SatisfactorySession { session_name: string; saves: SatisfactorySave[] }
export interface SatisfactoryWorldSnapshot {
  instance_id: string;
  connection_status: "stopped" | "unclaimed" | "authorization_required" | "ready";
  revision: string;
  server_name: string | null;
  active_session_name: string;
  auto_load_session_name: string;
  is_game_running: boolean;
  connected_players: number;
  creative_mode_enabled: boolean;
  advanced_game_settings: Record<string, string>;
  server_options: Record<string, string>;
  pending_server_options: Record<string, string>;
  sessions: SatisfactorySession[];
  rule_definitions: SatisfactoryRuleDefinition[];
  starting_locations: SatisfactoryRuleOption[];
}
export interface SatisfactoryWorldOperationResult { instance_id: string; accepted: boolean; session_name: string }
export interface SetupSatisfactoryServerInput { instance_id: string; server_name: string; admin_password: string | null }
export interface AuthorizeSatisfactoryServerInput { instance_id: string; admin_password: string | null }
export interface WriteSatisfactoryRoomInput {
  instance_id: string; expected_revision: string;
  server_name: string | null; client_password: string | null; auto_load_session_name: string | null;
}
export interface WriteSatisfactoryWorldRulesInput {
  instance_id: string; expected_revision: string;
  acknowledge_enable_advanced_settings: boolean; advanced_game_settings: Record<string, string>;
}
export interface CreateSatisfactoryWorldInput {
  instance_id: string; expected_revision: string; session_name: string; starting_location: string;
  skip_onboarding: boolean;
  acknowledge_enable_advanced_settings: boolean;
  game_mode_settings: Record<string, string>; advanced_game_settings: Record<string, string>;
}
export interface LoadSatisfactorySaveInput { instance_id: string; expected_revision: string; save_name: string }

export const readSatisfactoryWorldSettings = (instanceId: string) =>
  invokeOrMock<SatisfactoryWorldSnapshot>("read_satisfactory_world_settings", { instanceId });
export const setupSatisfactoryServer = (input: SetupSatisfactoryServerInput) =>
  invokeOrMock<SatisfactoryWorldSnapshot>("setup_satisfactory_server", { input });
export const authorizeSatisfactoryServer = (input: AuthorizeSatisfactoryServerInput) =>
  invokeOrMock<SatisfactoryWorldSnapshot>("authorize_satisfactory_server", { input });
export const writeSatisfactoryRoom = (input: WriteSatisfactoryRoomInput) =>
  invokeOrMock<SatisfactoryWorldSnapshot>("write_satisfactory_room", { input });
export const writeSatisfactoryWorldRules = (input: WriteSatisfactoryWorldRulesInput) =>
  invokeOrMock<SatisfactoryWorldSnapshot>("write_satisfactory_world_rules", { input });
export const createSatisfactoryWorld = (input: CreateSatisfactoryWorldInput) =>
  invokeOrMock<SatisfactoryWorldOperationResult>("create_satisfactory_world", { input });
export const loadSatisfactorySave = (input: LoadSatisfactorySaveInput) =>
  invokeOrMock<SatisfactoryWorldOperationResult>("load_satisfactory_save", { input });
export const readSatisfactoryAdminPassword = (instanceId: string) =>
  invokeOrMock<string | null>("read_satisfactory_admin_password", { instanceId });

export const SATISFACTORY_RULES: readonly SatisfactoryRuleDefinition[] = nativeCatalog.settings.map((entry) => {
  if ((entry.scope !== "creation" && entry.scope !== "world" && entry.scope !== "player_defaults") ||
    (entry.kind !== "boolean" && entry.kind !== "integer" && entry.kind !== "select")) {
    throw new Error(`Unsupported Satisfactory rule definition: ${entry.key}`);
  }
  return { ...entry, scope: entry.scope, kind: entry.kind,
    options: "options" in entry ? entry.options ?? [] : [],
    minimum: "minimum" in entry ? entry.minimum ?? null : null, maximum: "maximum" in entry ? entry.maximum ?? null : null };
});
export const SATISFACTORY_STARTING_LOCATIONS: readonly SatisfactoryRuleOption[] = nativeCatalog.starting_locations;
export const satisfactoryRuleId = (key: string) => `satisfactory_${key.replace(/\./gu, "_")}`;
export const satisfactoryRuleCopyKey = (key: string) => key.slice(key.lastIndexOf(".") + 1);

export function isSatisfactoryRuleValueValid(rule: SatisfactoryRuleDefinition, value: string): boolean {
  if (rule.kind === "boolean") return value === "True" || value === "False";
  if (rule.kind === "select") return rule.options.some((option) => option.value === value);
  if (!/^-?\d+$/u.test(value)) return false;
  const number = Number(value);
  return Number.isSafeInteger(number) && (rule.minimum === null || number >= rule.minimum) &&
    (rule.maximum === null || number <= rule.maximum);
}

export function splitSatisfactoryCreationRules(values: Readonly<Record<string, string>>) {
  const game_mode_settings: Record<string, string> = {};
  const advanced_game_settings: Record<string, string> = {};
  for (const rule of SATISFACTORY_RULES.filter((entry) => entry.scope === "creation")) {
    const value = values[rule.key] ?? rule.default_value;
    if (!isSatisfactoryRuleValueValid(rule, value)) throw new Error(`Invalid Satisfactory rule: ${rule.key}`);
    if (value === rule.default_value) continue;
    (rule.key.startsWith("FG.GameMode.") ? game_mode_settings : advanced_game_settings)[rule.key] = value;
  }
  return { game_mode_settings, advanced_game_settings };
}

export function buildSatisfactoryRulesPatch(snapshot: SatisfactoryWorldSnapshot, draft: Readonly<Record<string, string>>) {
  const patch: Record<string, string> = {};
  for (const [key, value] of Object.entries(draft)) {
    const rule = SATISFACTORY_RULES.find((entry) => entry.key === key && entry.scope !== "creation");
    if (!rule || !isSatisfactoryRuleValueValid(rule, value)) throw new Error(`Invalid Satisfactory rule: ${key}`);
    if (value !== (snapshot.advanced_game_settings[key] ?? rule.default_value)) patch[key] = value;
  }
  return patch;
}
