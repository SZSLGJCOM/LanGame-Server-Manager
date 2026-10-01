const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) =>
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}
require.extensions[".css"] = (module, filename) => module._compile("", filename);
require.extensions[".png"] = (module, filename) => module._compile(`module.exports = ${JSON.stringify(filename)};`, filename);

const { parseGuidedSettingsSchema } = require("../src/views/settings/guided-settings.ts");
const { buildConfigurationWorkspaceModel, resolveConfigurationSectionId } = require("../src/views/settings/configuration-workspace-model.ts");
const { resolveSettingsModuleDefinition, listSettingsModuleIds } = require("../src/views/settings/module-registry.ts");
const { EN_US_MESSAGES } = require("../src/i18n-messages.ts");
const { ZH_CN_MESSAGES } = require("../src/i18n-messages-zh-cn.ts");
const modulesRoot = path.resolve(__dirname, "../../../modules");

// Representative room controls for every supported game, including native aliases
// and SCUM's nested properties. These describe user tasks, not file sections.
const roomControls = {
  abioticfactor: ["server_name", "world_save_name", "max_server_players", "server_password"],
  arksurvivalascended: ["server_name", "max_players", "map_name", "server_password"],
  arksurvivalevolved: ["server_name", "max_players", "map_name", "server_password"],
  astroneer: ["server_name", "max_players", "server_password"],
  barotrauma: ["server_name", "max_players", "server_password", "public_server"],
  conanexiles: ["server_name", "max_players", "server_password", "public_server"],
  corekeeper: ["server_name", "max_players", "join_password"],
  dontstarve: ["cluster_name", "cluster_description", "max_players", "cluster_password"],
  enshrouded: ["server_name", "max_players", "server_tags"],
  humanitz: ["server_name", "max_players", "server_password", "save_name"],
  minecraft: ["motd", "max_players", "level_name"],
  necesse: ["world_name", "max_slots", "password"],
  nightingale: ["max_players", "server_password"],
  palworld: ["server_name", "max_players", "server_password", "community_server"],
  projectzomboid: ["server_name", "max_players", "server_password"],
  returntomoria: ["server_name", "server_password", "world_file_name"],
  rimworld: ["server_name", "server_description", "max_players"],
  romestead: ["max_players", "password", "auto_start_world_name"],
  runescapedragonwilds: ["server_name", "world_password", "default_world_name"],
  rust: ["server_name", "server_description", "max_players"],
  satisfactory: ["max_players"],
  scum: ["server_general.server_name", "server_general.server_description", "server_general.max_players", "server_general.server_password"],
  sevendaystodie: ["server_name", "max_players", "server_password", "game_world", "world_name"],
  sonsoftheforest: ["server_name", "max_players", "server_password"],
  soulmask: ["server_name", "max_players", "server_password"],
  squad: ["server_name", "max_players", "server_message"],
  terraria: ["world_name", "world_file", "max_players", "password"],
  theforest: ["server_name", "max_players", "server_password"],
  unturned: ["server_name", "max_players", "password", "map"],
  valheim: ["server_name", "world_name", "server_password", "public_server"],
  vrising: ["server_name", "server_description", "max_players", "server_password", "save_name"],
  windrose: ["server_name", "max_players", "server_password", "world_name"]
};

function schemaFor(moduleId, locale, catalog) {
  const t = (key, _params, fallback) => catalog[key] ?? fallback ?? key;
  const schema = parseGuidedSettingsSchema({
    summary: { id: moduleId, name: moduleId },
    schema_json: fs.readFileSync(path.join(modulesRoot, moduleId, "schema.json"), "utf8")
  }, locale, t);
  assert.equal(schema.parseError, null, moduleId);
  return { schema, t };
}

test("every game starts with the same room task, including ARK and nested native settings", () => {
  assert.deepEqual(Object.keys(roomControls).sort(), listSettingsModuleIds());
  for (const [locale, catalog] of [["zh-CN", ZH_CN_MESSAGES], ["en-US", EN_US_MESSAGES]]) {
    for (const [moduleId, keys] of Object.entries(roomControls)) {
      const { schema, t } = schemaFor(moduleId, locale, catalog);
      const model = buildConfigurationWorkspaceModel(schema);
      assert.equal(resolveConfigurationSectionId(model), "room", moduleId);
      assert.equal(model.roots[0].id, "room", moduleId);
      assert.equal(model.roots[0].title, locale === "zh-CN" ? "房间配置" : "Room Settings");
      for (const key of keys) {
        const items = model.items.filter((item) => item.fieldKey === key);
        assert.equal(items.length, 1, `${moduleId}.${key} has one entry`);
        assert.equal(items[0].sectionId, "room", `${moduleId}.${key}`);
        assert.equal(items[0].owner, "configuration", `${moduleId}.${key}`);
      }
      const fields = schema.fields.filter((field) => field.sectionId === "room");
      if (fields.length > 0) {
        const groups = resolveSettingsModuleDefinition(moduleId).buildFieldGroups("room", fields, locale, t);
        assert.equal(groups.length, 1, `${moduleId} has one room form`);
        assert.ok(!groups[0].title, `${moduleId} does not repeat a heading or add an 'additional' group`);
        assert.deepEqual(groups[0].fields.map((field) => field.key), fields.map((field) => field.key));
      }
    }
  }
});

