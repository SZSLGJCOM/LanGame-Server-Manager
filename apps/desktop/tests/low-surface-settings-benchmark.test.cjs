const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const repoRoot = path.resolve(__dirname, "..", "..", "..");

function read(relativePath) {
  return fs.readFileSync(path.join(repoRoot, relativePath), "utf8");
}

function readSchema(moduleId) {
  return JSON.parse(read(path.join("modules", moduleId, "schema.json")));
}

function assertSchemaKeys(moduleId, expectedKeys) {
  const actual = Object.keys(readSchema(moduleId).properties ?? {}).sort();
  assert.deepEqual(actual, [...expectedKeys].sort(), `${moduleId} schema inventory drifted`);
}

const fixtures = {
  astroneer: "2026-09-08-steamcmd_anonymous_validate_728470.json",
  nightingale: "2026-07-13-steam_public_build_22204011.json",
  satisfactory: "2026-07-13-steam_public_build_23855705.json",
  romestead: "2026-07-13-steam_public_build_23662689.json",
  runescapedragonwilds: "2026-07-13-steamdb_app_4019830.json"
};

test("ASTRONEER models the current exact-build Astro settings section", () => {
  assertSchemaKeys("astroneer", [
    "active_save_file_name",
    "auto_save_interval_seconds",
    "backup_save_interval_seconds",
    "console_password",
    "deny_unlisted_players",
    "disable_server_travel",
    "extra_launch_args",
    "load_auto_save",
    "max_players",
    "max_server_framerate",
    "max_server_idle_framerate",
    "owner_guid",
    "player_activity_timeout_seconds",
    "public_ip",
    "server_name",
    "server_owner_display_name",
    "server_password",
    "verbose_player_properties",
    "wait_for_players_before_shutdown"
  ]);

  const template = read(
    "modules/astroneer/templates/Astro/Saved/Config/WindowsServer/AstroServerSettings.ini.hbs"
  );
  assert.match(template, /^\[\/Script\/Astro\.AstroServerSettings\]/m);
  assert.doesNotMatch(template, /^\[AstroServerSettings\]/m);
  for (const nativeKey of [
    "bLoadAutoSave",
    "MaxServerFramerate",
    "MaxServerIdleFramerate",
    "bWaitForPlayersBeforeShutdown",
    "ConsolePassword",
    "ServerName",
    "PlayerActivityTimeout",
    "bDisableServerTravel",
    "DenyUnlistedPlayers",
    "VerbosePlayerProperties",
    "BackupSaveGamesInterval",
    "ActiveSaveFileDescriptiveName"
  ]) {
    assert.match(template, new RegExp(`^${nativeKey}=`, "m"));
  }
  assert.match(read("modules/astroneer/module.toml"), /\{\{astroneer\.extra_launch_args\}\}/);
});

test("Nightingale exposes package and official launch settings as typed fields", () => {
  assertSchemaKeys("nightingale", [
    "admin_password",
    "enable_cheats",
    "extra_launch_args",
    "json_logging",
    "max_players",
    "server_password",
    "starting_difficulty",
    "status_endpoint_enabled"
  ]);
  const schema = readSchema("nightingale");
  assert.equal(schema.properties.enable_cheats.type, "boolean");
  assert.equal(schema.properties.json_logging.type, "boolean");
  assert.equal(schema.properties.status_endpoint_enabled.type, "boolean");
  assert.deepEqual(schema.properties.starting_difficulty.enum, ["easy", "medium", "hard", "extreme"]);

  const manifest = read("modules/nightingale/module.toml");
  assert.match(manifest, /\{\{nightingale\.enable_cheats_flag\}\}/);
  assert.match(manifest, /\{\{nightingale\.json_logging_args\}\}/);
  assert.match(manifest, /\{\{nightingale\.status_endpoint_args\}\}/);
  assert.match(manifest, /\{\{nightingale\.extra_launch_args\}\}/);
});

