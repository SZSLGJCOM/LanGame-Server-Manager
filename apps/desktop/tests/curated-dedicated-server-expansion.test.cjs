const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const root = path.resolve(__dirname, "..", "..", "..");
const modulesDir = path.join(root, "modules");
const registrySource = fs.readFileSync(
  path.join(root, "apps", "desktop", "src", "views", "settings", "module-registry.ts"),
  "utf8"
);
const apiMockModuleAssetsSource = fs.readFileSync(path.join(root, "apps", "desktop", "src", "api-mock", "module-assets.ts"), "utf8");
const storeCopySource = fs.readFileSync(path.join(root, "apps", "desktop", "src", "store-copy.ts"), "utf8");
const storeSyncSource = fs.readFileSync(path.join(root, "scripts", "fetch_module_store_data.py"), "utf8");
const runtimeSource = fs.readFileSync(
  path.join(root, "crates", "app-storage", "src", "runtime.rs"),
  "utf8"
);
const moduleVerifierSource = fs.readFileSync(
  path.join(root, "scripts", "verify_module_setting_coverage.py"),
  "utf8"
) + fs.readFileSync(
  path.join(root, "scripts", "verify_module_setting_coverage_constants.py"),
  "utf8"
);

const curatedDedicatedServers = [
  {
    id: "runescapedragonwilds",
    steamAppId: 4019830,
    executable: "RSDragonwildsServer.exe",
    requiredSchemaKeys: [
      "owner_id",
      "server_name",
      "default_world_name",
      "admin_password",
      "world_password",
      "extra_launch_args"
    ],
    requiredPorts: ["game", "query"],
    requiredTemplates: [path.join("RSDragonwilds", "Saved", "Config", "WindowsServer", "DedicatedServer.ini.hbs")],
    requiredRuntimeSourceMarkers: [
      "dragonwilds_query_port_launch_arg_observed",
      "dragonwilds_query_port_not_bound"
    ],
    requiredTemplateMarkers: {
      [path.join("RSDragonwilds", "Saved", "Config", "WindowsServer", "DedicatedServer.ini.hbs")]: [
        "[/Script/Dominion.DedicatedServerSettings]"
      ]
    },
    requiredSchemaConstraints: {
      owner_id: { pattern: "^\\S*$" },
      server_name: { maxLength: 15 },
      default_world_name: { maxLength: 15 }
    },
    requiredLedgerMarkers: [
      "dragonwilds_query_port_a2s_probe_2026_06_29",
      "-QueryPort=27063",
      "A2S_INFO_ERROR ConnectionResetError"
    ],
  },
  {
    id: "romestead",
    steamAppId: 4763510,
    executable: "start-romestead.bat",
    requiredSchemaKeys: [
      "auto_start_world_name",
      "auto_create_and_load_world",
      "auto_create_world_size",
      "password",
      "max_players",
      "enable_cheats",
      "extra_launch_args"
    ],
    requiredPorts: ["game"],
    requiredTemplates: ["config.json.hbs", "start-romestead.bat.hbs"],
    requiredTemplateMarkers: {
      "config.json.hbs": ["AutoStartWorldName", "AutoCreateAndLoadWorld", "MaxPlayers"],
      "start-romestead.bat.hbs": ["DOTNET_ROOT", "Server.exe"]
    },
    requiredLedgerMarkers: [
      "steamcmd_appinfo_4763510_2026_07_01",
      "steamcmd_app_update_4763510_2026_07_01",
      "romestead_smoke_launch_2026_07_01",
      "freetodownload",
      "Server.exe",
      "start-romestead.bat",
      "config.json",
      "UDP 8050"
    ],
  }
];

const excludedServerFamilies = [
  "counterstrikesource",
  "dayofdefeatsource",
  "dayz",
  "factorio",
  "halflife2deathmatch",
  "killingfloor2",
  "left4dead2",
  "teamfortress2"
];

