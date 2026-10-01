const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = (module) => module._compile("module.exports = {};", module.filename);

const { parseGuidedSettingsSchema } = require("../src/views/settings/guided-settings.ts");
const { resolveSettingsModuleDefinition } = require("../src/views/settings/module-registry.ts");
const { buildConfigurationWorkspaceModel } = require("../src/views/settings/configuration-workspace-model.ts");

const roomInventory = {
  abioticfactor: ["server_name", "world_save_name", "max_server_players", "server_password", "lan_only"],
  astroneer: ["server_name", "max_players", "server_password", "active_save_file_name"],
  barotrauma: ["server_name", "server_message", "server_password", "max_players", "public_server", "play_style", "language"],
  conanexiles: ["server_name", "server_message_of_the_day", "excluded_regions", "max_players", "server_region", "public_server", "server_password", "show_online_players"],
  corekeeper: ["server_name", "game_id", "max_players", "world_index", "join_password"],
  enshrouded: ["server_name", "max_players", "server_tags"],
  humanitz: ["server_name", "max_players", "search_id", "welcome_message", "server_password", "save_name"],
  minecraft: ["motd", "max_players", "enable_status", "hide_online_players", "level_name", "bug_report_link"],
  necesse: ["world_name", "max_slots", "motd", "password", "language"],
  nightingale: ["server_password", "max_players"],
  palworld: ["server_name", "server_description", "max_players", "server_password", "community_server", "region", "show_player_list"],
  projectzomboid: ["server_name", "server_description", "welcome_message", "max_players", "public_server", "server_password"],
  returntomoria: ["server_name", "server_password", "world_file_name", "rules_message"],
  rimworld: ["server_name", "server_description", "max_players", "enable_server_browser", "discord_url", "steam_workshop_url", "server_password", "chat_login_notifications", "chat_disconnect_notifications", "chat_enable_mo_td", "chat_message_of_the_day"]
};

function parseModule(moduleId) {
  const schema = parseGuidedSettingsSchema({
    summary: { id: moduleId, name: moduleId },
    schema_json: fs.readFileSync(path.resolve(__dirname, "../../../modules", moduleId, "schema.json"), "utf8")
  });
  assert.equal(schema.parseError, null, moduleId);
  return schema;
}

for (const [moduleId, keys] of Object.entries(roomInventory)) {
  test(`${moduleId} room contains only its supported identity, join, listing, and world-selection fields`, () => {
    const schema = parseModule(moduleId);
    const model = buildConfigurationWorkspaceModel(schema);
    assert.deepEqual(model.items.filter((item) => item.sectionId === "room" && item.owner === "configuration")
      .map((item) => item.fieldKey).sort(), [...keys].sort());
    for (const key of keys) {
      assert.equal(model.items.filter((item) => item.fieldKey === key).length, 1, key);
    }
  });
}

test("RimWorld player notifications preserve the tagged native ChatConfig contract", () => {
  const modulePath = path.resolve(__dirname, "../../../modules/rimworld");
  const native = JSON.parse(fs.readFileSync(path.join(modulePath, "native-settings-26.8.31.1.json"), "utf8"));
  const fixture = JSON.parse(fs.readFileSync(path.join(modulePath, "config-fixtures/2026-09-28-rimworld_together_latest_release_api.json"), "utf8"));
  const chatConfig = native.records.find((record) => record.path === "Configs/ChatConfig.json");
  const expectedFile = fixture.expected.files.find((file) => file.path === "Configs/ChatConfig.json");
  const schema = JSON.parse(fs.readFileSync(path.join(modulePath, "schema.json"), "utf8"));
  assert.equal(native.release, "26.8.31.1");
  assert.equal(chatConfig.type, "RTShared.Files.Configs.FL_ChatConfig");
  for (const [key, nativeKey] of [
    ["chat_login_notifications", "LoginNotifications"],
    ["chat_disconnect_notifications", "DisconnectNotifications"]
  ]) {
    assert.deepEqual(chatConfig.fields[nativeKey], {
      type: "boolean", signature: "06-02", default: false, schema_key: key
    });
    assert.equal(expectedFile.keys[nativeKey], true);
    const property = schema.properties[key];
    assert.equal(property.type, "boolean", key);
    assert.equal(property["x-lsgm-source"], "shared_config_26_8_31_1", key);
    assert.equal(property["x-lsgm-source-key"], `Configs/ChatConfig.json#${nativeKey}`, key);
    assert.equal(property["x-lsgm-default-source"], "existing_native_configuration", key);
    assert.equal(Object.hasOwn(property, "default"), false, `${key} remains an optional native override`);
  }
});

