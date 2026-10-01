const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const repoRoot = path.resolve(__dirname, "..", "..", "..");
const barotraumaRoot = path.join(repoRoot, "modules", "barotrauma");
const conanRoot = path.join(repoRoot, "modules", "conanexiles");
const vrisingRoot = path.join(repoRoot, "modules", "vrising");
const squadRoot = path.join(repoRoot, "modules", "squad");
const soulmaskRoot = path.join(repoRoot, "modules", "soulmask");
const farmingSimulator25Root = path.join(repoRoot, "modules", "farmingsimulator25");
const satisfactoryRoot = path.join(repoRoot, "modules", "satisfactory");
const windroseRoot = path.join(repoRoot, "modules", "windrose");
const dayzRoot = path.join(repoRoot, "modules", "dayz");
const astroneerRoot = path.join(repoRoot, "modules", "astroneer");
const terrariaRoot = path.join(repoRoot, "modules", "terraria");
const necesseRoot = path.join(repoRoot, "modules", "necesse");
const coreKeeperRoot = path.join(repoRoot, "modules", "corekeeper");
const valheimRoot = path.join(repoRoot, "modules", "valheim");
const unturnedRoot = path.join(repoRoot, "modules", "unturned");
const minecraftRoot = path.join(repoRoot, "modules", "minecraft");
const dontStarveRoot = path.join(repoRoot, "modules", "dontstarve");
const rustRoot = path.join(repoRoot, "modules", "rust");
const palworldRoot = path.join(repoRoot, "modules", "palworld");
const projectZomboidRoot = path.join(repoRoot, "modules", "projectzomboid");
const sonsOfTheForestRoot = path.join(repoRoot, "modules", "sonsoftheforest");
const theForestRoot = path.join(repoRoot, "modules", "theforest");
const scumRoot = path.join(repoRoot, "modules", "scum");
const sevenDaysToDieRoot = path.join(repoRoot, "modules", "sevendaystodie");
const abioticFactorRoot = path.join(repoRoot, "modules", "abioticfactor");
const enshroudedRoot = path.join(repoRoot, "modules", "enshrouded");
const arkSurvivalAscendedRoot = path.join(repoRoot, "modules", "arksurvivalascended");
const arkSurvivalEvolvedRoot = path.join(repoRoot, "modules", "arksurvivalevolved");
const appStorageSrc = path.join(repoRoot, "crates", "app-storage", "src");
const appStorageTemplates = path.join(appStorageSrc, "templates.rs");
const appStorageTemplateFiles = fs
  .readdirSync(appStorageSrc)
  .filter((fileName) => /^templates.*\.rs$/.test(fileName))
  .map((fileName) => path.join(appStorageSrc, fileName))
  .sort();

function read(filePath) {
  return fs.readFileSync(filePath, "utf8");
}

test("September documentation reviews remain separate from dated native verification", () => {
  const reviewDate = "2026-09-30";
  const moduleIds = [
    "rimworld", "romestead", "runescapedragonwilds", "rust", "satisfactory", "scum",
    "sevendaystodie", "sonsoftheforest", "soulmask", "squad", "terraria", "theforest",
    "unturned", "valheim", "vrising", "windrose",
  ];
  for (const moduleId of moduleIds) {
    const ledger = read(path.join(repoRoot, "modules", moduleId, "config-sources.toml"));
    const review = ledger.split("[[sources]]").find((source) =>
      source.includes('id = "configuration_review_20260930"'));
    assert.ok(review, `${moduleId} records the September documentation review`);
    assert.match(review, /^kind = "documentation"\r?$/m);
    assert.match(review, /^authority = "first_party_documentation_and_read_only_installed_package_review"\r?$/m);
    assert.ok(review.includes(`description = "${reviewDate}: `));
    const verifiedDate = /^last_verified = "([^"]+)"\r?$/m.exec(ledger)?.[1];
    assert.ok(verifiedDate, `${moduleId} retains its verification date`);
    assert.notEqual(verifiedDate, reviewDate,
      `${moduleId} preserves its dated package/runtime verification`);
  }
  assert.match(read(path.join(rustRoot, "config-sources.toml")), /does not revalidate native RCON/);
  assert.match(read(path.join(valheimRoot, "config-sources.toml")),
    /older 0\.221\.12 lifecycle evidence does not validate 1\.0 networking or readiness/);
});

function readModWorkbenchSources() {
  return ["ModWorkbench.tsx", "mod-workbench-model.ts", "mod-workbench-plans.ts"]
    .map((fileName) => read(path.join(repoRoot, "apps", "desktop", "src", "views", "servers", fileName)))
    .join("\n");
}

function readAppStorageTemplates() {
  return appStorageTemplateFiles.map(read).join("\n");
}

function parseTomlArrayTables(text, tableName) {
  const tables = [];
  const pattern = new RegExp(`\\[\\[${tableName}\\]\\]([\\s\\S]*?)(?=\\n\\[\\[|\\n\\[|$)`, "g");
  for (const match of text.matchAll(pattern)) {
    const table = {};
    for (const line of match[1].split(/\r?\n/)) {
      const item = line.match(/^\s*([A-Za-z0-9_]+)\s*=\s*(?:"([^"]*)"|([0-9]+))\s*$/);
      if (item) {
        table[item[1]] = item[2] ?? item[3];
      }
    }
    tables.push(table);
  }
  return tables;
}

