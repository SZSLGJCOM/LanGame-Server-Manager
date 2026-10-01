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
    (node) => node.type === "TsTypeAliasDeclaration" && node.id.value === name,
    `type ${name}`
  );
  const type = declaration.typeAnnotation;
  const members = type.type === "TsUnionType" ? type.types : [type];
  return members.map((member) => {
    assert.equal(member.type, "TsLiteralType", `${name} must contain literal types only`);
    assert.equal(member.literal.type, "StringLiteral", `${name} must contain string literals only`);
    return member.literal.value;
  });
}

function interfaceProperties(name) {
  const declaration = findDeclaration(
    typesFile,
    (node) => node.type === "TsInterfaceDeclaration" && node.id.value === name,
    `interface ${name}`
  );
  return declaration.body.body.map((member) => {
    assert.equal(member.type, "TsPropertySignature", `${name} may contain properties only`);
    assert.ok(member.key, `${name} property must have a key`);
    return member.key.value;
  });
}

function apiFunctionSource(name) {
  const declaration = findDeclaration(
    apiFile,
    (node) => node.type === "VariableDeclarator" && node.id.type === "Identifier" && node.id.value === name,
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
    "squad_list_players", "palworld_players", "minecraft_players", "nightingale_players", "necesse_players", "romestead_players", "terraria_players", "astroneer_players", "soulmask_players", "satisfactory_frm_players", "barotrauma_players", "return_to_moria_players", "windrose_players", "dragonwilds_players", "scum_players", "a2s_players"
  ]);
  assert.deepEqual(typeAliasValues("RuntimePlayerIdentityKind"), [
    "klei_user_id", "steam_id", "ark_account_id", "conan_user_id", "eos_id", "player_name", "session_id",
    "palworld_user_id", "minecraft_uuid", "astroneer_guid"
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

test("live-player API clients use only their declared commands and LAN-compatible instance keys", () => {
  const read = apiFunctionSource("readInstanceLivePlayers");
  const refresh = apiFunctionSource("refreshInstanceLivePlayers");
  const execute = apiFunctionSource("executeInstancePlayerAction");

  assert.match(read, /"read_instance_live_players"/);
  assert.match(read, /instanceId,\s*instance_id:\s*instanceId/);
  assert.match(refresh, /"refresh_instance_live_players"/);
  assert.match(refresh, /instanceId,\s*instance_id:\s*instanceId/);
  assert.match(execute, /"execute_instance_player_action",\s*\{ input \}/);
  assert.doesNotMatch(execute, /runtimeAction|command|transport|target/);
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
