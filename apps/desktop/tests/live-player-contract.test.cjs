const ts = require("@typescript/typescript6");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const {
  parseSource,
  sourceText,
  visitSyntax
} = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");
const typesPath = path.join(desktopRoot, "src", "types.ts");
const apiPath = path.join(desktopRoot, "src", "api.ts");
const mockModuleDetailsPath = path.join(desktopRoot, "src", "api-mock", "module-details.ts");
const livePlayerMockPath = path.join(desktopRoot, "src", "api-mock", "live-players.ts");
const typesSource = fs.readFileSync(typesPath, "utf8");
const apiSource = fs.readFileSync(apiPath, "utf8");
const mockModuleDetailsSource = fs.readFileSync(mockModuleDetailsPath, "utf8");
const livePlayerMockSource = fs.readFileSync(livePlayerMockPath, "utf8");
const rustLivePlayerSource = fs.readFileSync(path.resolve(desktopRoot, "../../crates/app-core/src/live_players.rs"), "utf8");
const typesFile = parseSource(typesSource, typesPath);
const apiFile = parseSource(apiSource, apiPath);

function findDeclaration(sourceFile, predicate, description) {
  let found = null;
  visitSyntax(sourceFile, (node) => {
    if (!found && predicate(node)) {
      found = node;
      return false;
    }
  });
  assert.ok(found, `${description} is missing`);
  return found;
}

function typeAliasValues(name) {
  const declaration = findDeclaration(
    typesFile,
    (node) => ts.isTypeAliasDeclaration(node) && node.name.text === name,
    `type ${name}`
  );
  const type = declaration.type;
  const members = ts.isUnionTypeNode(type) ? type.types : [type];
  return members.map((member) => {
    assert.ok(ts.isLiteralTypeNode(member), `${name} must contain literal types only`);
    assert.ok(ts.isStringLiteral(member.literal), `${name} must contain string literals only`);
    return member.literal.text;
  });
}

function interfaceProperties(name) {
  const declaration = findDeclaration(
    typesFile,
    (node) => ts.isInterfaceDeclaration(node) && node.name.text === name,
    `interface ${name}`
  );
  return declaration.members.map((member) => {
    assert.ok(ts.isPropertySignature(member), `${name} may contain properties only`);
    assert.ok(member.name, `${name} property must have a key`);
    return member.name.text;
  });
}

function rustEnumJsonValues(name) {
  const declaration = rustLivePlayerSource.match(new RegExp(`pub enum ${name} \\{([^}]+)\\}`));
  assert.ok(declaration, `Rust enum ${name} is missing`);
  return declaration[1].split(",").map((variant) => variant.trim()).filter(Boolean)
    .map((variant) => variant.replace(/([a-z0-9])([A-Z])/g, "$1_$2").toLowerCase());
}

function apiFunctionSource(name) {
  const declaration = findDeclaration(
    apiFile,
    (node) => ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.name.text === name,
    `API ${name}`
  );
  return sourceText(apiSource, declaration);
}

test("TypeScript live-player unions mirror the Rust JSON vocabulary", () => {
  assert.deepEqual(typeAliasValues("ModulePlayerListScope"), ["online"]);
  assert.deepEqual(typeAliasValues("ModulePlayerListSource"), ["runtime_action", "structured_log", "http_api", "server_query", "console_log", "tcp_console", "native_console", "file_ipc"]);
  assert.deepEqual(typeAliasValues("ModulePlayerListCodec"), [
    "dst_client_table_v1", "rust_player_list", "ark_list_players", "conan_list_players",
    "humanitz_players", "zomboid_players", "seven_days_players",
    "squad_list_players", "palworld_players", "minecraft_players", "nightingale_players", "necesse_players", "romestead_players", "terraria_players", "astroneer_players", "soulmask_players", "satisfactory_frm_players", "barotrauma_players", "return_to_moria_players", "windrose_players", "dragonwilds_players", "scum_players", "enshrouded_native_players", "a2s_players"
  ]);
  assert.deepEqual(typeAliasValues("RuntimePlayerIdentityKind"), [
    "klei_user_id", "steam_id", "ark_account_id", "conan_user_id", "eos_id", "player_name", "session_id",
    "palworld_user_id", "minecraft_uuid", "astroneer_guid", "enshrouded_account_id", "valheim_platform_id"
  ]);
  assert.deepEqual(typeAliasValues("RuntimeLivePlayerStatus"), [
    "ready",
    "refreshing",
    "stopped",
    "unsupported",
    "misconfigured",
    "failed"
  ]);
  assert.deepEqual(typeAliasValues("RuntimeLivePlayerIssueCode"), [
    "process_unavailable",
    "process_untracked",
    "log_unavailable",
    "collection_timeout",
    "protocol_incomplete",
    "capture_limit",
    "io_failed",
    "runtime_action_unavailable",
    "adapter_unavailable",
    "names_unavailable",
    "query_unavailable",
    "authentication_failed",
    "extension_unavailable"
  ]);
  assert.deepEqual(typeAliasValues("RuntimeLivePlayerActionStatus"), ["sent"]);
});