function read(filePath) {
  return fs.readFileSync(filePath, "utf8");
}

function readModuleIds() {
  return fs
    .readdirSync(modulesDir)
    .filter((name) => fs.existsSync(path.join(modulesDir, name, "module.toml")))
    .filter((name) => fs.existsSync(path.join(modulesDir, name, "schema.json")))
    .sort();
}

function assertRecordContains(source, recordName, moduleId) {
  const match = source.match(
    new RegExp(String.raw`const\s+${recordName}\s*:\s*Record<string,\s*[^>]+>\s*=\s*\{([\s\S]*?)\n\};`)
  );
  assert.ok(match, `${recordName} record is missing`);
  assert.match(match[1], new RegExp(`^\\s+${moduleId}:`, "m"), `${recordName} missing ${moduleId}`);
}

test("curated dedicated-server expansion reaches exactly 32 supported module profiles", () => {
  const moduleIds = readModuleIds();
  assert.equal(moduleIds.length, 32);
  assert.ok(!moduleIds.includes("starbound"), "starbound is not eligible for one-click anonymous deployment");
});

test("HumanitZ restored module is wired through active support surfaces", () => {
  const moduleId = "humanitz";
  const moduleRoot = path.join(modulesDir, moduleId);
  const moduleTomlPath = path.join(moduleRoot, "module.toml");
  const schemaPath = path.join(moduleRoot, "schema.json");
  const ledgerPath = path.join(moduleRoot, "config-sources.toml");
  const settingsDefinitionPath = path.join(
    root,
    "apps",
    "desktop",
    "src",
    "views",
    "settings",
    "modules",
    "humanitz.ts"
  );

  assert.ok(fs.existsSync(moduleTomlPath), "humanitz module.toml missing");
  assert.ok(fs.existsSync(schemaPath), "humanitz schema.json missing");
  assert.ok(fs.existsSync(ledgerPath), "humanitz config-sources.toml missing");
  assert.ok(fs.existsSync(settingsDefinitionPath), "humanitz settings definition missing");

  const moduleToml = read(moduleTomlPath);
  const schema = JSON.parse(read(schemaPath));
  const ledger = read(ledgerPath);

  assert.match(moduleToml, /^id = "humanitz"$/m);
  assert.match(moduleToml, /^steam_app_id = 2728330$/m);
  assert.match(moduleToml, /executable = "HumanitZServer\.exe"/);
  // The shipped EOS session does not answer A2S. Counts must use the
  // authenticated roster without changing the game's session provider.
  assert.match(moduleToml, /\[runtime\]\s+player_count_source = "player_list"/);
  assert.match(moduleToml, /\[runtime\.player_query\]\s+protocol = "none"/);
  const playerList = moduleToml.split("[runtime.player_list]")[1]?.split(/\r?\n\[/)[0] ?? "";
  assert.match(playerList, /^action_id = "list_online_players"$/m);
  assert.match(playerList, /^response_codec = "humanitz_players"$/m);
  assert.match(moduleToml, /\[\[runtime\.player_actions\]\][\s\S]*id = "kick_player"/);
  assert.match(moduleToml, /\[player_management\][\s\S]*status = "runtime_actions"/);

  for (const key of [
    "server_name",
    "max_players",
    "server_password",
    "rcon_enabled",
    "rcon_password",
    "admin_steam_ids",
    "reserved_player_steam_ids",
    "banned_player_steam_ids",
    "extra_launch_args"
  ]) {
    assert.ok(schema.properties?.[key], `humanitz missing verified schema key ${key}`);
    assert.match(ledger, new RegExp(`schema_key = "${key}"`), `humanitz missing provenance item for ${key}`);
  }

  for (const portName of ["game", "query", "rcon"]) {
    assert.match(
      moduleToml,
      new RegExp(`\\[\\[default_ports\\]\\][\\s\\S]*?name = "${portName}"`),
      `humanitz missing default port ${portName}`
    );
  }

  for (const templatePath of [
    "GameServerSettings.ini.hbs",
    "AdminList.txt.hbs",
    "F_ReservedSlots.txt.hbs",
    "F_BannedPlayers.txt.hbs"
  ]) {
    assert.ok(
      fs.existsSync(path.join(moduleRoot, "templates", templatePath)),
      `humanitz missing template ${templatePath}`
    );
  }

  assert.equal(schema.properties.allowed_player_steam_ids, undefined);
  assert.ok(!fs.existsSync(path.join(moduleRoot, "templates", "F_MVPAccess.txt.hbs")));
  assert.match(registrySource, /humanitzSettingsDefinition/, "humanitz missing settings registry entry");
  assert.match(apiMockModuleAssetsSource, /schemaHumanitz/, "humanitz schema import missing in api mock assets");
  assert.match(apiMockModuleAssetsSource, /tomlHumanitz/, "humanitz toml import missing in api mock assets");
  assertRecordContains(apiMockModuleAssetsSource, "mockModuleSchemasById", moduleId);
  assertRecordContains(apiMockModuleAssetsSource, "mockModuleTomlById", moduleId);
  assert.match(storeCopySource, /^\s+humanitz:/m, "humanitz missing store copy");
  assert.match(storeSyncSource, /"humanitz"/, "humanitz missing remote store-media app override");
  assert.match(moduleVerifierSource, /"humanitz"/, "humanitz missing verifier coverage");
});

test("RimWorld Together replaces Garry's Mod as the supported colony-sim server", () => {
  const moduleIds = readModuleIds();
  assert.ok(moduleIds.includes("rimworld"), "rimworld module missing");
  assert.ok(!moduleIds.includes("garrysmod"), "garrysmod should no longer be supported");

  const moduleId = "rimworld";
  const moduleRoot = path.join(modulesDir, moduleId);
  const moduleTomlPath = path.join(moduleRoot, "module.toml");
  const schemaPath = path.join(moduleRoot, "schema.json");
  const ledgerPath = path.join(moduleRoot, "config-sources.toml");
  const settingsDefinitionPath = path.join(
    root,
    "apps",
    "desktop",
    "src",
    "views",
    "settings",
    "modules",
    "rimworld.ts"
  );

  assert.ok(fs.existsSync(moduleTomlPath), "rimworld module.toml missing");
  assert.ok(fs.existsSync(schemaPath), "rimworld schema.json missing");
  assert.ok(fs.existsSync(ledgerPath), "rimworld config-sources.toml missing");
  assert.ok(fs.existsSync(settingsDefinitionPath), "rimworld settings definition missing");

  const moduleToml = read(moduleTomlPath);
  const schema = JSON.parse(read(schemaPath));
  const ledger = read(ledgerPath);

  assert.match(moduleToml, /^id = "rimworld"$/m);
  assert.match(moduleToml, /name = "RimWorld Together Server"/);
  assert.match(moduleToml, /executable = "RTServer\.exe"/);
  assert.match(moduleToml, /port = 25555/);
  assert.match(moduleToml, /\[runtime\.player_query\][\s\S]*protocol = "none"/);
  assert.match(moduleToml, /\[\[runtime\.player_actions\]\][\s\S]*id = "list_players"/);
  assert.match(moduleToml, /\[\[runtime\.player_actions\]\][\s\S]*id = "kick_user"/);
  assert.match(moduleToml, /\[player_management\][\s\S]*status = "runtime_actions"/);
  assert.match(moduleToml, /RimWorld Together/);
  assert.match(moduleToml, /3005289691/);

  for (const key of [
    "server_name",
    "server_description",
    "bind_ip",
    "max_players",
    "verbosity",
    "display_chat_in_console",
    "use_upnp",
    "sync_local_save",
    "enable_server_browser",
    "enable_server_telemetry",
    "server_password"
  ]) {
    assert.ok(schema.properties?.[key], `rimworld missing verified schema key ${key}`);
    assert.match(ledger, new RegExp(`schema_key = "${key}"`), `rimworld missing provenance item for ${key}`);
  }

  for (const templatePath of ["ServerConfig.json.hbs"]) {
    assert.ok(
      fs.existsSync(path.join(moduleRoot, "templates", templatePath)),
      `rimworld missing template ${templatePath}`
    );
  }

  assert.match(registrySource, /rimworldSettingsDefinition/, "rimworld missing settings registry entry");
  assert.match(apiMockModuleAssetsSource, /schemaRimworld/, "rimworld schema import missing in api mock assets");
  assert.match(apiMockModuleAssetsSource, /tomlRimworld/, "rimworld toml import missing in api mock assets");
  assertRecordContains(apiMockModuleAssetsSource, "mockModuleSchemasById", moduleId);
  assertRecordContains(apiMockModuleAssetsSource, "mockModuleTomlById", moduleId);
  assert.match(storeCopySource, /^\s+rimworld:/m, "rimworld missing store copy");
  assert.match(storeSyncSource, /"rimworld"/, "rimworld missing remote store-media app override");
  assert.match(moduleVerifierSource, /"rimworld"/, "rimworld missing verifier coverage");
});

test("curated dedicated-server candidates are wired without exposing unverified admin capability", () => {
  for (const moduleSpec of curatedDedicatedServers) {
    const moduleRoot = path.join(modulesDir, moduleSpec.id);
    const moduleTomlPath = path.join(moduleRoot, "module.toml");
    const schemaPath = path.join(moduleRoot, "schema.json");
    const ledgerPath = path.join(moduleRoot, "config-sources.toml");
    const settingsDefinitionPath = path.join(
      root,
      "apps",
      "desktop",
      "src",
      "views",
      "settings",
      "modules",
      `${moduleSpec.id}.ts`
    );

    assert.ok(fs.existsSync(moduleTomlPath), `${moduleSpec.id} module.toml missing`);
    assert.ok(fs.existsSync(schemaPath), `${moduleSpec.id} schema.json missing`);
    assert.ok(fs.existsSync(ledgerPath), `${moduleSpec.id} config-sources.toml missing`);
    assert.ok(fs.existsSync(settingsDefinitionPath), `${moduleSpec.id} settings definition missing`);

    const moduleToml = read(moduleTomlPath);
    const ledger = read(ledgerPath);
    assert.match(moduleToml, new RegExp(`^id = "${moduleSpec.id}"$`, "m"));
    assert.match(moduleToml, new RegExp(`^steam_app_id = ${moduleSpec.steamAppId}$`, "m"));
    assert.match(moduleToml, new RegExp(`executable = "${moduleSpec.executable.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}"`));
    assert.match(moduleToml, new RegExp(`\\[player_management\\][\\s\\S]*status = "${moduleSpec.id === "romestead" ? "runtime_actions" : "pending_adapter"}"`));
    assert.match(moduleToml, /\[runtime\.player_query\][\s\S]*protocol = "none"/);
    if (moduleSpec.id === "romestead") {
      assert.match(moduleToml, /response_codec = "romestead_players"/);
      assert.match(moduleToml, /player_action_ids = \[\]/);
      assert.match(moduleToml, /command_template = "list"/);
      assert.doesNotMatch(moduleToml, /command_template = "(?:kick|ban|unban) /);
    } else {
      assert.doesNotMatch(moduleToml, /\[\[runtime\.player_actions\]\]/);
    }

    const schema = JSON.parse(read(schemaPath));
    const schemaKeys = Object.keys(schema.properties ?? {});
    assert.ok(schema.properties?.extra_launch_args, `${moduleSpec.id} schema should retain audited launch args`);
    assert.ok(schemaKeys.length > 1, `${moduleSpec.id} should expose verified dedicated-server settings, not only raw launch args`);
    for (const key of moduleSpec.requiredSchemaKeys) {
      assert.ok(schema.properties?.[key], `${moduleSpec.id} missing verified schema key ${key}`);
      assert.match(
        moduleToml + read(schemaPath),
        new RegExp(`settings\\.${key}|\\{\\{\\s*${key}\\s*\\}\\}|${key}`),
        `${moduleSpec.id} schema key ${key} is not represented in launch args or templates`
      );
      assert.match(ledger, new RegExp(`schema_key = "${key}"`), `${moduleSpec.id} missing provenance item for ${key}`);
    }
    for (const [key, constraints] of Object.entries(moduleSpec.requiredSchemaConstraints ?? {})) {
      for (const [constraintName, expectedValue] of Object.entries(constraints)) {
        assert.equal(
          schema.properties?.[key]?.[constraintName],
          expectedValue,
          `${moduleSpec.id} schema key ${key} should set ${constraintName}=${expectedValue}`
        );
      }
    }

    for (const portName of moduleSpec.requiredPorts) {
      assert.match(
        moduleToml,
        new RegExp(`\\[\\[default_ports\\]\\][\\s\\S]*?name = "${portName}"`),
        `${moduleSpec.id} missing default port ${portName}`
      );
    }

    for (const templatePath of moduleSpec.requiredTemplates ?? []) {
      const absoluteTemplatePath = path.join(moduleRoot, "templates", templatePath);
      assert.ok(
        fs.existsSync(absoluteTemplatePath),
        `${moduleSpec.id} missing template ${templatePath}`
      );
      for (const marker of moduleSpec.requiredTemplateMarkers?.[templatePath] ?? []) {
        assert.match(
          read(absoluteTemplatePath),
          new RegExp(marker.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")),
          `${moduleSpec.id} template ${templatePath} missing marker ${marker}`
        );
      }
    }

    for (const marker of moduleSpec.requiredRuntimeSourceMarkers ?? []) {
      assert.match(
        runtimeSource,
        new RegExp(marker.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")),
        `${moduleSpec.id} runtime source missing marker ${marker}`
      );
    }

    for (const marker of moduleSpec.requiredLedgerMarkers ?? []) {
      assert.match(
        ledger,
        new RegExp(marker.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")),
        `${moduleSpec.id} ledger missing marker ${marker}`
      );
    }

    assert.match(registrySource, new RegExp(`${moduleSpec.id}SettingsDefinition`), `${moduleSpec.id} missing settings registry entry`);
    assert.match(apiMockModuleAssetsSource, new RegExp(`schema${moduleSpec.id}`, "i"), `${moduleSpec.id} schema import missing in api mock assets`);
    assert.match(apiMockModuleAssetsSource, new RegExp(`toml${moduleSpec.id}`, "i"), `${moduleSpec.id} toml import missing in api mock assets`);
    assertRecordContains(apiMockModuleAssetsSource, "mockModuleSchemasById", moduleSpec.id);
    assertRecordContains(apiMockModuleAssetsSource, "mockModuleTomlById", moduleSpec.id);
    assert.match(storeCopySource, new RegExp(`^\\s+${moduleSpec.id}:`, "m"), `${moduleSpec.id} missing store copy`);
    assert.match(storeSyncSource, new RegExp(`"${moduleSpec.id}"`), `${moduleSpec.id} missing remote store-media app override`);
    assert.match(moduleVerifierSource, new RegExp(`"${moduleSpec.id}"`), `${moduleSpec.id} missing verifier coverage`);
  }
});

test("excluded legacy or permission-gated server families stay out of active module profiles", () => {
  const moduleIds = new Set(readModuleIds());
  for (const moduleId of excludedServerFamilies) {
    assert.equal(moduleIds.has(moduleId), false, `${moduleId} should not be counted in the 34-module target`);
  }
});
