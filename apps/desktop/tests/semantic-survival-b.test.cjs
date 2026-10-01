const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const Module = require("node:module");
const React = require("react");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const root = path.resolve(__dirname, "../../..");
const settingsRoot = path.join(root, "apps/desktop/src/views/settings");
for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = () => {};
require.extensions[".png"] = (module, filename) => { module.exports = filename; };
const { resolveSettingsModuleDefinition } = require(path.join(settingsRoot, "module-registry.ts"));

const expectedRooms = {
  romestead: ["auto_start_world_name", "password", "max_players"],
  runescapedragonwilds: ["server_name", "default_world_name", "world_password"],
  rust: ["server_name", "server_description", "max_players", "tags", "website_url", "header_image_url", "logo_image_url", "level", "level_url", "censor_player_list"],
  satisfactory: ["max_players"],
  sonsoftheforest: ["server_name", "max_players", "server_password", "lan_only", "save_slot"],
  soulmask: ["server_name", "server_password", "max_players"],
  squad: ["server_name", "max_players", "advertise", "lan_only", "map_rotation", "layer_rotation", "level_rotation", "motd_cfg", "server_message", "server_tags", "server_rules", "server_message_interval_seconds"],
  terraria: ["world_name", "world_file", "motd", "max_players", "password", "lobby", "language"],
  theforest: ["server_name", "max_players", "server_password", "save_slot"],
  unturned: ["server_name", "internet_server", "map", "max_players", "password", "welcome_message", "browser_icon_url", "browser_desc_hint", "browser_desc_full", "browser_links_json", "native_browser_desc_server_list", "native_browser_monetization", "native_browser_thumbnail"],
  valheim: ["server_name", "world_name", "server_password", "public_server"],
  vrising: ["server_name", "server_description", "max_players", "server_password", "list_on_eos", "save_name", "list_on_steam", "safe_reconnect_slots", "safe_reconnect_time"],
  windrose: ["server_name", "server_password", "invite_code", "max_players", "user_selected_region", "world_island_id", "world_name"]
};

const schema = (moduleId) => JSON.parse(fs.readFileSync(path.join(root, "modules", moduleId, "schema.json"), "utf8"));

for (const [moduleId, expected] of Object.entries(expectedRooms)) {
  test(`${moduleId} room owns only its supported identity, join, listing and map selection fields`, () => {
    const actual = Object.entries(schema(moduleId).properties)
      .filter(([, property]) => property["x-lsgm-section"] === "room")
      .map(([key]) => key);
    assert.deepEqual(actual.sort(), [...expected].sort());
  });
}

test("section field ordering stays unique after semantic moves", () => {
  for (const moduleId of Object.keys(expectedRooms)) {
    const seen = new Map();
    for (const [key, property] of Object.entries(schema(moduleId).properties)) {
      const slot = `${property["x-lsgm-section"]}:${property["x-lsgm-order"]}`;
      assert.equal(seen.has(slot), false, `${moduleId}.${key} collides with ${seen.get(slot)} at ${slot}`);
      seen.set(slot, key);
    }
  }
});

test("permissions, network services and simulation controls keep separate semantic ownership", () => {
  const expected = {
    romestead: { enable_cheats: "access", auto_create_world_seed: "world" },
    runescapedragonwilds: { owner_id: "access", admin_password: "access" },
    rust: { server_gamemode: "gamemode", pve: "gamemode", favorites_endpoint: "network", seed: "world" },
    satisfactory: { disable_seasonal_events: "world" },
    sonsoftheforest: { game_mode: "world", save_mode: "world" },
    soulmask: { pvp_mode_arg: "world", admin_password: "access" },
    squad: { enforce_team_balance: "world", tk_auto_kick_enabled: "world", reserved_slots: "access", public_queue_limit: "access", admins_cfg: "access", joining_player_timeout_seconds: "network", remote_admin_hosts: "access", remote_ban_hosts: "access", excluded_factions: "world", excluded_layers: "world", excluded_levels: "world" },
    terraria: { upnp: "network", steam: "network", seed: "world", disableannouncementbox: "world", announcementboxrange: "world" },
    theforest: { admin_password: "access", allow_cheats: "access" },
    valheim: { crossplay_enabled: "network", instance_id: "network", custom_launch_flags: "advanced", log_file: "advanced" },
    vrising: { max_admins: "access", server_fps: "advanced", hide_ip_address: "network", admin_only_debug_events: "access", api_enabled: "network" },
    windrose: { allow_multiple_server_instances: "advanced", auto_load_latest_backup_if_has_broken: "advanced" }
  };
  for (const [moduleId, fields] of Object.entries(expected)) {
    for (const [key, section] of Object.entries(fields)) {
      assert.equal(schema(moduleId).properties[key]["x-lsgm-section"], section, `${moduleId}.${key}`);
    }
  }
});

test("Terraria Journey permission levels are nested under permissions", () => {
  const { terrariaSettingsDefinition } = require(path.join(settingsRoot, "modules/terraria.ts"));
  const sections = terrariaSettingsDefinition.getSections((key, _params, fallback) => fallback ?? key);
  assert.equal(sections.find((section) => section.id === "journey")?.parentId, "access");
});