test("snapshot, row, issue, and action DTO fields match the public Rust contract", () => {
  assert.deepEqual(interfaceProperties("RuntimeLivePlayerSnapshot"), [
    "snapshot_id",
    "instance_id",
    "status",
    "source",
    "observed_at_unix_ms",
    "expires_at_unix_ms",
    "complete",
    "truncated",
    "stale",
    "current_players",
    "max_players",
    "entries",
    "issue"
  ]);
  assert.deepEqual(interfaceProperties("RuntimeLivePlayerEntry"), [
    "player_key",
    "display_name",
    "identifiers",
    "available_action_ids",
    "ping_ms",
    "session_started_at_unix_ms",
    "role",
    "attributes"
  ]);
  assert.deepEqual(interfaceProperties("RuntimeLivePlayerIdentifier"), ["kind", "value", "stable"]);
  assert.deepEqual(interfaceProperties("RuntimeLivePlayerAttribute"), ["key", "value"]);
  assert.deepEqual(interfaceProperties("RuntimeLivePlayerIssue"), ["code", "setting_keys", "summary"]);
  assert.deepEqual(interfaceProperties("ExecuteInstancePlayerActionInput"), [
    "instance_id",
    "snapshot_id",
    "player_key",
    "action_id"
  ]);
  assert.deepEqual(interfaceProperties("ExecuteInstancePlayerActionResult"), [
    "action_id",
    "status",
    "executed_at_unix_ms",
    "summary"
  ]);

  for (const forbidden of [
    "target",
    "command",
    "transport",
    "portName",
    "passwordSettingKey",
    "enabledSettingKey",
    "raw_response"
  ]) {
    assert.ok(!interfaceProperties("ExecuteInstancePlayerActionInput").includes(forbidden));
  }
});

test("native extension codec and account namespace match the Rust enums without adding a source", () => {
  for (const name of ["ModulePlayerListSource", "ModulePlayerListCodec", "RuntimePlayerIdentityKind"]) {
    assert.deepEqual(typeAliasValues(name).sort(), rustEnumJsonValues(name).sort(), `${name} must agree across the IPC boundary`);
  }
  assert.ok(typeAliasValues("ModulePlayerListCodec").includes("enshrouded_native_players"));
  assert.ok(typeAliasValues("RuntimePlayerIdentityKind").includes("enshrouded_account_id"));
  assert.ok(typeAliasValues("ModulePlayerListSource").includes("file_ipc"));
  assert.equal(typeAliasValues("ModulePlayerListSource").length, 8);
});

test("declared manual player actions keep backend descriptors authoritative", () => {
  const manual = apiFunctionSource("executeDeclaredRuntimePlayerAction");
  assert.match(manual, /"execute_instance_manual_player_action"/);
  assert.match(manual, /instance_id:\s*instanceId/);
  assert.match(manual, /action_id:\s*actionId/);
  assert.match(manual, /target,/);
  assert.match(manual, /role:\s*role\s*\|\|\s*null/);
  assert.doesNotMatch(manual, /command_template|transport|portName|processKey|passwordSettingKey|enabledSettingKey/);
});

test("mock module capabilities project runtime.player_list from the module TOML", () => {
  assert.match(livePlayerMockSource, /readMockTomlTable\(toml, "runtime\.player_list"\)/);
  assert.match(livePlayerMockSource, /export function parseMockPlayerListFromModuleToml\(moduleId: string\)/);
  assert.match(mockModuleDetailsSource, /player_list:\s*parseMockPlayerListFromModuleToml\(summary\.id\)/);
  const parserStart = livePlayerMockSource.indexOf("export function parseMockPlayerListFromModuleToml");
  const parserEnd = livePlayerMockSource.indexOf("\n}\n", parserStart) + 3;
  const parser = livePlayerMockSource.slice(parserStart, parserEnd);
  assert.doesNotMatch(parser, /dontstarve|minecraft|moduleId\s*===/);
});