test("Satisfactory writes every documented tick-rate target and typed stable options", () => {
  const schema = readSchema("satisfactory");
  for (const key of [
    "disable_crash_reporting",
    "disable_packet_routing",
    "disable_seasonal_events",
    "external_reliable_port",
    "rotating_autosaves"
  ]) {
    assert.ok(schema.properties[key], `missing satisfactory.${key}`);
  }
  assert.equal(schema.properties.max_players.maximum, 127);
  assert.ok(
    schema.properties.engine_ini_extra["x-lsgm-disallowed-line-prefixes"].includes("NetServerMaxTickRate=")
  );
  assert.deepEqual(
    schema.properties.game_ini_extra["x-lsgm-disallowed-line-prefixes"],
    ["[/Script/Engine.GameSession]", "MaxPlayers="]
  );
  const definition = read("apps/desktop/src/views/settings/modules/satisfactory.ts");
  assert.doesNotMatch(definition, /highPlayerCap|insecureLocalApi/);

  const engine = read("modules/satisfactory/templates/Engine.ini.hbs");
  assert.match(engine, /\[\/Script\/OnlineSubsystemUtils\.IpNetDriver\][\s\S]*NetServerMaxTickRate=[\s\S]*LanServerMaxTickRate=/);
  assert.match(engine, /\[\/Script\/SocketSubsystemEpic\.EpicNetDriver\][\s\S]*NetServerMaxTickRate=[\s\S]*LanServerMaxTickRate=/);
  assert.match(engine, /\[\/Script\/Engine\.Engine\][\s\S]*NetClientTicksPerSecond=/);
  assert.match(engine, /\[\/Script\/FactoryGame\.FGSaveSession\][\s\S]*mNumRotatingAutosaves=/);
  assert.match(engine, /\[CrashReportClient\][\s\S]*bImplicitSend=/);
});

test("Romestead models every key emitted by the exact-build config", () => {
  assertSchemaKeys("romestead", [
    "auto_create_and_load_world",
    "auto_create_world_seed",
    "auto_create_world_size",
    "auto_start_world_name",
    "enable_cheats",
    "extra_launch_args",
    "max_players",
    "password",
    "sleep_threshold_ms"
  ]);
  const template = read("modules/romestead/templates/config.json.hbs");
  assert.match(template, /"AutoCreateWorldSeed":\s*\{\{romestead\.world_seed_json\}\}/);
  assert.match(template, /"SleepThresholdMs":\s*\{\{settings\.sleep_threshold_ms\}\}/);
  assert.match(read("modules/romestead/module.toml"), /\{\{romestead\.extra_launch_args\}\}/);
  const definitionSource = read("apps/desktop/src/views/settings/modules/romestead.ts");
  assert.match(definitionSource, /numeric === -1/);
  assert.match(definitionSource, /numeric >= 1 && numeric <= 16\.5/);
});

test("Dragonwilds uses the current first-party guide and preserves server-owned identity", () => {
  const ledger = read("modules/runescapedragonwilds/config-sources.toml");
  assert.match(ledger, /runescapedragonwilds\.help\.jagex\.com\/hc\/en-gb\/articles\/45365343055249/);
  assert.doesNotMatch(ledger, /dragonwilds\.runescape\.com\/news\/how-to-dedicated-servers/);
  assert.match(
    read("modules/runescapedragonwilds/module.toml"),
    /\{\{runescapedragonwilds\.extra_launch_args\}\}/
  );

  const materializer = read("crates/app-storage/src/templates_materialize.rs");
  assert.match(
    materializer,
    /materialize_runescapedragonwilds_support_files[\s\S]*merge_rendered_ini_file/
  );
  const fixture = JSON.parse(
    read(
      "modules/runescapedragonwilds/config-fixtures/2026-07-13-steamdb_app_4019830.json"
    )
  );
  const initial = fixture.initial.files[0].content;
  const expectedKeys = fixture.expected.files[0].keys;
  assert.match(initial, /ServerGuid=stable-guid/);
  assert.match(initial, /AdminUsers=owner-a,owner-b/);
  assert.equal(
    expectedKeys["/Script/Dominion.DedicatedServerSettings.ServerGuid"],
    "stable-guid"
  );
  assert.equal(
    expectedKeys["/Script/Dominion.DedicatedServerSettings.AdminUsers"],
    "owner-a,owner-b"
  );
});

test("the five-module cohort declares versioned native-output fixtures", () => {
  for (const [moduleId, fileName] of Object.entries(fixtures)) {
    const fixturePath = path.join("modules", moduleId, "config-fixtures", fileName);
    const fixture = JSON.parse(read(fixturePath));
    assert.equal(fixture.module_id, moduleId);
    assert.ok(fixture.expected.files.length > 0, `${moduleId} fixture has no native files`);
    assert.ok(
      fixture.expected.launch.executable_suffix || fixture.expected.launch.arguments.length > 0,
      `${moduleId} fixture has no native launch evidence`
    );
  }
});
