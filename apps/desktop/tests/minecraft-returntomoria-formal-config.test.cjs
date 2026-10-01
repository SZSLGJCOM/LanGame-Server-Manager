const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const root = path.resolve(__dirname, "..", "..", "..");

function read(relativePath) {
  return fs.readFileSync(path.join(root, relativePath), "utf8");
}

function readSchema(moduleId) {
  return JSON.parse(read(`modules/${moduleId}/schema.json`));
}

function templatePropertyKeys(template) {
  return template
    .split(/\r?\n/)
    .filter((line) => line && !line.startsWith("#") && !line.startsWith("{{") && line.includes("="))
    .map((line) => line.slice(0, line.indexOf("=")));
}

function assertSettingsReferencesExist(template, schema) {
  const references = [...template.matchAll(/\{\{settings\.([A-Za-z0-9_]+)\}\}/g)].map((match) => match[1]);
  const missing = [...new Set(references)].filter((key) => !schema.properties[key]);
  assert.deepEqual(missing, []);
}

test("Minecraft reference properties omit generated credentials", () => {
  const reference = read("modules/minecraft/reference-configs/server.properties");
  const properties = Object.fromEntries(reference.split(/\r?\n/)
    .filter((line) => line && !line.startsWith("#") && line.includes("="))
    .map((line) => [line.slice(0, line.indexOf("=")), line.slice(line.indexOf("=") + 1)]));

  for (const key of ["management-server-secret", "management-server-tls-keystore-password", "rcon.password"]) {
    assert.ok(Object.hasOwn(properties, key), `Reference property is missing: ${key}`);
    assert.ok(properties[key] === "", `Reference property must be empty: ${key}`);
  }
});

test("Minecraft template matches the formal 26.2 server.properties surface", () => {
  const schema = readSchema("minecraft");
  const template = read("modules/minecraft/templates/server.properties.hbs");
  const sourceLedger = read("modules/minecraft/config-sources.toml");
  const expectedKeys = [
    "accepts-transfers",
    "allow-flight",
    "broadcast-console-to-ops",
    "broadcast-rcon-to-ops",
    "bug-report-link",
    "chat-spam-threshold-seconds",
    "command-spam-threshold-seconds",
    "difficulty",
    "enable-code-of-conduct",
    "enable-jmx-monitoring",
    "enable-query",
    "enable-rcon",
    "enable-status",
    "enforce-secure-profile",
    "enforce-whitelist",
    "entity-broadcast-range-percentage",
    "force-gamemode",
    "function-permission-level",
    "gamemode",
    "generate-structures",
    "generator-settings",
    "hardcore",
    "hide-online-players",
    "initial-disabled-packs",
    "initial-enabled-packs",
    "level-name",
    "level-seed",
    "level-type",
    "log-ips",
    "management-server-allowed-origins",
    "management-server-enabled",
    "management-server-host",
    "management-server-port",
    "management-server-secret",
    "management-server-tls-enabled",
    "management-server-tls-keystore",
    "management-server-tls-keystore-password",
    "max-chained-neighbor-updates",
    "max-players",
    "max-tick-time",
    "max-world-size",
    "motd",
    "network-compression-threshold",
    "online-mode",
    "op-permission-level",
    "pause-when-empty-seconds",
    "player-idle-timeout",
    "prevent-proxy-connections",
    "query.port",
    "rate-limit",
    "rcon.password",
    "rcon.port",
    "region-file-compression",
    "require-resource-pack",
    "resource-pack",
    "resource-pack-id",
    "resource-pack-prompt",
    "resource-pack-sha1",
    "server-ip",
    "server-port",
    "simulation-distance",
    "spawn-protection",
    "status-heartbeat-interval",
    "sync-chunk-writes",
    "text-filtering-config",
    "text-filtering-version",
    "use-native-transport",
    "view-distance",
    "white-list"
  ].sort();

  assert.deepEqual(templatePropertyKeys(template).sort(), expectedKeys);
  assertSettingsReferencesExist(template, schema);
  assert.equal(schema.properties.difficulty.default, "easy");
  assert.equal(schema.properties.management_server_tls_enabled.default, true);
  assert.equal(schema.properties.chat_spam_threshold_seconds.default, 10);
  assert.equal(schema.properties.command_spam_threshold_seconds.default, 10);
  assert.equal(schema.properties.rcon_password.default, undefined);
  assert.equal(schema.properties.rcon_password["x-lsgm-default-source"], "generated_secret");
  assert.equal(schema.properties.rcon_password.minLength, 24);

  for (const retiredKey of [
    "allow_nether",
    "pvp",
    "enable_command_block",
    "spawn_monsters",
    "spawn_animals",
    "spawn_npcs",
    "debug"
  ]) {
    assert.equal(schema.properties[retiredKey], undefined);
  }

  assert.match(sourceLedger, /status = "exhaustive_verified"/);
  assert.match(sourceLedger, /latest\.release to 26\.2/);
  assert.match(sourceLedger, /823e2250d24b3ddac457a60c92a6a941943fcd6a/);
});

test("Return to Moria template uses the native build 21872765 sectioned config", () => {
  const schema = readSchema("returntomoria");
  const template = read("modules/returntomoria/templates/MoriaServerConfig.ini.hbs");
  const moduleToml = read("modules/returntomoria/module.toml");
  const sourceLedger = read("modules/returntomoria/config-sources.toml");
  const sections = [...template.matchAll(/^\[([^\]]+)\]$/gm)].map((match) => match[1]);

  assert.deepEqual(sections, ["Main", "World", "World.Create", "Host", "Console", "Performance"]);
  assertSettingsReferencesExist(template, schema);
  assert.doesNotMatch(template, /^\[Server\]$/m);
  assert.doesNotMatch(template, /^MaxPlayers=/m);
  assert.equal(schema.properties.max_players, undefined);
  assert.equal(schema.properties.server_name.default, "Dedicated Server World");
  assert.equal(schema.properties.world_file_name.default, "");
  assert.equal(schema.properties.world_seed.default, "random");
  assert.equal(schema.properties.game_mode.default, "campaign");
  assert.equal(schema.properties.difficulty_preset.default, "normal");
  assert.equal(schema.properties.server_fps.default, 60);
  assert.equal(schema.properties.loaded_area_limit.default, 12);
  assert.equal(schema.properties.loaded_area_limit.minimum, 4);
  assert.equal(schema.properties.loaded_area_limit.maximum, 32);
  assert.equal(schema.properties.rules_message.maxLength, 1024);

  assert.match(moduleToml, /name = "game"\s+protocol = "udp"\s+port = 7777/s);
  assert.match(moduleToml, /name = "game_tcp"\s+protocol = "tcp"\s+port = 7777/s);
  assert.match(moduleToml, /\[\[runtime\.port_groups\]\]\s+id = "game_transport"\s+members = \["game", "game_tcp"\]/s);
  assert.match(moduleToml, /\{\{launch\.extra_args\}\}/);
  assert.doesNotMatch(moduleToml, /\{\{settings\.extra_launch_args\}\}/);
  assert.match(sourceLedger, /status = "exhaustive_verified"/);
  assert.match(sourceLedger, /build 21872765/);
  assert.match(sourceLedger, /fixed eight-player simultaneous-session limit/);
});