test("Windrose world identity renderer follows the room without moving world rules", () => {
  const { windroseSettingsDefinition } = require(path.join(settingsRoot, "modules/windrose.ts"));
  const { fieldPresentationOverrides, specializedRenderers } = windroseSettingsDefinition;
  const name = fieldPresentationOverrides.world_name;
  assert.equal(name.sectionId, "room");
  assert.equal(specializedRenderers[name.rendererId].sectionId, "room");
  assert.equal(specializedRenderers[name.rendererId].fieldKey, undefined,
    "the real input owns the focus ID; its add-on wrapper must not duplicate it");
  for (const [key, presentation] of Object.entries(fieldPresentationOverrides)) {
    if (key === "world_name") continue;
    assert.equal(presentation.sectionId, "world", key);
    assert.equal(specializedRenderers[presentation.rendererId].sectionId, "world", key);
  }
});

test("Windrose room name retains selection and process guards and emits only its own setting patch", () => {
  const filename = path.join(settingsRoot, "WindroseWorldSettingsPanel.tsx");
  const loaded = new Module(filename, module);
  const originalRequire = Module.createRequire(filename);
  loaded.filename = filename;
  loaded.require = (id) => id === "../../i18n" ? {
    useI18n: () => ({ t: (key, _params, fallback) => fallback ?? key })
  } : id === "./ConfigurationFieldHelp" ? {
    useConfigurationFieldHelp: () => ({ anchorRef() {}, interactionProps: {}, helpNode: null })
  } : originalRequire(id);
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  function findInput(element) {
    if (!React.isValidElement(element)) return null;
    if (element.type === "input") return element;
    if (typeof element.type === "function") return findInput(element.type(element.props));
    for (const child of React.Children.toArray(element.props.children)) {
      const input = findInput(child);
      if (input) return input;
    }
    return null;
  }
  for (const [status, worldId, disabled, expected] of [
    ["stopped", "island-one", false, false],
    ["stopped", "", false, true],
    ["starting", "island-one", false, true],
    ["running", "island-one", false, true],
    ["stopping", "island-one", false, true],
    ["stopped", "island-one", true, true]
  ]) {
    let patch;
    const input = findInput(loaded.exports.WindroseWorldNameField({
      details: { summary: { status } }, settings: { world_island_id: worldId, world_name: "Existing world" },
      disabled, onPatch: (value) => { patch = value; }
    }));
    assert.ok(input);
    assert.equal(input.props.disabled, expected, `${status}/${worldId}/${disabled}`);
    assert.equal(input.props.value, "Existing world");
    if (!expected) {
      input.props.onChange({ target: { value: "Harbor" } });
      assert.deepEqual(patch, { world_name: "Harbor" });
    }
  }
});

test("Windrose full room renders each setting and input ID exactly once", () => {
  const { renderToStaticMarkup } = require("react-dom/server");
  const { I18nContext } = require("../src/i18n-context.ts");
  const filename = path.join(settingsRoot, "ConfigurationWorkspace.tsx");
  const loaded = new Module(filename, module);
  const originalRequire = Module.createRequire(filename);
  loaded.filename = filename;
  loaded.require = (id) => id === "./useAutoSaveInstanceSettings" ? {
    useAutoSaveInstanceSettings: () => ({ status: { state: "saved" }, retry() {} })
  } : id === "./useInstancePortRegistration" ? {
    useInstancePortRegistration: () => ({ ports: [], defaultPorts: [], setPorts() {} })
  } : id === "./useModuleConfigurationIcons" ? {
    useModuleConfigurationIcons: () => ({ icons: {} })
  } : originalRequire(id);
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  const settings = Object.fromEntries(Object.entries(schema("windrose").properties)
    .map(([key, property]) => [key, property.default]));
  const props = {
    details: {
      summary: { id: "windrose-semantic", module_id: "windrose", name: "Harbor", status: "stopped", bind_ip: "127.0.0.1" },
      settings_json: JSON.stringify({ ...settings, world_island_id: "island-one" }),
      auto_backup_on_stop: false, backup_retention_count: 3
    },
    moduleDetails: { summary: { id: "windrose", name: "Windrose" }, schema_json: JSON.stringify(schema("windrose")) },
    bindAddressCandidates: [], runtime: null, launchPlan: null, launchPlanError: null,
    onSave() {}
  };
  const html = renderToStaticMarkup(React.createElement(I18nContext.Provider, {
    value: { locale: "en-US", setLocale() {}, t: (key, _params, fallback) => fallback ?? key }
  }, React.createElement(loaded.exports.ConfigurationWorkspace, props)));
  const fieldKeys = [...html.matchAll(/data-field-key="([^"]+)"/g)].map((match) => match[1]);
  assert.deepEqual(fieldKeys.sort(), [...expectedRooms.windrose].sort());
  const ids = [...html.matchAll(/\bid="([^"]+)"/g)].map((match) => match[1]);
  assert.equal(ids.length, new Set(ids).size, "all room DOM IDs are unique");
  assert.equal((html.match(/<input[^>]*id="configuration-windrose-world-name-input"/g) ?? []).length, 1);
  assert.doesNotMatch(html, /A specialized editor is unavailable/);
});