test("gameplay, process tuning and operator credentials never masquerade as room identity", () => {
  const misplacedControls = {
    arksurvivalascended: ["active_event", "admin_password"],
    arksurvivalevolved: ["active_event", "admin_password"],
    barotrauma: ["tick_rate", "randomize_seed", "karma_enabled"],
    dontstarve: ["game_mode", "pvp", "vote_enabled", "pause_when_empty", "cluster_token"],
    enshrouded: ["game_settings_preset", "enable_voice_chat", "enable_text_chat"],
    humanitz: ["pvp_enabled"],
    projectzomboid: ["pause_empty", "global_chat", "chat_streams"],
    rust: ["server_gamemode"],
    sonsoftheforest: ["game_mode"],
    soulmask: ["pvp_mode_arg"],
    squad: ["allow_team_changes", "enforce_team_balance", "tk_auto_kick_enabled"],
    vrising: ["server_fps", "max_admins"]
  };
  for (const [moduleId, keys] of Object.entries(misplacedControls)) {
    const { schema } = schemaFor(moduleId, "en-US", EN_US_MESSAGES);
    for (const key of keys) {
      const field = schema.presentationFields.find((entry) => entry.key === key);
      assert.ok(field, `${moduleId}.${key} remains available`);
      assert.notEqual(field.sectionId, "room", `${moduleId}.${key}`);
    }
  }
});

test("common permission and runtime categories keep semantic child sections and localized titles", () => {
  for (const [locale, catalog] of [["zh-CN", ZH_CN_MESSAGES], ["en-US", EN_US_MESSAGES]]) {
    for (const moduleId of listSettingsModuleIds()) {
      const { schema } = schemaFor(moduleId, locale, catalog);
      const sections = new Map(schema.sections.map((section) => [section.id, section]));
      assert.equal(sections.get("access")?.title, locale === "zh-CN" ? "管理权限" : "Administration & Permissions", moduleId);
      assert.equal(sections.get("runtime")?.title, locale === "zh-CN" ? "运行与高级" : "Runtime & Advanced", moduleId);
      for (const id of ["admin", "join", "admin_role", "friend_role", "guest_role", "visitor_role"]) {
        if (sections.has(id)) assert.equal(sections.get(id).parentId, "access", `${moduleId}.${id}`);
      }
      for (const id of ["advanced", "performance", "host", "raw", "services"]) {
        if (sections.has(id)) assert.equal(sections.get(id).parentId, "runtime", `${moduleId}.${id}`);
      }
    }
  }
});


test("every visible game setting has exactly one group without implementation-status copy", () => {
  for (const [locale, catalog] of [["zh-CN", ZH_CN_MESSAGES], ["en-US", EN_US_MESSAGES]]) {
    for (const moduleId of listSettingsModuleIds()) {
      const { schema, t } = schemaFor(moduleId, locale, catalog);
      const definition = resolveSettingsModuleDefinition(moduleId);
      for (const section of schema.sections) {
        const fields = schema.fields.filter((field) => field.sectionId === section.id);
        if (fields.length === 0) continue;
        const groups = definition.buildFieldGroups?.(section.id, fields, locale, t) ?? [{ fields }];
        const rendered = groups.flatMap((group) => group.fields.map((field) => field.key));
        assert.deepEqual(rendered.toSorted(), fields.map((field) => field.key).toSorted(),
          `${moduleId}.${section.id} keeps every setting once`);
        for (const group of groups) {
          assert.doesNotMatch(`${group.title ?? ""} ${group.description ?? ""}`,
            /workflow group yet|without a dedicated|do not have a dedicated|暂时还没有|尚未.*分组/,
            `${moduleId}.${section.id} uses operator-facing copy`);
        }
      }
    }
  }
});