test("survival settings keep gameplay, operator credentials, and network controls in their own sections", () => {
  const expected = {
    astroneer: { console_password: "network", server_owner_display_name: "access", owner_guid: "access" },
    barotrauma: { tick_rate: "runtime", randomize_seed: "round", karma_enabled: "gameplay" },
    corekeeper: { world_seed: "world", world_mode: "world", allowed_platform_code: "access" },
    enshrouded: { voice_chat_mode: "communication", enable_voice_chat: "communication", enable_text_chat: "communication", game_settings_preset: "world" },
    humanitz: { pvp_enabled: "world" },
    nightingale: { admin_password: "access", enable_cheats: "access" },
    projectzomboid: { pause_empty: "services", global_chat: "moderation", chat_streams: "moderation", admin_username: "access", admin_password: "access", upnp: "network" },
    returntomoria: { world_seed: "world", console_enabled: "runtime", advertise_address: "network", advertise_port: "network", permissions_lines: "access" },
    rimworld: { verbosity: "runtime", display_chat_in_console: "runtime", sync_local_save: "runtime" }
  };
  for (const [moduleId, fields] of Object.entries(expected)) {
    const schema = parseModule(moduleId);
    for (const [key, sectionId] of Object.entries(fields)) {
      assert.equal(schema.presentationFields.find((field) => field.key === key)?.sectionId, sectionId, `${moduleId}.${key}`);
    }
  }
  const enshrouded = resolveSettingsModuleDefinition("enshrouded");
  const sections = enshrouded.getSections((_key, _params, fallback) => fallback);
  for (const id of ["admin_role", "friend_role", "guest_role", "visitor_role"]) {
    assert.equal(sections.find((section) => section.id === id)?.parentId, "access", id);
  }
});

test("native gameplay and access policies do not fall into runtime or transport categories", () => {
  const expected = {
    barotrauma: { lines_per_log_file: "runtime", kill_disconnected_time: "round", despawn_disconnected_permadeath_time: "round", max_transport_time: "round", allow_remote_campaign_interactions: "campaign", allow_spectating: "gameplay" },
    humanitz: { limited_spawns: "world", no_death_feedback: "world", no_join_feedback: "world", map_segment_0: "world", map_segment_1: "world", map_segment_2: "world", voip_enabled: "network" },
    minecraft: { log_ips: "advanced", allow_flight: "access", chat_spam_threshold_seconds: "access", command_spam_threshold_seconds: "access", text_filtering_config: "access", text_filtering_version: "access", initial_enabled_packs: "world", initial_disabled_packs: "world", resource_pack: "world", resource_pack_id: "world", resource_pack_sha1: "world", resource_pack_prompt: "world", require_resource_pack: "world" },
    necesse: { ignore_seasons: "world", strict_server_authority: "access", max_client_latency_seconds: "network", unload_levels_cooldown: "host", unload_settlements: "host", max_settlements_per_player: "world", world_border_size: "world" },
    palworld: { base_camp_max_num: "world", base_camp_max_num_in_guild: "world", base_camp_worker_max_num: "world", guild_player_max_num: "world", guild_rejoin_cooldown_minutes: "world", max_building_limit_num: "world", max_building_limit_num_per_player: "world", drop_item_alive_max_hours: "world", join_left_message: "world", enable_building_player_uid_display: "world", chat_post_limit_per_minute: "access", enable_voice_chat: "network", voice_chat_max_volume_distance: "network", voice_chat_zero_volume_distance: "network" },
    projectzomboid: { anti_cheat_speed: "anticheat", war: "safehouses", max_packets_per_second: "network", chat_message_character_limit: "moderation", disable_vehicle_towing: "world", display_user_name: "world", show_first_and_last_name: "world", steam_scoreboard: "world", mouse_over_to_see_display_name: "world", hide_players_behind_you: "world", login_queue_enabled: "join", login_queue_connect_timeout: "network", voice_enable: "network", voice_min_distance: "network", voice_max_distance: "network", voice_3d: "network" },
    rust: { bans_server_endpoint: "access", bans_server_failure_mode: "access", bans_server_timeout_seconds: "access", reports_print_to_console: "access", reports_server_endpoint: "access", reports_server_endpoint_key: "access" },
    satisfactory: { net_connection_timeout_seconds: "network", net_initial_connect_timeout_seconds: "network", net_max_client_rate: "network", net_max_internet_client_rate: "network" },
    sonsoftheforest: { idle_day_cycle_speed: "world" },
    unturned: { timeout_seconds: "network", max_ping: "network" }
  };
  for (const [moduleId, fields] of Object.entries(expected)) {
    const schema = parseModule(moduleId);
    const model = buildConfigurationWorkspaceModel(schema);
    for (const [key, sectionId] of Object.entries(fields)) {
      const item = model.items.find((entry) => entry.fieldKey === key);
      assert.equal(item?.sectionId, sectionId, `${moduleId}.${key}`);
      assert.equal(item.owner, "configuration", `${moduleId}.${key}`);
      assert.ok(model.actionableSectionIds.includes(sectionId), `${moduleId}.${key} is reachable`);
    }
  }
});