test("mod workflow audit reads shared capability catalog and keeps guardrails not_modelled", () => {
  const auditScript = read(path.join(repoRoot, "scripts", "audit_mod_workflows.py"));
  const capability = read(path.join(repoRoot, "apps", "desktop", "src", "views", "servers", "mod-workbench-capability.ts"));

  assert.match(capability, /runescapedragonwilds:\s*\{[\s\S]*supportStatus:\s*"not_modelled"/);
  assert.match(auditScript, /MOD_WORKBENCH_CAPABILITY/);
  assert.match(auditScript, /mod_workbench_guardrail_ids/);
  assert.match(auditScript, /supportStatus:\s*"not_modelled"/);
  assert.match(auditScript, /frontend_guardrail/);
  assert.match(auditScript, /UI: unsupported guardrail/);
  assert.match(auditScript, /Shockbyte/);
  assert.match(auditScript, /self-hosted mod/);
});

test("Astroneer community server mods stage pak packages into Astro Saved Paks", () => {
  const moduleToml = read(path.join(astroneerRoot, "module.toml"));

  assert.match(moduleToml, /\[mods\.source\][\s\S]*provider = "manual"/);
  assert.match(moduleToml, /\[mods\.source\][\s\S]*label = "AstroModLoader Classic"/);
  assert.match(moduleToml, /url = "https:\/\/astroneermodding\.readthedocs\.io\/en\/latest\/guides\/faq\.html"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*target_template = "\{\{paths\.install_root\}\}\/Astro\/Saved\/Paks"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*target_label = "Astro\/Saved\/Paks"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*accepts = \["pak", "zip"\]/);
  assert.doesNotMatch(moduleToml, /\[mods\.enablement\]/);
});

test("Enshrouded community server mods stage Nexus packages into the loader mods directory", () => {
  const moduleToml = read(path.join(enshroudedRoot, "module.toml"));

  assert.match(moduleToml, /\[mods\.source\][\s\S]*provider = "nexus"/);
  assert.match(moduleToml, /\[mods\.source\][\s\S]*label = "Nexus Mods"/);
  assert.match(moduleToml, /url = "https:\/\/www\.nexusmods\.com\/enshrouded"/);
  assert.match(moduleToml, /\[mods\.source\][\s\S]*install_note = "Enshrouded has no official server mod API; install and patch Shroudtopia or EML separately, then stage downloaded Nexus packages into the loader-created mods directory\./);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*target_template = "\{\{paths\.install_root\}\}\/mods"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*target_label = "mods"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*accepts = \["folder", "zip", "dll"\]/);
  assert.doesNotMatch(moduleToml, /\[mods\.enablement\]/);
});

test("Barotrauma concrete serversettings.xml gaps are modeled", () => {
  const ledger = read(path.join(barotraumaRoot, "config-sources.toml"));
  const serverSettingsTemplate = read(
    path.join(barotraumaRoot, "templates", "serversettings.xml.hbs")
  );
  const items = parseTomlArrayTables(ledger, "items");
  const exclusions = parseTomlArrayTables(ledger, "exclusions");

  const expectedItems = [
    ["server_settings", "serversettings.TickRate", "tick_rate"],
    ["server_settings", "serversettings.RandomizeSeed", "randomize_seed"],
    ["server_settings", "serversettings.UseRespawnShuttle", "use_respawn_shuttle"],
    ["server_settings", "serversettings.RespawnInterval", "respawn_interval"],
    ["server_settings", "serversettings.AutoRestart", "auto_restart"],
    ["server_settings", "serversettings.AutoRestartInterval", "auto_restart_interval"],
    ["server_settings", "serversettings.StartWhenClientsReady", "start_when_clients_ready"],
    ["server_settings", "serversettings.AllowVoteKick", "allow_vote_kick"],
    ["server_settings", "serversettings.AllowEndVoting", "allow_end_voting"],
    ["server_settings", "serversettings.BanAfterWrongPassword", "ban_after_wrong_password"],
    [
      "server_settings",
      "serversettings.MaxPasswordRetriesBeforeBan",
      "max_password_retries_before_ban"
    ],
    ["server_settings", "serversettings.AutoBanTime", "auto_ban_time"]
  ];

  for (const [source, key, schemaKey] of expectedItems) {
    assert.ok(
      items.some((item) => item.source === source && item.key === key && item.schema_key === schemaKey),
      `${source}.${key} should map to schema key ${schemaKey}`
    );
    assert.ok(
      !exclusions.some((exclusion) => exclusion.source === source && exclusion.key === key),
      `${source}.${key} should not remain excluded`
    );
    assert.match(
      serverSettingsTemplate,
      new RegExp(`${key.replace("serversettings.", "")}="\\{\\{xml\\.settings\\.${schemaKey}\\}\\}"`),
      `serversettings.xml should render ${key} from an XML-escaped settings.${schemaKey}`
    );
  }
});

test("Barotrauma Steam Workshop mod workflow declares config_player.xml content packages", () => {
  const moduleToml = read(path.join(barotraumaRoot, "module.toml"));
  const schema = JSON.parse(read(path.join(barotraumaRoot, "schema.json")));
  const ledger = read(path.join(barotraumaRoot, "config-sources.toml"));
  const materializer = read(path.join(appStorageSrc, "templates_materialize.rs"));

  assert.match(moduleToml, /\[workshop\]/);
  assert.match(moduleToml, /consumer_app_id = 602960/);
  assert.match(moduleToml, /\[mods\.source\]/);
  assert.match(moduleToml, /provider = "steam"/);
  assert.match(moduleToml, /\[mods\.manual_staging\]/);
  assert.match(moduleToml, /target_template = "\{\{paths\.config_dir\}\}\/LocalMods"/);
  assert.match(moduleToml, /\[mods\.enablement\]/);
  assert.match(moduleToml, /setting_key = "mod_workshop_ids"/);

  const modField = schema.properties.mod_workshop_ids;
  assert.ok(modField, "Barotrauma should expose Workshop mod ids as an editable schema field");
  assert.equal(modField["x-lsgm-section"], "runtime");
  assert.equal(modField["x-lsgm-source-key"], "config.contentpackages.regularpackages");

  assert.match(ledger, /barotrauma_mod_content_packages/);
  assert.match(ledger, /config_player\.xml/);
  assert.match(materializer, /materialize_barotrauma_workshop_mods/);
});

test("Conan Exiles concrete ServerSettings.ini gaps are modeled", () => {
  const ledger = read(path.join(conanRoot, "config-sources.toml"));
  const template = read(path.join(conanRoot, "templates", "ServerSettings.ini.hbs"));
  const items = parseTomlArrayTables(ledger, "items");
  const exclusions = parseTomlArrayTables(ledger, "exclusions");

  const expectedItems = [
    ["server_settings", "ServerSettings.NPCMindReadingMode", "npc_mind_reading_mode"],
    [
      "server_settings",
      "ServerSettings.PlayerKnockbackMultiplier",
      "player_knockback_multiplier"
    ],
    ["server_settings", "ServerSettings.NPCKnockbackMultiplier", "npc_knockback_multiplier"],
    ["server_settings", "ServerSettings.StructureHealthMultiplier", "structure_health_multiplier"],
    [
      "server_settings",
      "ServerSettings.StructureDamageTakenMultiplier",
      "structure_damage_taken_multiplier"
    ]
  ];

  for (const [source, key, schemaKey] of expectedItems) {
    assert.ok(
      items.some((item) => item.source === source && item.key === key && item.schema_key === schemaKey),
      `${source}.${key} should map to schema key ${schemaKey}`
    );
    assert.ok(
      !exclusions.some((exclusion) => exclusion.source === source && exclusion.key === key),
      `${source}.${key} should not remain excluded`
    );
    assert.match(
      template,
      new RegExp(`${key.replace("ServerSettings.", "")}=\\{\\{${schemaKey}\\}\\}`),
      `ServerSettings.ini should render ${key} from ${schemaKey}`
    );
  }
});

test("V Rising current WarEventGameSettings are modeled from the real server JSON", () => {
  const ledger = read(path.join(vrisingRoot, "config-sources.toml"));
  const schema = JSON.parse(read(path.join(vrisingRoot, "schema.json")));
  const renderer = readAppStorageTemplates();
  const items = parseTomlArrayTables(ledger, "items");
  const exclusions = parseTomlArrayTables(ledger, "exclusions");

  const expectedItems = [
    ["server_game_settings_json", "WarEventGameSettings.Interval", "war_event_interval"],
    ["server_game_settings_json", "WarEventGameSettings.MajorDuration", "war_event_major_duration"],
    ["server_game_settings_json", "WarEventGameSettings.MinorDuration", "war_event_minor_duration"],
    [
      "server_game_settings_json",
      "WarEventGameSettings.WeekdayTime.StartHour",
      "war_event_weekday_start_hour"
    ],
    [
      "server_game_settings_json",
      "WarEventGameSettings.WeekdayTime.StartMinute",
      "war_event_weekday_start_minute"
    ],
    [
      "server_game_settings_json",
      "WarEventGameSettings.WeekdayTime.EndHour",
      "war_event_weekday_end_hour"
    ],
    [
      "server_game_settings_json",
      "WarEventGameSettings.WeekdayTime.EndMinute",
      "war_event_weekday_end_minute"
    ],
    [
      "server_game_settings_json",
      "WarEventGameSettings.WeekendTime.StartHour",
      "war_event_weekend_start_hour"
    ],
    [
      "server_game_settings_json",
      "WarEventGameSettings.WeekendTime.StartMinute",
      "war_event_weekend_start_minute"
    ],
    [
      "server_game_settings_json",
      "WarEventGameSettings.WeekendTime.EndHour",
      "war_event_weekend_end_hour"
    ],
    [
      "server_game_settings_json",
      "WarEventGameSettings.WeekendTime.EndMinute",
      "war_event_weekend_end_minute"
    ],
    [
      "server_game_settings_json",
      "WarEventGameSettings.ScalingPlayers1.PointsModifier",
      "war_event_scaling_players_1_points_modifier"
    ],
    [
      "server_game_settings_json",
      "WarEventGameSettings.ScalingPlayers1.DropModifier",
      "war_event_scaling_players_1_drop_modifier"
    ],
    [
      "server_game_settings_json",
      "WarEventGameSettings.ScalingPlayers2.PointsModifier",
      "war_event_scaling_players_2_points_modifier"
    ],
    [
      "server_game_settings_json",
      "WarEventGameSettings.ScalingPlayers2.DropModifier",
      "war_event_scaling_players_2_drop_modifier"
    ],
    [
      "server_game_settings_json",
      "WarEventGameSettings.ScalingPlayers3.PointsModifier",
      "war_event_scaling_players_3_points_modifier"
    ],
    [
      "server_game_settings_json",
      "WarEventGameSettings.ScalingPlayers3.DropModifier",
      "war_event_scaling_players_3_drop_modifier"
    ],
    [
      "server_game_settings_json",
      "WarEventGameSettings.ScalingPlayers4.PointsModifier",
      "war_event_scaling_players_4_points_modifier"
    ],
    [
      "server_game_settings_json",
      "WarEventGameSettings.ScalingPlayers4.DropModifier",
      "war_event_scaling_players_4_drop_modifier"
    ]
  ];

  for (const [source, key, schemaKey] of expectedItems) {
    assert.ok(
      items.some((item) => item.source === source && item.key === key && item.schema_key === schemaKey),
      `${source}.${key} should map to schema key ${schemaKey}`
    );
    assert.ok(
      !exclusions.some((exclusion) => exclusion.source === source && exclusion.key === key),
      `${source}.${key} should not remain excluded`
    );
    assert.equal(
      schema.properties[schemaKey]?.["x-lsgm-source-key"],
      key,
      `${schemaKey} should point at ${key}`
    );
  }

  assert.match(renderer, /WarEventGameSettings/);
  assert.match(renderer, /war_event_scaling_players_4_drop_modifier/);
});

test("V Rising Thunderstore mods stage into BepInEx plugins without assuming the loader", () => {
  const moduleToml = read(path.join(vrisingRoot, "module.toml"));

  assert.match(moduleToml, /\[mods\.source\][\s\S]*provider = "thunderstore"/);
  assert.match(moduleToml, /\[mods\.source\][\s\S]*label = "Thunderstore"/);
  assert.match(moduleToml, /url = "https:\/\/thunderstore\.io\/c\/v-rising\/"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*target_template = "\{\{paths\.install_root\}\}\/BepInEx\/plugins"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*target_label = "BepInEx\/plugins"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*accepts = \["dll", "zip", "folder"\]/);
  assert.doesNotMatch(moduleToml, /\[mods\.enablement\]/);
});


test("Squad current ServerConfig defaults are modeled from the installed server files", () => {
  const ledger = read(path.join(squadRoot, "config-sources.toml"));
  const schema = JSON.parse(read(path.join(squadRoot, "schema.json")));
  const serverTemplate = read(path.join(squadRoot, "templates", "Server.cfg.hbs"));
  const rconTemplate = read(path.join(squadRoot, "templates", "Rcon.cfg.hbs"));
  const items = parseTomlArrayTables(ledger, "items");
  const exclusions = parseTomlArrayTables(ledger, "exclusions");

  const expectedServerSettings = [
    ["PublicQueueLimit", "public_queue_limit"],
    ["JoiningPlayerTimeout", "joining_player_timeout_seconds"],
    ["Tags", "server_tags"],
    ["Rules", "server_rules"],
    ["MapRotationMode", "map_rotation_mode"],
    ["RandomizeAtStart", "randomize_rotation_at_start"],
    ["UseVoteFactions", "use_vote_factions"],
    ["UseVoteLevel", "use_vote_level"],
    ["UseVoteLayer", "use_vote_layer"],
    ["AllowFireteamLayersInRotation", "allow_fireteam_layers_in_rotation"],
    ["NumPlayersDiffForTeamChanges", "num_players_diff_for_team_changes"],
    ["AllianceEnabled", "alliance_enabled"],
    ["AllowPublicClientsToRecord", "allow_public_clients_to_record"],
    ["ServerMessageInterval", "server_message_interval_seconds"],
    ["TKAutoKickEnabled", "tk_auto_kick_enabled"],
    ["AutoTKBanNumberTKs", "auto_tk_ban_number_tks"],
    ["AutoTKBanTime", "auto_tk_ban_time_seconds"],
    ["VehicleKitRequirementDisabled", "vehicle_kit_requirement_disabled"],
    ["AllowCommunityAdminAccess", "allow_community_admin_access"],
    ["TimeBetweenMatches", "time_between_matches_seconds"],
    ["TimeBeforeVote", "time_before_vote_seconds"],
    ["AllowDevProfiling", "allow_dev_profiling"],
    ["AllowQA", "allow_qa"],
    ["VehicleClaimingDisabled", "vehicle_claiming_disabled"],
    ["PrepTimeStandard", "prep_time_standard_seconds"],
    ["PrepTimeSmallScale", "prep_time_small_scale_seconds"]
  ];

  for (const [key, schemaKey] of expectedServerSettings) {
    assert.ok(schema.properties[schemaKey], `Squad schema should expose ${schemaKey} for ${key}`);
    assert.ok(
      items.some((item) => item.source === "server_cfg" && item.key === key && item.schema_key === schemaKey),
      `Squad ledger should map ${key} to ${schemaKey}`
    );
    assert.match(serverTemplate, new RegExp(`${key}="?\\{\\{settings\\.${schemaKey}\\}\\}"?`));
  }

  const expectedRconSettings = [
    ["MaxConnections", "rcon_max_connections"],
    ["MaxConnectionsFromSameHost", "rcon_max_connections_from_same_host"],
    ["ConnectionTimeout", "rcon_connection_timeout_seconds"],
    ["SecondsBeforeTimeoutCheck", "rcon_seconds_before_timeout_check"],
    ["AuthenticationTimeout", "rcon_authentication_timeout_seconds"]
  ];

  for (const [key, schemaKey] of expectedRconSettings) {
    assert.ok(schema.properties[schemaKey], `Squad schema should expose ${schemaKey} for ${key}`);
    assert.ok(
      items.some((item) => item.source === "rcon_cfg" && item.key === key && item.schema_key === schemaKey),
      `Squad ledger should map ${key} to ${schemaKey}`
    );
    assert.match(rconTemplate, new RegExp(`${key}=\\{\\{settings\\.${schemaKey}\\}\\}`));
  }

  for (const key of ["Port"]) {
    assert.ok(
      exclusions.some((exclusion) => exclusion.source === "rcon_cfg" && exclusion.key === key),
      `Squad Rcon.cfg ${key} should be excluded as instance network state`
    );
  }
});

test("Squad Steam Workshop mods install into Plugins/Mods", () => {
  const moduleToml = read(path.join(squadRoot, "module.toml"));
  const auditScript = read(path.join(repoRoot, "scripts", "audit_mod_workflows.py"));
  const commandMods = read(path.join(repoRoot, "apps", "desktop", "src-tauri", "src", "commands_mods.rs"));

  assert.match(moduleToml, /\[workshop\][\s\S]*provider = "steam"/);
  assert.match(moduleToml, /\[workshop\][\s\S]*consumer_app_id = 393380/);
  assert.match(moduleToml, /\[mods\.source\][\s\S]*provider = "steam"/);
  assert.match(moduleToml, /url = "https:\/\/steamcommunity\.com\/app\/393380\/workshop\/"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*target_template = "\{\{paths\.install_root\}\}\/SquadGame\/Plugins\/Mods"/);
  assert.doesNotMatch(moduleToml, /\[mods\.enablement\]/);
  assert.match(commandMods, /stage_downloaded_workshop_items_into_manual_target/);
  assert.match(auditScript, /workshop and source and staging and not enablement/);
});

test("Soulmask current GameXishu gameplay settings are modeled from the installed server templates", () => {
  const ledger = read(path.join(soulmaskRoot, "config-sources.toml"));
  const schema = JSON.parse(read(path.join(soulmaskRoot, "schema.json")));
  const template = fs
    .readdirSync(path.join(soulmaskRoot, "templates"))
    .filter((fileName) => /^GameXishu\.profile-[0-2]\.json\.hbs$/.test(fileName))
    .sort()
    .map((fileName) => read(path.join(soulmaskRoot, "templates", fileName)))
    .join("\n");
  const renderer = [
    readAppStorageTemplates(),
    read(path.join(appStorageSrc, "templates_materialize", "unreal_large.rs"))
  ].join("\n");
  const items = parseTomlArrayTables(ledger, "items");
  const exclusions = parseTomlArrayTables(ledger, "exclusions");

  assert.match(ledger, /WS\/Config\/GameplaySettings\/GameXishuConfig_Template\.json/);
  assert.match(ledger, /WS\/Saved\/GameplaySettings\/GameXishu\.json/);
  assert.match(ledger, /last_verified = "2026-09-28"/);
  assert.match(ledger, /game_xishu_build_25117179/);
  assert.match(ledger, /same 282 coefficient keys/);
  assert.match(ledger, /buildid 24123420/);
  assert.match(ledger, /all 276 keys/);
  assert.match(renderer, /fn materialize_soulmask/);
  assert.match(renderer, /ManagedConfigFile::JsonObject/);
  assert.match(renderer, /SOULMASK_GAME_XISHU_FILE/);

  const expectedGameplaySettings = [
    ["ExpRatio", "xishu_exp_ratio"],
    ["CaiJiDiaoLuoRatio", "xishu_cai_ji_diao_luo_ratio"],
    ["GameWorldDayTimePortion", "xishu_game_world_day_time_portion"],
    ["JianZhuChuanSongMenPlusKaiGuan", "xishu_jian_zhu_chuan_song_men_plus_kai_guan"],
    ["SpecialEventConfigSwitch", "xishu_special_event_config_switch"],
    ["MaxConveyorCount", "xishu_max_conveyor_count"],
    ["DrawDebugDungeon", "xishu_draw_debug_dungeon"],
    ["NewYingHuoTimeLenMul", "xishu_new_ying_huo_time_len_mul"],
    ["JianDuiRuQinKaiGuan", "xishu_jian_dui_ru_qin_kai_guan"],
    ["ReleaseControlStatusCDRatio", "xishu_release_control_status_cd_ratio"]
  ];

  for (const [key, schemaKey] of expectedGameplaySettings) {
    assert.ok(schema.properties[schemaKey], `Soulmask schema should expose ${schemaKey} for ${key}`);
    assert.ok(
      items.some((item) => item.source === "game_xishu_json" && item.key === key && item.schema_key === schemaKey),
      `Soulmask ledger should map ${key} to ${schemaKey}`
    );
    assert.ok(
      !exclusions.some((exclusion) => exclusion.source === "game_xishu_json" && exclusion.key === key),
      `Soulmask ${key} should not remain excluded`
    );
    assert.match(template, new RegExp(`"${key}":\\s*\\{\\{settings\\.${schemaKey}\\}\\}`));
  }

  assert.equal(
    Object.values(schema.properties).filter((field) => field["x-lsgm-source"] === "game_xishu_json").length,
    276
  );
  for (const key of [
    "xishu_man_ren_tenacity_damage_ratio",
    "xishu_man_ren_boss_tenacity_damage_ratio",
    "xishu_dong_wu_tenacity_damage_ratio",
    "xishu_dong_wu_boss_tenacity_damage_ratio"
  ]) {
    assert.equal(schema.properties[key].default, 0.5, `${key} should use the current package default`);
  }
  assert.equal(schema.properties.xishu_zu_ren_direct_cun_qu.default, 1);
});

test("Soulmask Steam Workshop mods are modeled through the startup mod argument", () => {
  const moduleToml = read(path.join(soulmaskRoot, "module.toml"));
  const schema = JSON.parse(read(path.join(soulmaskRoot, "schema.json")));
  const ledger = read(path.join(soulmaskRoot, "config-sources.toml"));
  const settingsDefinition = ["soulmask.ts", "soulmask-groups.ts"]
    .map((fileName) => read(path.join(repoRoot, "apps", "desktop", "src", "views", "settings", "modules", fileName)))
    .join("\n");
  const modWorkbench = readModWorkbenchSources();
  const appRuntime = read(path.join(repoRoot, "crates", "app-runtime", "src", "launch_templates.rs"));

  assert.match(moduleToml, /\[workshop\][\s\S]*provider = "steam"/);
  assert.match(moduleToml, /\[workshop\][\s\S]*consumer_app_id = 2646460/);
  assert.match(moduleToml, /\[mods\.source\][\s\S]*provider = "steam"/);
  assert.match(moduleToml, /url = "https:\/\/steamcommunity\.com\/app\/2646460\/workshop\/"/);
  assert.match(moduleToml, /\[mods\.enablement\][\s\S]*setting_key = "mod_workshop_ids"/);
  assert.match(moduleToml, /\[mods\.enablement\][\s\S]*id_strategy = "steam_workshop_id"/);
  assert.match(moduleToml, /\[mods\.enablement\][\s\S]*reference_strategy = "steam_workshop_id"/);
  assert.match(moduleToml, /\{\{soulmask\.workshop_mods_arg\}\}/);

  const modField = schema.properties.mod_workshop_ids;
  assert.ok(modField, "Soulmask should expose Workshop IDs for the -mod launch argument.");
  assert.equal(modField["x-lsgm-source-key"], "-mod");
  assert.match(ledger, /key = "-mod"[\s\S]*schema_key = "mod_workshop_ids"/);
  assert.match(settingsDefinition, /"mod_workshop_ids"/);
  assert.match(modWorkbench, /case "soulmask"/);
  assert.match(appRuntime, /render_soulmask_workshop_mods_arg/);
  assert.doesNotMatch(moduleToml, /\[mods\.manual_staging\]/);
});

test("Farming Simulator 25 is not shipped as a LanGame server module", () => {
  assert.ok(
    !fs.existsSync(farmingSimulator25Root),
    "FS25 should not be discoverable from modules/farmingsimulator25"
  );

  const activeSurfaces = [
    ...appStorageTemplateFiles,
    path.join(repoRoot, "crates", "app-runtime", "src", "lib.rs"),
    path.join(repoRoot, "apps", "desktop", "src", "api-mock.ts"),
    path.join(repoRoot, "apps", "desktop", "src", "api-mock", "module-details.ts"),
    path.join(repoRoot, "apps", "desktop", "src", "api-mock", "module-manifest.ts"),
    path.join(repoRoot, "apps", "desktop", "src", "i18n-messages-en-extra.ts"),
    path.join(repoRoot, "apps", "desktop", "src", "i18n-messages-zh-extra.ts"),
    path.join(repoRoot, "apps", "desktop", "src", "views", "settings", "module-registry.ts"),
    path.join(repoRoot, "apps", "desktop", "src", "views", "settings", "ConfigurationWorkspace.tsx"),
    path.join(repoRoot, "apps", "desktop", "src", "i18n", "games", "en-us.ts"),
    path.join(repoRoot, "apps", "desktop", "src", "i18n", "games", "zh-cn.ts"),
    path.join(repoRoot, "apps", "desktop", "src", "store-copy.ts"),
    path.join(repoRoot, "apps", "desktop", "src", "data", "module-store-data.json"),
    path.join(repoRoot, "scripts", "verify_module_setting_coverage.py")
  ];

  for (const surface of activeSurfaces) {
    const source = read(surface);
    assert.doesNotMatch(source, /farmingsimulator25|Farming Simulator 25|FARMING_SIMULATOR/);
  }
});

test("DayZ is not shipped as a LanGame server module", () => {
  assert.ok(!fs.existsSync(dayzRoot), "DayZ should not be discoverable from modules/dayz");

  const removedFilesOrDirs = [
    path.join(repoRoot, "apps", "desktop", "src", "views", "settings", "modules", "dayz.ts"),
    path.join(repoRoot, "apps", "desktop", "src", "i18n", "games", "dayz.en.ts"),
    path.join(repoRoot, "apps", "desktop", "src", "i18n", "games", "dayz.zh-cn.ts"),
    path.join(repoRoot, "apps", "desktop", "public", "game-covers", "dayz.jpg"),
    path.join(repoRoot, "apps", "desktop", "public", "game-media", "dayz")
  ];

  for (const removedPath of removedFilesOrDirs) {
    assert.ok(!fs.existsSync(removedPath), `${removedPath} should be removed with the DayZ module`);
  }

  const activeSurfaces = [
    ...appStorageTemplateFiles,
    path.join(repoRoot, "apps", "desktop", "src", "api-mock.ts"),
    path.join(repoRoot, "apps", "desktop", "src", "api-mock", "module-details.ts"),
    path.join(repoRoot, "apps", "desktop", "src", "api-mock", "module-manifest.ts"),
    path.join(repoRoot, "apps", "desktop", "src", "views", "settings", "module-registry.ts"),
    path.join(repoRoot, "apps", "desktop", "src", "views", "settings", "ConfigurationWorkspace.tsx"),
    path.join(repoRoot, "apps", "desktop", "src", "i18n", "games", "en-us.ts"),
    path.join(repoRoot, "apps", "desktop", "src", "i18n", "games", "zh-cn.ts"),
    path.join(repoRoot, "apps", "desktop", "src", "store-copy.ts"),
    path.join(repoRoot, "apps", "desktop", "src", "data", "module-store-data.json"),
    path.join(repoRoot, "scripts", "verify_module_setting_coverage.py"),
    path.join(repoRoot, "scripts", "fetch_module_store_data.py"),
    path.join(repoRoot, "docs", "game-config-source-ledger.csv"),
    path.join(repoRoot, "docs", "game-config-source-ledger.md")
  ];

  for (const surface of activeSurfaces) {
    const source = read(surface);
    assert.doesNotMatch(source, /\bdayz\b|DayZ|DAYZ|223350|DayZServer_x64\.exe/);
  }
});

test("Satisfactory anonymous dedicated package validate keeps INI materialization explicit", () => {
  const moduleToml = read(path.join(satisfactoryRoot, "module.toml"));
  const ledger = read(path.join(satisfactoryRoot, "config-sources.toml"));

  assert.match(moduleToml, /steam_app_id = 1690800/);
  assert.match(moduleToml, /executable = "Engine\/Binaries\/Win64\/FactoryServer-Win64-Shipping-Cmd\.exe"/);
  assert.match(moduleToml, /args_template\s*=\s*\[\s*"FactoryGame",/);
  assert.match(ledger, /id = "native_console_entry_build_24656085"/);

  assert.match(ledger, /steamcmd_anonymous_app_1690800/);
  assert.match(ledger, /last_verified = "2026-09-08"/);
  assert.match(ledger, /steam_public_build_23855705/);
  assert.match(ledger, /installed_dedicated_server_build_24656085/);
  assert.match(ledger, /native_profile_isolation/);
  assert.match(moduleToml, /\[process\.environment_template\]/);
  assert.match(moduleToml, /USERPROFILE = "\{\{paths\.data_dir\}\}\/profile"/);
  assert.match(ledger, /Anonymous SteamCMD validate for app 1690800 succeeded/);
  assert.match(ledger, /FactoryServer\.exe/);
  assert.match(ledger, /no plain Game\.ini or Engine\.ini template under FactoryGame\/Saved\/Config\/WindowsServer/);
  assert.match(ledger, /networking, logging, and local API settings materialized before launch/);
});

test("Satisfactory server mods stage into FactoryGame Mods without assuming SMM", () => {
  const moduleToml = read(path.join(satisfactoryRoot, "module.toml"));

  assert.match(moduleToml, /\[mods\.source\][\s\S]*provider = "manual"/);
  assert.match(moduleToml, /\[mods\.source\][\s\S]*label = "Satisfactory Mod Manager \/ FICSIT\.app"/);
  assert.match(moduleToml, /url = "https:\/\/ficsit\.app\/"/);
  assert.match(
    moduleToml,
    /\[mods\.manual_staging\][\s\S]*target_template = "\{\{paths\.install_root\}\}\/FactoryGame\/Mods"/
  );
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*target_label = "FactoryGame\/Mods"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*accepts = \["smod", "zip", "folder"\]/);
  assert.doesNotMatch(moduleToml, /\[mods\.enablement\]/);
});

test("Windrose anonymous dedicated package validate still matches ServerDescription coverage", () => {
  const moduleToml = read(path.join(windroseRoot, "module.toml"));
  const ledger = read(path.join(windroseRoot, "config-sources.toml"));
  const schema = JSON.parse(read(path.join(windroseRoot, "schema.json")));
  const template = read(path.join(windroseRoot, "templates", "ServerDescription.json.hbs"));

  assert.match(moduleToml, /steam_app_id = 4129620/);
  assert.match(moduleToml, /verification_path = "WindroseServer\.exe"/);
  assert.match(moduleToml, /executable = "WindroseServer\.exe"/);

  assert.match(ledger, /steamcmd_anonymous_app_4129620/);
  assert.match(ledger, /app_update 4129620 validate/);
  assert.match(ledger, /buildid 24094403/);
  assert.match(ledger, /SizeOnDisk 3101317483/);
  assert.match(ledger, /DedicatedServer\.md/);
  assert.match(ledger, /RocksDB_v2/);
  assert.equal(schema.properties.allow_multiple_server_instances.default, false);
  assert.match(template, /"CanLaunchMultipleServerInstances": \{\{settings\.allow_multiple_server_instances\}\}/);
  assert.match(moduleToml, /R5\/ServerDescription\.json/);
});

test("Windrose community pak mods stage into the server tilde mods directory", () => {
  const moduleToml = read(path.join(windroseRoot, "module.toml"));

  assert.match(moduleToml, /\[mods\.source\][\s\S]*provider = "manual"/);
  assert.match(moduleToml, /\[mods\.source\][\s\S]*label = "Nexus \/ CurseForge PAK mods"/);
  assert.match(moduleToml, /url = "https:\/\/hypeserv\.com\/en\/blog\/how-to-install-mods-on-a-windrose-server"/);
  assert.match(
    moduleToml,
    /\[mods\.manual_staging\][\s\S]*target_template = "\{\{paths\.install_root\}\}\/R5\/Content\/Paks\/~mods"/
  );
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*target_label = "R5\/Content\/Paks\/~mods"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*accepts = \["pak", "ucas", "utoc", "zip"\]/);
  assert.doesNotMatch(moduleToml, /accepts\s*=\s*\[[^\]]*"7z"/);
  assert.match(moduleToml, /Extract \.7z archives separately before importing/);
  assert.doesNotMatch(moduleToml, /\[mods\.enablement\]/);
});

test("Terraria official 1.4.5.8 package download and serverconfig path-owned keys are recorded", () => {
  const moduleToml = read(path.join(terrariaRoot, "module.toml"));
  const ledger = read(path.join(terrariaRoot, "config-sources.toml"));
  const template = read(path.join(terrariaRoot, "templates", "serverconfig.txt.hbs"));
  const exclusions = parseTomlArrayTables(ledger, "exclusions");

  assert.match(moduleToml, /download_url_windows = "https:\/\/terraria\.org\/api\/download\/pc-dedicated-server\/terraria-server-1458\.zip"/);
  assert.match(moduleToml, /size = 46415317/);
  assert.match(moduleToml, /sha256 = "f513a4ac9789d34af766291ae217c9cd7d9472e13782a0e2b17512f70d7a8334"/);
  assert.match(moduleToml, /executable = "\{\{terraria\.server_executable\}\}"/);
  assert.match(moduleToml, /args_template = \[\s*\n\s+"\{\{terraria\.launch_args\}\}"/);
  assert.match(moduleToml, /working_directory_template = "\{\{terraria\.working_directory\}\}"/);

  assert.match(ledger, /last_verified = "2026-09-28"/);
  assert.match(ledger, /official_dedicated_zip_1458/);
  assert.match(ledger, /official_dedicated_zip_1456/);
  assert.match(ledger, /terraria-server-1456\.zip/);
  assert.match(ledger, /45635619-byte archive/);
  assert.match(ledger, /SHA256 D75C455AC217FD3434448C8F8251C1347F0875A85C438589DC71B557777E9155/);
  assert.match(ledger, /1456\/Windows\/serverconfig\.txt/);
  assert.match(ledger, /Version 1\.4\.5\.6/);

  for (const key of ["port", "worldpath", "banlist"]) {
    assert.ok(
      exclusions.some((exclusion) => exclusion.source === "serverconfig" && exclusion.key === key),
      `Terraria serverconfig ${key} should be explicitly owned by LanGame generated paths or ports`
    );
  }

  assert.match(template, /port=\{\{ports\.game\.port\}\}/);
  assert.match(template, /banlist=\{\{paths\.config_dir\}\}\\banlist\.txt/);
  assert.match(template, /world=\{\{paths\.saves_dir\}\}\\\{\{settings\.world_file\}\}/);
});

test("Terraria exposes tModLoader Steam Workshop modpack settings", () => {
  const moduleToml = read(path.join(terrariaRoot, "module.toml"));
  const ledger = read(path.join(terrariaRoot, "config-sources.toml"));
  const schema = JSON.parse(read(path.join(terrariaRoot, "schema.json")));
  const settingsDefinition = read(path.join(repoRoot, "apps", "desktop", "src", "views", "settings", "modules", "terraria.ts"));
  const auditScript = read(path.join(repoRoot, "scripts", "audit_mod_workflows.py"));

  assert.match(moduleToml, /executable = "\{\{terraria\.server_executable\}\}"/);
  assert.match(moduleToml, /args_template = \[\s*\n\s+"\{\{terraria\.launch_args\}\}"/);
  assert.match(moduleToml, /working_directory_template = "\{\{terraria\.working_directory\}\}"/);
  assert.match(moduleToml, /\[workshop\][\s\S]*provider = "steam"/);
  assert.match(moduleToml, /\[workshop\][\s\S]*consumer_app_id = 1281930/);
  assert.match(moduleToml, /\[workshop\][\s\S]*supports_collections = true/);
  assert.match(moduleToml, /\[mods\.source\][\s\S]*provider = "steam"/);
  assert.match(moduleToml, /\[mods\.source\][\s\S]*label = "tModLoader Steam Workshop"/);
  assert.match(moduleToml, /url = "https:\/\/steamcommunity\.com\/app\/1281930\/workshop\/"/);
  assert.match(moduleToml, /tmodloader\/steamapps/);
  assert.match(moduleToml, /Mods\/install\.txt/);
  assert.match(moduleToml, /Vanilla TerrariaServer\.exe does not load tModLoader mods/);
  assert.match(
    moduleToml,
    /\[mods\.manual_staging\][\s\S]*target_template = "\{\{paths\.instance_root\}\}\/tmodloader\/steamapps\/workshop\/content\/1281930"/
  );
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*target_label = "tModLoader\/steamapps\/workshop\/content\/1281930"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*accepts = \["folder", "zip"\]/);
  assert.match(moduleToml, /\[mods\.enablement\][\s\S]*setting_key = "tmodloader_workshop_item_ids"/);
  assert.match(moduleToml, /\[mods\.enablement\][\s\S]*id_strategy = "folder_name"/);
  assert.match(moduleToml, /\[mods\.enablement\][\s\S]*reference_strategy = "steam_workshop_id"/);
  assert.equal(schema.properties.server_runtime.default, "vanilla");
  assert.deepEqual(schema.properties.server_runtime.enum, ["vanilla", "tmodloader"]);
  assert.equal(schema.properties.server_runtime["x-lsgm-source-key"], "terraria.runtime");
  assert.equal(schema.properties.tmodloader_runtime_dir["x-lsgm-source-key"], "tmodloader.runtime_dir");
  assert.equal(schema.properties.tmodloader_workshop_item_ids["x-lsgm-source-key"], "Mods/install.txt");
  assert.equal(schema.properties.tmodloader_workshop_item_ids["x-lsgm-section"], "mods");
  assert.equal(schema.properties.tmodloader_enabled_mod_names["x-lsgm-source-key"], "Mods/enabled.json");
  assert.equal(schema.properties.tmodloader_enabled_mod_names["x-lsgm-section"], "mods");
  assert.match(settingsDefinition, /keys: \["server_runtime", "tmodloader_runtime_dir"\]/);
  assert.match(settingsDefinition, /keys: \["tmodloader_workshop_item_ids", "tmodloader_enabled_mod_names"\]/);
  assert.match(settingsDefinition, /key === "tmodloader_workshop_item_ids"[\s\S]*return "workshop-id-list"/);
  assert.match(settingsDefinition, /key === "tmodloader_enabled_mod_names"[\s\S]*return "string-list"/);
  assert.match(ledger, /tmodloader_dedicated_server_utils/);
  assert.match(ledger, /tmodloader_steam_workshop/);
  assert.match(ledger, /steamapps\/workshop\/content\/1281930/);
  assert.match(ledger, /key = "Mods\/install\.txt"[\s\S]*schema_key = "tmodloader_workshop_item_ids"/);
  assert.match(ledger, /key = "Mods\/enabled\.json"[\s\S]*schema_key = "tmodloader_enabled_mod_names"/);
  assert.match(ledger, /schema_key = "server_runtime"/);
  assert.match(ledger, /schema_key = "tmodloader_runtime_dir"/);
  assert.doesNotMatch(auditScript, /"terraria": "Vanilla TerrariaServer\.exe is not the tModLoader server/);
});

test("Necesse anonymous package validate preserves the official nogui JVM launch wrapper", () => {
  const moduleToml = read(path.join(necesseRoot, "module.toml"));
  const ledger = read(path.join(necesseRoot, "config-sources.toml"));

  assert.match(moduleToml, /steam_app_id = 1169370/);
  assert.match(moduleToml, /executable = "jre\/bin\/java\.exe"/);
  for (const flag of [
    "-XX:+UnlockExperimentalVMOptions",
    "-XX:+UseG1GC",
    "-XX:+ExplicitGCInvokesConcurrent",
    "-XX:G1NewSizePercent=20",
    "-XX:G1ReservePercent=20",
    "-XX:MaxGCPauseMillis=50",
    "-XX:G1HeapRegionSize=32M"
  ]) {
    assert.match(moduleToml, new RegExp(flag.replace(/[+]/g, "\\+")));
  }
  assert.match(moduleToml, /"-jar",\s*\n\s*"Server\.jar",\s*\n\s*"-nogui"/);

  assert.match(ledger, /last_verified = "2026-07-13"/);
  assert.match(ledger, /steamcmd_anonymous_app_1169370/);
  assert.match(ledger, /app_update 1169370 validate/);
  assert.match(ledger, /app_update 1169370 validate returned Success/);
  assert.match(ledger, /buildid 23522725/);
  assert.match(ledger, /SizeOnDisk 238462099/);
  assert.match(ledger, /StartServer-nogui\.bat/);
  assert.match(ledger, /Server\.jar/);
  assert.match(ledger, /jre\/bin\/java\.exe/);
  assert.match(ledger, /No standalone server settings file is shipped/);
});

test("Necesse local jar mods stage into the server mods directory", () => {
  const moduleToml = read(path.join(necesseRoot, "module.toml"));

  assert.match(moduleToml, /\[mods\.source\][\s\S]*provider = "manual"/);
  assert.match(moduleToml, /\[mods\.source\][\s\S]*label = "Necesse local mods"/);
  assert.match(moduleToml, /url = "https:\/\/necessewiki\.com\/Modding"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*target_template = "\{\{paths\.install_root\}\}\/mods"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*target_label = "mods"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*accepts = \["jar", "zip", "folder"\]/);
  assert.doesNotMatch(moduleToml, /\[mods\.enablement\]/);
});

test("Core Keeper records current package probe and first-party ARGUMENTS coverage", () => {
  const moduleToml = read(path.join(coreKeeperRoot, "module.toml"));
  const ledger = read(path.join(coreKeeperRoot, "config-sources.toml"));
  const items = parseTomlArrayTables(ledger, "items");
  const exclusions = parseTomlArrayTables(ledger, "exclusions");

  assert.match(moduleToml, /steam_app_id = 1963720/);
  assert.match(moduleToml, /executable = "CoreKeeperServer\.exe"/);
  assert.match(moduleToml, /name = "game"\s+protocol = "udp"\s+port = 27015/);
  assert.match(moduleToml, /name = "query"\s+protocol = "udp"\s+port = 27016/);
  assert.match(moduleToml, /member_offsets = \{ game = 0, query = 1 \}/);
  assert.match(moduleToml, /\[runtime\.player_query\]\s+protocol = "a2s_info"\s+port_names = \["query"\]/);
  assert.doesNotMatch(moduleToml, /\[runtime\.join\]/);

  assert.match(ledger, /last_verified = "2026-08-30"/);
  assert.match(ledger, /steamcmd_anonymous_probe/);
  assert.match(ledger, /app_update 1963720 validate/);
  assert.match(ledger, /StateFlags 4/);
  assert.match(ledger, /UpdateResult 0/);
  assert.match(ledger, /buildid and TargetBuildID 23543502/);
  assert.match(ledger, /SizeOnDisk 577900888/);
  assert.match(ledger, /clean_server_config_probe/);
  assert.match(ledger, /maxNumberPlayers 100/);
  assert.match(ledger, /networkSendRate 20/);
  assert.match(ledger, /direct_connection_runtime_probe/);
  assert.match(ledger, /Listening on ip:0\.0\.0\.0:27017/);
  assert.match(ledger, /direct_connection_a2s_info_probe/);
  assert.match(ledger, /full Steam game ID 1621690/);
  assert.match(ledger, /LGSM allocated game port 27018/);
  assert.match(ledger, /query port 27019/);
  assert.match(ledger, /EDF-advertised game port 27018/);
  assert.match(ledger, /does not establish a steam:\/\/connect join contract/);
  assert.match(ledger, /official_package_arguments_txt/);
  assert.match(ledger, /ARGUMENTS\.txt/);
  assert.match(ledger, /README\.txt/);
  assert.match(ledger, /CoreKeeperServer\.exe/);

  const expectedLaunchMirrors = [
    ["-worldname", "server_name"],
    ["-worldseed", "world_seed"],
    ["-hashedworldseed", "hashed_world_seed"],
    ["-worldmode", "world_mode"],
    ["-season", "season_override"],
    ["-gameid", "game_id"],
    ["-maxplayers", "max_players"]
  ];

  for (const [key, schemaKey] of expectedLaunchMirrors) {
    assert.ok(
      items.some((item) => item.source === "launch_args" && item.key === key && item.schema_key === schemaKey),
      `Core Keeper ${key} should be crosswalked to ${schemaKey}`
    );
  }

  for (const key of ["-activatecontent", "-activateallcontent"]) {
    assert.ok(
      exclusions.some((exclusion) => exclusion.source === "launch_args" && exclusion.key === key),
      `Core Keeper ${key} should be explicitly excluded or deferred`
    );
  }
});

test("Valheim anonymous package validate and packaged headless script are recorded", () => {
  const moduleToml = read(path.join(valheimRoot, "module.toml"));
  const ledger = read(path.join(valheimRoot, "config-sources.toml"));
  const exclusions = parseTomlArrayTables(ledger, "exclusions");

  assert.match(moduleToml, /steam_app_id = 896660/);
  assert.match(moduleToml, /executable = "valheim_server\.exe"/);

  assert.match(ledger, /last_verified = "2026-07-13"/);
  assert.match(ledger, /steamcmd_anonymous_app_896660/);
  assert.match(ledger, /app_update 896660 validate/);
  assert.match(ledger, /Success! App '896660' fully installed/);
  assert.match(ledger, /buildid 21981590/);
  assert.match(ledger, /SizeOnDisk 1636649733/);
  assert.match(ledger, /start_headless_server\.bat/);
  assert.match(ledger, /Valheim Dedicated Server Manual\.pdf/);
  assert.match(ledger, /-nographics -batchmode -name \\"My server\\" -port 2456 -world \\"Dedicated\\" -password \\"secret\\" -crossplay/);

  assert.ok(
    exclusions.some((exclusion) => exclusion.source === "launch_args" && exclusion.key === "-savedir"),
    "Valheim -savedir should stay explicitly owned by LanGame path routing"
  );
  assert.ok(
    !exclusions.some((exclusion) => exclusion.source === "launch_args" && exclusion.key === "-logFile"),
    "Valheim -logFile should stay mapped because LanGame exposes it as a setting"
  );
});

test("Unturned anonymous package validate and first-party server scripts are recorded", () => {
  const moduleToml = read(path.join(unturnedRoot, "module.toml"));
  const ledger = read(path.join(unturnedRoot, "config-sources.toml"));
  const schema = JSON.parse(read(path.join(unturnedRoot, "schema.json")));
  const configTemplate = read(path.join(unturnedRoot, "templates", "Config.txt.hbs"));

  assert.match(moduleToml, /steam_app_id = 1110390/);
  assert.match(moduleToml, /executable = "Unturned\.exe"/);
  assert.match(moduleToml, /\{\{unturned\.server_launch_mode\}\}/);
  assert.doesNotMatch(moduleToml, /UseLegacyJsonGameplayConfig|gameplay_config_file_args/);
  assert.match(moduleToml, /Commands\.dat, Config\.txt, and WorkshopDownloadConfig\.json/);

  assert.match(ledger, /last_verified = "2026-09-28"/);
  assert.match(ledger, /steamcmd_anonymous_app_1110390/);
  assert.match(ledger, /app_update 1110390 validate/);
  assert.match(ledger, /buildid 24080174/);
  assert.match(ledger, /SizeOnDisk 1860413320/);
  assert.match(ledger, /official_server_configuration_docs/);
  assert.match(ledger, /Servers\/<ServerID>\/Config\.txt/);
  assert.match(ledger, /ExampleServer\.bat/);
  assert.match(ledger, /ServerHelper\.bat/);
  assert.match(ledger, /\+InternetServer\/ServerId/);
  assert.match(ledger, /\+LanServer\/ServerId/);
  assert.match(ledger, /Servers\/ServerId\/Server\/Commands\.dat/);
  assert.match(ledger, /-CommandName\/Arg0\/Arg1\/Arg#/);
  assert.match(ledger, /Port 27017/);

  assert.equal(schema.properties.use_legacy_json_gameplay_config, undefined);
  assert.equal(schema.properties.gameplay_config_file, undefined);
  assert.equal(schema.properties.game_server_login_token["x-lsgm-source-key"], "Browser.Login_Token");
  assert.equal(schema.properties.battl_eye["x-lsgm-source-key"], "Server.BattlEye_Secure");
  assert.equal(schema.properties.max_ping["x-lsgm-source-key"], "Server.Max_Ping_Milliseconds");
  assert.equal(schema.properties.timeout_seconds["x-lsgm-source-key"], "Server.Timeout_Queue_Seconds");
  assert.equal(fs.existsSync(path.join(unturnedRoot, "templates", "Config.json.hbs")), false);
  assert.equal(configTemplate.trim(), "{{unturned.native_config}}");
  assert.match(ledger, /native_config_3_26_3_12/);
  for (const key of [
    "Browser.Icon",
    "Browser.Desc_Hint",
    "Browser.Desc_Full",
    "Browser.Login_Token",
    "Browser.BookmarkHost",
    "Server.VAC_Secure",
    "Server.BattlEye_Secure",
    "Server.Use_FakeIP",
    "Server.Max_Ping_Milliseconds",
    "Server.Timeout_Queue_Seconds"
  ]) {
    assert.ok(Object.values(schema.properties).some((property) =>
      property["x-lsgm-source-key"] === key && property["x-lsgm-native-type"]), `${key} is mapped to a typed native dictionary field`);
  }
});

test("Minecraft historical download and current 26.3 native defaults are recorded", () => {
  const moduleToml = read(path.join(minecraftRoot, "module.toml"));
  const ledger = read(path.join(minecraftRoot, "config-sources.toml"));
  const schema = JSON.parse(read(path.join(minecraftRoot, "schema.json")));
  const historical = JSON.parse(read(path.join(minecraftRoot, "config-fixtures/2026-07-13-mojang_manifest_release_26_2.json")));
  const current = JSON.parse(read(path.join(minecraftRoot, "config-fixtures/2026-09-28-mojang_release_26_3_configuration.json")));

  assert.match(moduleToml, /manifest_url = "https:\/\/piston-meta\.mojang\.com\/mc\/game\/version_manifest_v2\.json"/);
  assert.match(moduleToml, /server_jar = "server\.jar"/);

  assert.match(ledger, /last_verified = "2026-09-28"/);
  assert.match(ledger, /mojang_manifest_release_26_2/);
  assert.match(ledger, /latest\.release 26\.2/);
  assert.match(ledger, /823e2250d24b3ddac457a60c92a6a941943fcd6a/);
  assert.match(ledger, /CDACDFB25898DE5E4B4B0E5DDCC2722F77067E46605709C2D886C000EBB63EC5/);
  assert.match(ledger, /size 60894273/);
  assert.match(ledger, /Java major version 25/);
  assert.match(ledger, /69-key upstream server\.properties/);
  assert.match(ledger, /id = "mojang_release_26_3_configuration"/);
  assert.match(ledger, /33680f5f2ac32864d6d7cf5e56a705fdb3e05f4c, 62294556 bytes/);
  assert.match(ledger, /DedicatedServerProperties\.class confirms all 69 rendered property names/);
  assert.match(ledger, /no 26\.3 server or world was launched/);
  assert.equal(schema.properties.enable_whitelist.default, true);
  assert.deepEqual(current.settings, {}, "new-instance fixture must use the schema default");
  assert.equal(current.expected.files.find((file) => file.path === "server.properties").keys["white-list"], "true");
  assert.deepEqual(current.expected.launch.arguments, [
    "-Xms1024M", "-Xmx4096M", "-jar", "{{paths.install_root}}/server.jar", "nogui"
  ]);
  assert.equal(historical.settings.enable_whitelist, false, "an existing explicit false remains authoritative");
  assert.equal(historical.expected.files.find((file) => file.path === "server.properties").keys["white-list"], "false");
});

test("Conan Exiles mod workflow declares Workshop ids and modlist materialization", () => {
  const moduleToml = read(path.join(conanRoot, "module.toml"));
  const schema = JSON.parse(read(path.join(conanRoot, "schema.json")));
  const ledger = read(path.join(conanRoot, "config-sources.toml"));
  const materializerEntry = read(path.join(appStorageSrc, "templates_materialize.rs"));
  const unrealMaterializer = read(path.join(appStorageSrc, "templates_materialize", "unreal_large.rs"));
  const materializer = read(path.join(appStorageSrc, "templates_materialize", "workshop_packages.rs"));

  assert.match(moduleToml, /\[workshop\]/);
  assert.match(moduleToml, /consumer_app_id = 440900/);
  assert.match(moduleToml, /\[mods\.source\]/);
  assert.match(moduleToml, /provider = "steam"/);
  assert.match(moduleToml, /\[mods\.manual_staging\]/);
  assert.match(moduleToml, /target_template = "\{\{paths\.install_root\}\}\/ConanSandbox\/Mods"/);
  assert.match(moduleToml, /\[mods\.enablement\]/);
  assert.match(moduleToml, /setting_key = "mod_workshop_ids"/);

  const modField = schema.properties.mod_workshop_ids;
  assert.ok(modField, "Conan should expose Workshop mod ids as an editable schema field");
  assert.equal(modField["x-lsgm-section"], "mods");
  assert.equal(modField["x-lsgm-source-key"], "ConanSandbox/Mods/modlist.txt");

  assert.match(ledger, /conan_modlist_txt/);
  assert.match(ledger, /ConanSandbox\/Mods\/modlist\.txt/);
  assert.match(materializerEntry, /#\[path = "templates_materialize\/workshop_packages\.rs"\]\s*mod workshop_packages;/);
  assert.match(materializerEntry, /use workshop_packages::\{[^}]*\bmaterialize_conan_modlist\b/);
  assert.match(unrealMaterializer, /"conanexiles"\s*=>\s*materialize_conan\(context,\s*files,\s*prepared\)/);
  assert.match(unrealMaterializer, /materialize_conan_modlist\(context,\s*files\)/);
  assert.match(materializer, /let path = local_root\.join\(CONAN_MODLIST_FILE\)/);
  assert.match(materializer, /destination_path: path,\s*replacement: render_conan_modlist_lines\(&names\)\.into_bytes\(\)/);
  assert.match(materializer, /files\.apply\(vec!\[prepare_conan_modlist\(context\)\?\]\)/);
});

test("Don't Starve Together records the current validated package and installed mod documentation", () => {
  const ledger = read(path.join(dontStarveRoot, "config-sources.toml"));

  assert.match(ledger, /last_verified = "2026-09-19"/);
  assert.match(ledger, /id = "installed_package_masteroption_inheritance"/);
  assert.match(ledger, /Caves has 86 independent options plus 34 inherited values/);
  assert.match(ledger, /id = "installed_package_location_defaults"/);
  assert.match(ledger, /installed Steam build 24700372/);
  assert.match(ledger, /task_set=cave_default and start_location=caves/);
  assert.match(ledger, /steamcmd_anonymous_validate_343050/);
  assert.match(ledger, /app_update 343050 validate/);
  assert.match(ledger, /StateFlags 4/);
  assert.match(ledger, /UpdateResult 0/);
  assert.match(ledger, /buildid and TargetBuildID 24080846/);
  assert.match(ledger, /SizeOnDisk 4509552230/);
  assert.match(ledger, /version\.txt reports 740477/);
  assert.match(ledger, /INSTALLING_MODS\.txt/);
  assert.match(ledger, /dedicated_server_mods_setup\.lua/);
});

test("Rust anonymous package validate records generated cfg ownership boundary", () => {
  const ledger = read(path.join(rustRoot, "config-sources.toml"));
  const schema = JSON.parse(read(path.join(rustRoot, "schema.json")));
  const defaults = JSON.parse(read(path.join(rustRoot, "config-fixtures/2026-09-28-facepunch_september_2026.json")));
  const optIn = JSON.parse(read(path.join(rustRoot, "config-fixtures/2026-09-28-native_september_configuration_build_25129933.json")));

  assert.match(ledger, /last_verified = "2026-09-28"/);
  assert.match(ledger, /steam_public_build_24090743/);
  assert.match(ledger, /Public branch build 24090743/);
  assert.match(ledger, /steamcmd_anonymous_app_258550/);
  assert.match(ledger, /buildid 23476634/);
  assert.match(ledger, /SizeOnDisk 5793956963/);
  assert.match(ledger, /not used as exact-build evidence/);
  assert.match(ledger, /no packaged server\.cfg, users\.cfg, or bans\.cfg under <validated-install-root>\/cfg/);
  assert.doesNotMatch(
    ledger,
    /(?:^|[\s"'])[A-Za-z]:[\\/]/m,
    "public provenance must not expose workstation paths"
  );
  assert.match(ledger, /RustDedicated_Data\/StreamingAssets\/default_graphics\.cfg/);
  assert.match(ledger, /LanGame-owned server\/<instance-id>\/cfg materialization remains the authoritative server config path/);
  assert.match(ledger, /id = "facepunch_september_2026"/);
  assert.match(ledger, /id = "native_september_configuration_build_25129933"/);
  assert.match(ledger, /CE9884CA79446BDA128FB7358DB56E6E11CE0C9400CD5CB2C482481013ED709D/);
  assert.match(ledger, /default cap 3 is the total multiplier/);
  assert.match(ledger, /user count <= upkeep_lock_min_users/);
  assert.match(ledger, /No game code was executed/);
  assert.equal(schema.properties.upkeep_group_max_multiplier.default, 3);
  assert.equal(schema.properties.upkeep_lock_min_users.default, 2);
  assert.equal(schema.properties.use_new_navmesh.default, false);
  assert.ok(!defaults.expected.launch.arguments.includes("-useNewNavmesh"));
  assert.equal(optIn.settings.use_new_navmesh, true);
  assert.equal(optIn.expected.launch.arguments.filter((argument) => argument === "-useNewNavmesh").length, 1);
  assert.equal(optIn.expected.files[0].fragments.length, 11, "all eleven group-upkeep convars have nondefault native expectations");
});

test("Rust uMod plugins stage into oxide plugins without assuming the loader", () => {
  const moduleToml = read(path.join(rustRoot, "module.toml"));

  assert.match(moduleToml, /\[mods\.source\][\s\S]*provider = "manual"/);
  assert.match(moduleToml, /\[mods\.source\][\s\S]*label = "uMod\/Oxide plugins"/);
  assert.match(moduleToml, /url = "https:\/\/umod\.org\/documentation\/plugins\/installation"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*target_template = "\{\{paths\.install_root\}\}\/oxide\/plugins"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*target_label = "oxide\/plugins"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*accepts = \["cs", "zip", "folder"\]/);
  assert.doesNotMatch(moduleToml, /\[mods\.enablement\]/);
});

test("Palworld anonymous package validate records DefaultPalWorldSettings handoff", () => {
  const ledger = read(path.join(palworldRoot, "config-sources.toml"));

  assert.match(ledger, /last_verified = "2026-07-13"/);
  assert.match(ledger, /steamcmd_anonymous_app_2394010/);
  assert.match(ledger, /app_update 2394010 validate/);
  assert.match(ledger, /completed successfully/);
  assert.match(ledger, /buildid 24088465/);
  assert.match(ledger, /TargetBuildID 24088465/);
  assert.match(ledger, /SizeOnDisk 6047845660/);
  assert.match(ledger, /DefaultPalWorldSettings\.ini/);
  assert.match(ledger, /Changes to this file will NOT be reflected on the server/);
  assert.match(ledger, /Pal\/Saved\/Config\/WindowsServer\/PalWorldSettings\.ini/);
  assert.match(ledger, /CrossplayPlatforms=\(Steam,Xbox,PS5,Mac\)/);
});

test("Palworld official Workshop mods stage and enable through PalModSettings", () => {
  const moduleToml = read(path.join(palworldRoot, "module.toml"));
  const schema = JSON.parse(read(path.join(palworldRoot, "schema.json")));
  const ledger = read(path.join(palworldRoot, "config-sources.toml"));
  const settingsDefinition = read(path.join(repoRoot, "apps", "desktop", "src", "views", "settings", "modules", "palworld.ts"));
  const settingsRegistry = read(path.join(repoRoot, "apps", "desktop", "src", "views", "settings", "module-registry.ts"));
  const modWorkbench = readModWorkbenchSources();
  const materializer = read(path.join(appStorageSrc, "templates_materialize.rs"));

  assert.match(moduleToml, /\[workshop\][\s\S]*provider = "steam"/);
  assert.match(moduleToml, /\[workshop\][\s\S]*consumer_app_id = 1623730/);
  assert.match(moduleToml, /\[mods\.source\][\s\S]*provider = "steam"/);
  assert.match(moduleToml, /\[mods\.source\][\s\S]*label = "Steam Workshop"/);
  assert.match(moduleToml, /url = "https:\/\/steamcommunity\.com\/app\/1623730\/workshop\/"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*target_template = "\{\{paths\.install_root\}\}\/Pal\/Binaries\/Win64\/Mods\/Workshop"/);
  assert.match(moduleToml, /\[mods\.enablement\][\s\S]*setting_key = "mod_package_names"/);
  assert.match(moduleToml, /\[mods\.enablement\][\s\S]*id_strategy = "palworld_package_name"/);
  assert.match(moduleToml, /\[mods\.enablement\][\s\S]*reference_strategy = "plain_id"/);

  const modField = schema.properties.mod_package_names;
  assert.ok(modField, "Palworld should expose PalModSettings ActiveModList package names.");
  assert.equal(modField["x-lsgm-source-key"], "Mods/PalModSettings.ini:ActiveModList");
  assert.doesNotMatch(settingsDefinition, /"mod_package_names"/, "Configuration must not duplicate the Mod package editor");
  assert.match(settingsRegistry, /withWorkspaceRenderedField\(\s*palworldSettingsDefinition,\s*"mod_package_names",\s*"services",\s*"mod-workbench-package-names",\s*"mods"/);
  assert.match(modWorkbench, /palworld_package_name/);
  assert.match(ledger, /palworld_server_mods/);
  assert.match(ledger, /Mods\/PalModSettings\.ini/);
  assert.match(materializer, /render_palworld_mod_settings_ini/);
});

test("Project Zomboid anonymous package validate records bundled Java launch wrapper", () => {
  const ledger = read(path.join(projectZomboidRoot, "config-sources.toml"));

  assert.match(ledger, /last_verified = "2026-09-28"/);
  assert.match(ledger, /native_server_options_build_24909836/);
  assert.match(ledger, /144 option constructors/);
  assert.match(ledger, /steamcmd_anonymous_app_380870/);
  assert.match(ledger, /Steam public branch and the validated local manifest both report/);
  assert.match(ledger, /TargetBuildID 22695654/);
  assert.match(ledger, /buildid 22695654/);
  assert.match(ledger, /SizeOnDisk 5385767121/);
  assert.match(ledger, /StartServer64\.bat/);
  assert.match(ledger, /ProjectZomboid64\.json/);
  assert.match(ledger, /-XX:\+UseZGC/);
  assert.match(ledger, /-Xms16g -Xmx16g/);
  assert.match(ledger, /LanGame adds -Duser\.home/);
  assert.match(ledger, /media\/lua\/shared\/Sandbox\/SandboxVars\.lua/);
  assert.match(ledger, /media\/lua\/shared\/SpawnRegions\.lua/);
});

test("Sons of the Forest current native JSON surface is exhaustive", () => {
  const ledger = read(path.join(sonsOfTheForestRoot, "config-sources.toml"));
  const schema = JSON.parse(read(path.join(sonsOfTheForestRoot, "schema.json")));
  const template = read(path.join(sonsOfTheForestRoot, "templates", "dedicatedserver.cfg.hbs"));

  assert.match(ledger, /status = "exhaustive_verified"/);
  assert.match(ledger, /last_verified = "2026-09-08"/);
  assert.match(ledger, /native_userdata_configuration_probe/);
  assert.match(ledger, /steamcmd_anonymous_app_2465200/);
  assert.match(ledger, /buildid 20228410/);
  assert.match(ledger, /SizeOnDisk 3425287354/);
  assert.match(ledger, /StartSOTFDedicated\.bat/);
  assert.match(ledger, /SonsOfTheForestDS\.exe \| consoleparser -colorize/);
  assert.match(ledger, /VerifyUpdateSOTFDedicated\.bat/);
  assert.match(ledger, /no packaged dedicatedserver\.cfg or ownerswhitelist\.txt/);
  assert.match(ledger, /steamcommunity\.com\/sharedfiles\/filedetails\/\?id=2992700419/);

  assert.equal(schema.properties.max_players.maximum, 8);
  assert.equal(schema.properties.server_password["x-lsgm-source-key"], "Password");
  assert.equal(schema.properties.log_files_enabled.default, false);
  assert.ok(schema.properties.game_mode.enum.includes("Custom"));
  assert.equal(schema.properties.idle_target_framerate.default, 5);
  assert.equal(schema.properties.active_target_framerate.default, 60);
  assert.equal(schema.properties.custom_single_use_containers.default, true);
  assert.ok(!("admin_password" in schema.properties));
  assert.ok(!("game_difficulty" in schema.properties));

  assert.match(template, /"Password": \{\{json\.settings\.server_password\}\}/);
  assert.match(template, /"GameSettings": \{/);
  assert.match(template, /"CustomGameModeSettings": \{/);
  assert.match(template, /"GameSetting\.Survival\.OneHitToCutTrees"/);
  assert.doesNotMatch(template, /ServerPassword|AdminPassword|GameDifficulty/);
});

test("Sons of the Forest RedLoader mods stage into the dedicated server Mods directory", () => {
  const moduleToml = read(path.join(sonsOfTheForestRoot, "module.toml"));

  assert.match(moduleToml, /\[mods\.source\][\s\S]*provider = "manual"/);
  assert.match(moduleToml, /\[mods\.source\][\s\S]*label = "RedLoader \/ SOTF Mods"/);
  assert.match(moduleToml, /url = "https:\/\/sotf-mods\.com\/mods"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*target_template = "\{\{paths\.install_root\}\}\/Mods"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*target_label = "Mods"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*accepts = \["folder", "dll", "zip"\]/);
  assert.doesNotMatch(moduleToml, /\[mods\.enablement\]/);
});

test("The Forest current generated server.cfg surface uses native values", () => {
  const moduleToml = read(path.join(theForestRoot, "module.toml"));
  const ledger = read(path.join(theForestRoot, "config-sources.toml"));
  const schema = JSON.parse(read(path.join(theForestRoot, "schema.json")));
  const template = read(path.join(theForestRoot, "templates", "server.cfg.hbs"));

  assert.match(moduleToml, /\[runtime\.player_query\]\s+protocol = "a2s_info"\s+port_names = \["query"\]/);
  assert.match(moduleToml, /\[runtime\.player_list\][\s\S]*response_codec = "a2s_players"/);
  assert.match(ledger, /a2s_player_probe_3488796/);
  assert.match(ledger, /status = "exhaustive_verified"/);
  assert.match(ledger, /last_verified = "2026-07-13"/);
  assert.match(ledger, /steam_public_build_3488796/);
  assert.match(ledger, /buildid and TargetBuildID 3488796/);
  assert.match(ledger, /SizeOnDisk 1762277705/);
  assert.match(ledger, /steamcommunity\.com\/sharedfiles\/filedetails\/\?id=907906289/);

  assert.equal(schema.properties.autosave_interval_minutes.default, 30);
  assert.equal(schema.properties.autosave_interval_minutes.minimum, 15);
  assert.equal(schema.properties.vac_enabled.default, false);
  assert.equal(schema.properties.admin_password.default, "");
  assert.equal(schema.properties.allow_cheats["x-lsgm-source-key"], "allowCheats");
  assert.equal(schema.properties.server_contact, undefined);
  assert.match(
    ledger,
    /source = "exact_build_server_cfg_3488796"[\s\S]*key = "serverContact"[\s\S]*no accessor or other consumer/
  );
  for (const key of [
    "reset_holes_on_load",
    "tree_regrowth",
    "building_destruction",
    "enemies_in_creative",
    "idle_target_fps",
    "active_target_fps"
  ]) {
    assert.ok(schema.properties[key], `The Forest should expose ${key}`);
  }

  assert.match(template, /^serverIP \{\{instance\.bind_ip\}\}:\{\{ports\.game\.port\}\}$/m);
  assert.match(template, /^allowCheats \{\{theforest\.allow_cheats_on_off\}\}$/m);
  assert.match(template, /^targetFpsIdle \{\{settings\.idle_target_fps\}\}$/m);
  assert.match(template, /^targetFpsActive \{\{settings\.active_target_fps\}\}$/m);
  assert.doesNotMatch(
    template,
    /\{\{settings\.(?:vac_enabled|show_logs|vegan_mode|vegetarian_mode|reset_holes_on_load|tree_regrowth|building_destruction|enemies_in_creative|allow_cheats|realistic_player_damage)\}\}/
  );
});

test("SCUM 1.3.1.1 package and current ServerSettings.ini keys are recorded", () => {
  const ledger = read(path.join(scumRoot, "config-sources.toml"));
  const schema = JSON.parse(read(path.join(scumRoot, "schema.json")));
  const template = read(path.join(scumRoot, "templates", "ServerSettings.ini.hbs"));

  assert.match(ledger, /last_verified = "2026-09-08"/);
  assert.match(ledger, /steamcmd_anonymous_app_3792580/);
  assert.match(ledger, /anonymous install validation for SCUM Server build 24107648/);
  assert.match(ledger, /18,851,870,520 bytes/);
  assert.match(ledger, /FAE30EDB50B77F09AE9CD318F31816379E7E116A8EF1204109FF1C513FC8BF0D/);
  assert.match(ledger, /v7, 433 native keys in six sections/);
  assert.match(ledger, /generated_economy_override/);
  assert.match(ledger, /generated_raid_times/);
  assert.match(ledger, /generated_notifications/);
  assert.equal(template.trim(), "{{scum.server_settings_ini}}");
  const renderer = read(
    path.join(repoRoot, "crates", "app-storage", "src", "templates_render_scum.rs")
  );
  for (const inventory of ["general", "world", "features", "respawn", "vehicles", "damage"]) {
    assert.match(renderer, new RegExp(`server-settings-v7/${inventory}\\.json`));
  }
  assert.match(renderer, /render_scum_server_settings_ini/);
  assert.doesNotMatch(template, /\/Script\/Scum\.ScumGameSession/);

  const nativeInventory = ["general", "world", "features", "respawn", "vehicles", "damage"]
    .flatMap((section) => JSON.parse(read(
      path.join(scumRoot, "server-settings-v7", `${section}.json`)
    )));
  for (const nativeKey of [
    "scum.AreAnimalsAllowedInWorld",
    "scum.AnimalGlobalDensityMultiplier",
    "scum.MaxNonVirtualAnimalsInWorld",
    "scum.FishingHighActivityZoneAmount",
    "scum.RadiationEnabled",
    "scum.NumberOfAllowedFlagsPerPlayer",
    "scum.TombstoneMaxAmountPerSquad"
  ]) {
    assert.ok(
      nativeInventory.some((entry) => entry.nativeKey === nativeKey),
      `${nativeKey} should remain in the exact typed SCUM inventory`
    );
  }

  assert.equal(
    schema.properties.server_world.properties.animal_global_density_multiplier.default,
    0.5
  );
  assert.equal(
    schema.properties.server_world.properties.max_non_virtual_animals_in_world.default,
    500
  );
  assert.equal(
    schema.properties.server_features.properties.number_of_allowed_flags_per_player.default,
    10
  );
  assert.equal(
    schema.properties.server_features.properties.number_of_allowed_flags_per_player.maximum,
    undefined
  );
});

test("7 Days to Die anonymous package validate records first-party XML and launch wrapper", () => {
  const ledger = read(path.join(sevenDaysToDieRoot, "config-sources.toml"));
  const schema = JSON.parse(read(path.join(sevenDaysToDieRoot, "schema.json")));
  const template = read(path.join(sevenDaysToDieRoot, "templates", "serverconfig.xml.hbs"));
  const settingsDefinition = read(path.join(repoRoot, "apps", "desktop", "src", "views", "settings", "modules", "sevendaystodie.ts"));
  const mockSettings = read(path.join(repoRoot, "apps", "desktop", "src", "api-mock", "module-settings.ts"));

  assert.match(ledger, /last_verified = "2026-08-10"/);
  assert.match(ledger, /steamcmd_anonymous_app_294420/);
  assert.match(ledger, /app_update 294420 validate/);
  assert.match(ledger, /build 24392395/);
  assert.match(ledger, /SizeOnDisk 17683442531/);
  assert.match(ledger, /V2\.6/);
  assert.match(ledger, /installed_serverconfig_xml/);
  assert.match(ledger, /serverconfig\.xml/);
  assert.match(ledger, /recorded serverconfig\.xml inventory contains 69 property names/);
  assert.match(ledger, /official V3\.0 release notes[^\n]+introduce SandboxCode/);
  assert.match(ledger, /This documentation check does not establish the currently installed game version\./);
  assert.match(ledger, /https:\/\/7-days-to-die\.zendesk\.com\/hc\/en-us\/articles\/50318172509972-V3-0-Dead-Hot-Summer-Release-Note/);
  assert.match(ledger, /startdedicated\.bat/);
  assert.match(ledger, /-configfile=serverconfig\.xml -dedicated/);
  assert.match(ledger, /no packaged serveradmin\.xml/);

  assert.equal(schema.properties.sandbox_code?.["x-lsgm-source-key"], "SandboxCode");
  assert.equal((template.match(/<property name=/g) ?? []).length, 69);
  assert.match(template, /<property name="SandboxCode" value="\{\{xml\.settings\.sandbox_code\}\}"\/>/);
  assert.doesNotMatch(template, /GameDifficulty|BiomeProgression|BloodMoonEnemyCount/);
  assert.match(settingsDefinition, /"sandbox_code"/);
  assert.equal(schema.properties.blacklist_entries.items.properties.unbandate.type, "string");
  assert.equal(schema.properties.blacklist_entries.items.properties.unbandate.default, "9999-12-31");
  assert.match(mockSettings, /unbandate:\s*"9999-12-31"/);
  assert.doesNotMatch(mockSettings, /unbandate:\s*0\b/);

  for (const staleKey of [
    "game_difficulty",
    "loot_abundance",
    "loot_respawn_days",
    "air_drop_frequency",
    "blood_moon_frequency",
    "blood_moon_range",
    "blood_moon_warning",
    "blood_moon_enemy_count",
    "drop_on_death",
    "zombie_move_night"
  ]) {
    const keyPattern = new RegExp(`\\b${staleKey}\\b`);
    assert.equal(schema.properties[staleKey], undefined);
    assert.doesNotMatch(settingsDefinition, keyPattern);
    assert.doesNotMatch(mockSettings, keyPattern);
  }
});

test("Abiotic Factor records current sandbox options and the original package evidence", () => {
  const ledger = read(path.join(abioticFactorRoot, "config-sources.toml"));

  assert.match(ledger, /last_verified = "2026-09-08"/);
  assert.match(ledger, /steamcmd_anonymous_app_2857200/);
  assert.match(ledger, /app_update 2857200 validate/);
  assert.match(ledger, /Success! App '2857200' fully installed/);
  assert.match(ledger, /StateFlags 4/);
  assert.match(ledger, /UpdateResult 0/);
  assert.match(ledger, /buildid\/TargetBuildID 23174893/);
  assert.match(ledger, /SizeOnDisk 2973839093/);
  assert.match(ledger, /no packaged SandboxSettings\.ini or Admin\.ini/);
});

test("Abiotic Factor records MultiHome as instance-owned instead of a game setting", () => {
  const schema = JSON.parse(read(path.join(abioticFactorRoot, "schema.json")));
  const ledger = read(path.join(abioticFactorRoot, "config-sources.toml"));
  const settingsDefinition = read(
    path.join(repoRoot, "apps", "desktop", "src", "views", "settings", "modules", "abioticfactor.ts")
  );
  const items = parseTomlArrayTables(ledger, "items");
  const exclusions = parseTomlArrayTables(ledger, "exclusions");

  assert.equal(Object.hasOwn(schema.properties, "multihome_address"), false);
  assert.equal(Object.hasOwn(schema.properties, "use_local_ips"), true);
  assert.equal(items.some((item) => item.schema_key === "multihome_address"), false);
  const multihomeExclusions = exclusions.filter(
    (exclusion) => exclusion.source === "launch_args" && exclusion.key === "-MultiHome"
  );
  assert.equal(multihomeExclusions.length, 1);
  assert.match(multihomeExclusions[0].reason, /instance-level bind address/i);
  assert.doesNotMatch(settingsDefinition, /\bmultihome_address\b/);
});

test("Abiotic Factor UE4SS server mods stage into the Win64 ue4ss Mods directory", () => {
  const moduleToml = read(path.join(abioticFactorRoot, "module.toml"));

  assert.match(moduleToml, /\[mods\.source\][\s\S]*provider = "nexus"/);
  assert.match(moduleToml, /\[mods\.source\][\s\S]*label = "Abiotic Factor UE4SS mods"/);
  assert.match(moduleToml, /url = "https:\/\/www\.nexusmods\.com\/abioticfactor"/);
  assert.match(
    moduleToml,
    /\[mods\.manual_staging\][\s\S]*target_template = "\{\{paths\.install_root\}\}\/AbioticFactor\/Binaries\/Win64\/ue4ss\/Mods"/
  );
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*target_label = "AbioticFactor\/Binaries\/Win64\/ue4ss\/Mods"/);
  assert.match(moduleToml, /\[mods\.manual_staging\][\s\S]*accepts = \["folder", "zip", "lua", "pak"\]/);
  assert.doesNotMatch(moduleToml, /\[mods\.enablement\]/);
});

test("Enshrouded records the current validated package and installed readme config evidence", () => {
  const ledger = read(path.join(enshroudedRoot, "config-sources.toml"));

  assert.match(ledger, /last_verified = "2026-09-08"/);
  assert.match(ledger, /Official role password[\s\S]*reverified 2026-09-08/);
  assert.match(ledger, /steamcmd_anonymous_probe_2278520/);
  assert.match(ledger, /StateFlags 4/);
  assert.match(ledger, /UpdateResult 0/);
  assert.match(ledger, /buildid and TargetBuildID 23178631/);
  assert.match(ledger, /SizeOnDisk 8808364250/);
  assert.match(ledger, /enshrouded_server_readme\.txt/);
  assert.match(ledger, /Version: 0\.9\.0\.0/);
  assert.match(ledger, /tags allow describing/);
  assert.match(ledger, /userGroups/);
  assert.match(ledger, /bans/);
});

test("V Rising anonymous package validate records installed StreamingAssets settings", () => {
  const ledger = read(path.join(vrisingRoot, "config-sources.toml"));

  assert.match(ledger, /last_verified = "2026-09-28"/);
  assert.match(ledger, /official_host_settings_1_1/);
  assert.match(ledger, /steamcmd_anonymous_app_1829350/);
  assert.match(ledger, /app_update 1829350 validate/);
  assert.match(ledger, /completed successfully/);
  assert.match(ledger, /buildid 23401381/);
  assert.match(ledger, /SizeOnDisk 2021415384/);
  assert.match(ledger, /VRisingServer_Data\/StreamingAssets\/Settings\/ServerHostSettings\.json/);
  assert.match(ledger, /VRisingServer_Data\/StreamingAssets\/Settings\/ServerGameSettings\.json/);
  assert.match(ledger, /WarEventGameSettings/);
  assert.match(ledger, /GameDifficultyPresets\/Difficulty_Brutal\.json/);
});


test("Squad anonymous package validate records installed ServerConfig templates", () => {
  const ledger = read(path.join(squadRoot, "config-sources.toml"));
  const serverTemplate = read(path.join(squadRoot, "templates", "Server.cfg.hbs"));
  const rconTemplate = read(path.join(squadRoot, "templates", "Rcon.cfg.hbs"));

  assert.match(ledger, /last_verified = "2026-07-13"/);
  assert.match(ledger, /steamcmd_anonymous_app_403240/);
  assert.match(ledger, /app_update 403240 validate/);
  assert.match(ledger, /buildid 23797339/);
  assert.match(ledger, /SizeOnDisk 14558146773/);
  assert.match(ledger, /Server\.cfg has 36 keys and Rcon\.cfg has 7 keys/);
  assert.match(ledger, /SquadGame\/ServerConfig\/Server\.cfg/);
  assert.match(ledger, /SquadGame\/ServerConfig\/Rcon\.cfg/);
  assert.match(ledger, /SquadGame\/ServerConfig\/Admins\.cfg/);
  assert.match(ledger, /LayerVoting\.cfg/);
  assert.equal(serverTemplate.trim().split(/\r?\n/).length, 36);
  assert.equal(rconTemplate.trim().split(/\r?\n/).length, 7);
  assert.match(serverTemplate, /AllowFireteamLayersInRotation=/);
});

test("ARK Survival Ascended preserves package evidence alongside independently verified settings", () => {
  const ledger = read(path.join(arkSurvivalAscendedRoot, "config-sources.toml"));

  assert.match(ledger, /last_verified = "2026-09-08"/);
  assert.match(ledger, /id = "asa_server_patch_notes_v93_19"/);
  assert.match(ledger, /ServerSettings\.PreventTemplateOnSaddle/);
  assert.match(ledger, /^status = "best_effort_verified"/);
  assert.match(ledger, /id = "asa_advanced_game_ini"/);
  assert.match(ledger, /94cf4d4258aae0f5190781cddfe24a327c149503/);
  assert.match(ledger, /72b00288f130913ad6dfac1b680c62c159e76e86c70b3f90a2369260d63d5f63/);
  assert.match(ledger, /steamcmd_anonymous_app_2430930/);
  assert.match(ledger, /app_update 2430930 validate/);
  assert.match(ledger, /completed successfully/);
  assert.match(ledger, /StateFlags 4/);
  assert.match(ledger, /UpdateResult 0/);
  assert.match(ledger, /buildid\/TargetBuildID 24159508/);
  assert.match(ledger, /SizeOnDisk 12835784254/);
  assert.match(ledger, /no packaged DefaultGameUserSettings\.ini or DefaultGame\.ini/);
  assert.match(ledger, /ShooterGame\/Saved\/Config\/WindowsServer\/GameUserSettings\.ini is LanGame materialized output/);
});

test("ARK Survival Evolved anonymous package validate records packaged defaults and live config paths", () => {
  const ledger = read(path.join(arkSurvivalEvolvedRoot, "config-sources.toml"));
  const userSettingsTemplate = read(path.join(arkSurvivalEvolvedRoot, "templates/GameUserSettings.ini.hbs"));
  const nativeSettingsFixture = JSON.parse(read(path.join(
    arkSurvivalEvolvedRoot,
    "config-fixtures/2026-09-08-evolved_native_user_settings_version.json",
  )));

  assert.match(ledger, /last_verified = "2026-09-08"/);
  assert.match(ledger, /id = "evolved_native_user_settings_version"/);
  assert.match(ledger, /issuecomment-274333283/);
  assert.match(userSettingsTemplate, /\[\/Script\/ShooterGame\.ShooterGameUserSettings\]\r?\nVersion=5(?:\r?\n|$)/);
  assert.deepEqual(nativeSettingsFixture.evidence.source_ids, ["evolved_native_user_settings_version"]);
  const nativeOutput = nativeSettingsFixture.expected.files.find((file) => file.root === "install");
  assert.equal(nativeOutput.path, "ShooterGame/Saved/Config/WindowsServer/GameUserSettings.ini");
  for (const fragment of [
    "[/Script/ShooterGame.ShooterGameUserSettings]\nVersion=5",
    "SessionName=LanGame ASE Native Settings",
    "MaxPlayers=8",
    "RCONEnabled=true",
    "RCONPort=27020",
  ]) {
    assert.ok(nativeOutput.fragments.includes(fragment), `ASE native startup fixture must retain ${fragment}`);
  }
  assert.match(ledger, /72b00288f130913ad6dfac1b680c62c159e76e86c70b3f90a2369260d63d5f63/);
  assert.match(ledger, /steamcmd_anonymous_app_376030/);
  assert.match(ledger, /app_update 376030 validate/);
  assert.match(ledger, /Success! App '376030' fully installed/);
  assert.match(ledger, /StateFlags 4/);
  assert.match(ledger, /UpdateResult 0/);
  assert.match(ledger, /buildid\/TargetBuildID 21241282/);
  assert.match(ledger, /SizeOnDisk 22895670223/);
  assert.match(ledger, /ShooterGame\/Config\/DefaultGameUserSettings\.ini/);
  assert.match(ledger, /ShooterGame\/Config\/DefaultGame\.ini/);
  assert.match(ledger, /ShooterGame\/Saved\/Config\/WindowsServer\/GameUserSettings\.ini/);
  assert.match(ledger, /PlayersExclusiveJoinList\.txt/);
  assert.match(ledger, /AllowedCheaterSteamIDs\.txt/);
});

test("Rust deep-sea terrain is an opt-in world-generation setting", () => {
  const schema = JSON.parse(read(path.join(rustRoot, "schema.json")));
  const field = schema.properties.deep_sea_terrain_everywhere;
  assert.ok(field, "the September update terrain option must be configurable");
  assert.equal(field.type, "boolean");
  assert.equal(field.default, false, "new instances preserve the native terrain behavior");
  assert.equal(field["x-lsgm-section"], "world");
  assert.equal(field["x-lsgm-source-key"], "deepsea.terrain_everywhere");
  assert.equal(field["x-lsgm-player-access-kind"], undefined);

  const filename = path.join(repoRoot, "apps/desktop/src/views/settings/modules/rust.ts");
  const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
  const Module = require("node:module");
  const compiled = new Module(filename, module);
  compiled.filename = filename;
  compiled.paths = module.paths;
  compiled._compile(transpileTypeScript(read(filename), filename), filename);
  const groups = compiled.exports.rustSettingsDefinition.buildFieldGroups(
    "world", [{ key: "deep_sea_terrain_everywhere", ...field }], "en-US",
    (_key, _parameters, fallback) => fallback ?? ""
  );
  assert.equal(groups.length, 1);
  assert.equal(groups[0].id, "world-generation");
  assert.deepEqual(groups[0].fields.map(({ key }) => key), ["deep_sea_terrain_everywhere"]);
});
