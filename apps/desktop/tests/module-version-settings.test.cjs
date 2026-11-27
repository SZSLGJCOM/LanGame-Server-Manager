const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = (module) => module._compile("module.exports = {};", module.filename);

const root = path.resolve(__dirname, "../../..");
const {
  parseGuidedSettingsSchema,
  readGuidedFieldValue,
  writeGuidedFieldValue,
  validateGuidedSettingsObject
} = require("../src/views/settings/guided-settings.ts");
const { isOptionalBooleanOverride } = require("../src/views/settings/guided-setting-values.ts");
const { ConfigurationField } = require("../src/views/settings/ConfigurationField.tsx");
const { resolveSettingsModuleDefinition } = require("../src/views/settings/module-registry.ts");
const { buildConfigurationWorkspaceModel, searchConfigurationItems } = require("../src/views/settings/configuration-workspace-model.ts");
const { EN_US_MESSAGES } = require("../src/i18n-messages.ts");
const { ZH_CN_MESSAGES } = require("../src/i18n-messages-zh-cn.ts");

function translator(catalog) {
  return (key, params, fallback) => String(catalog[key] ?? fallback ?? key).replace(
    /\{\s*([\w.]+)\s*\}/g,
    (match, paramKey) => params?.[paramKey] == null ? match : String(params[paramKey])
  );
}

function readSchema(moduleId) {
  return JSON.parse(fs.readFileSync(path.join(root, "modules", moduleId, "schema.json"), "utf8"));
}

function parsed(moduleId, locale = "en-US") {
  const catalog = locale === "zh-CN" ? ZH_CN_MESSAGES : EN_US_MESSAGES;
  const t = translator(catalog);
  const schema = parseGuidedSettingsSchema({
    summary: { id: moduleId, name: moduleId },
    schema_json: JSON.stringify(readSchema(moduleId))
  }, locale, t);
  assert.equal(schema.parseError, null, moduleId);
  return { schema, t, catalog };
}

function issuesFor(schema, key, value) {
  return validateGuidedSettingsObject(schema, { [key]: value })
    .filter((issue) => issue.fieldKey === key);
}

test("Rust optional overrides, population controls, navmesh default and performance controls behave independently", () => {
  const { schema, t } = parsed("rust");
  const rustDefinition = resolveSettingsModuleDefinition("rust");
  const pad = schema.fields.find((entry) => entry.key === "vehicle_pad_repairs_required");
  assert.ok(pad);
  assert.equal(isOptionalBooleanOverride(pad), true);
  assert.equal(pad.defaultValue, undefined);
  assert.equal(readGuidedFieldValue(pad, {}), undefined);
  const padHtml = renderToStaticMarkup(React.createElement(ConfigurationField, {
    field: pad, value: undefined, settings: {}, onPatch() {},
    copy: {
      concealSecret: "Hide", revealSecret: "Show", useGameDefault: "Use game setting",
      restartScopes: { none: "None", server: "Restart server", world: "Restart world", cluster: "Restart cluster" }
    }
  }));
  assert.match(padHtml, /<select/);
  assert.match(padHtml, /value="" selected=""/);
  const disabled = writeGuidedFieldValue({}, pad, false);
  assert.equal(disabled.vehicle_pad_repairs_required, false);
  const enabled = writeGuidedFieldValue(disabled, pad, true);
  assert.equal(readGuidedFieldValue(pad, enabled), true);
  assert.deepEqual(writeGuidedFieldValue(enabled, pad, undefined), {});

  const low = schema.fields.find((entry) => entry.key === "powergrid_fuse_decay_low_pop");
  const scale = schema.fields.find((entry) => entry.key === "powergrid_fuse_decay_low_pop_scale");
  assert.equal(writeGuidedFieldValue({}, low, "0").powergrid_fuse_decay_low_pop, 0);
  assert.equal(writeGuidedFieldValue({}, scale, "0.125").powergrid_fuse_decay_low_pop_scale, 0.125);
  assert.deepEqual(writeGuidedFieldValue({ powergrid_fuse_decay_low_pop: 10 }, low, ""), {});
  assert.equal(issuesFor(schema, low.key, 2147483648).length, 1);
  assert.equal(issuesFor(schema, low.key, 1.5).length, 1);
  assert.equal(issuesFor(schema, scale.key, 1e39).length, 1);
  const gamemodeGroups = rustDefinition.buildFieldGroups("gamemode", schema.fields.filter((field) => field.sectionId === "gamemode"), "en-US", t);
  assert.deepEqual(gamemodeGroups.find((group) => group.id === "heavy-fuse-decay").fields.map((field) => field.key),
    ["powergrid_fuse_decay_low_pop", "powergrid_fuse_decay_high_pop", "powergrid_fuse_decay_low_pop_scale"]);

  const navmesh = schema.fields.find((entry) => entry.key === "use_new_navmesh");
  assert.equal(isOptionalBooleanOverride(navmesh), false);
  assert.equal(navmesh.defaultValue, true);
  assert.equal(readGuidedFieldValue(navmesh, {}), true);
  const renderNav = (settings) => renderToStaticMarkup(React.createElement(ConfigurationField, {
    field: navmesh, value: readGuidedFieldValue(navmesh, settings), settings, onPatch() {},
    copy: { concealSecret: "Hide", revealSecret: "Show", restartScopes: {} }
  }));
  assert.match(renderNav({}), /type="checkbox"[^>]*checked=""/);
  assert.doesNotMatch(renderNav({ use_new_navmesh: false }), /checked=""/);

  const performanceKeys = ["network_position_lod", "network_position_lod_players", "batch_packets", "player_update_jobs"];
  for (const key of performanceKeys) {
    const field = schema.fields.find((entry) => entry.key === key);
    assert.ok(field, key);
    assert.equal(field.defaultValue, undefined, key);
    assert.equal(field.preserveNativeWhenUnset, true, key);
    assert.equal(readGuidedFieldValue(field, {}), undefined, key);
  }
  const batch = schema.fields.find((entry) => entry.key === "batch_packets");
  assert.ok(isOptionalBooleanOverride(batch));
  assert.deepEqual(writeGuidedFieldValue({}, batch, false), { batch_packets: false });
  assert.deepEqual(writeGuidedFieldValue({ batch_packets: false }, batch, undefined), {});
  for (const key of ["network_position_lod", "network_position_lod_players"]) {
    const field = schema.fields.find((entry) => entry.key === key);
    for (const value of [0, 1, 2]) assert.deepEqual(issuesFor(schema, key, value), []);
    for (const value of [-1, 3, 1.5, "1"]) assert.ok(issuesFor(schema, key, value).length, `${key}: ${value}`);
    assert.deepEqual(writeGuidedFieldValue({ [key]: 0 }, field, undefined), {});
  }
  for (const value of [3, 4]) assert.deepEqual(issuesFor(schema, "player_update_jobs", value), []);
  for (const value of [0, 2, 5, 3.5, "4"]) assert.ok(issuesFor(schema, "player_update_jobs", value).length);
  const advancedGroups = rustDefinition.buildFieldGroups("advanced", schema.fields.filter((field) => field.sectionId === "advanced"), "en-US", t);
  assert.deepEqual(advancedGroups.find((group) => group.id === "server-performance").fields.map((field) => field.key), performanceKeys);
});

test("Return to Moria worker threads and RimWorld image URLs enforce bounded validation", () => {
  const moria = parsed("returntomoria");
  const moriaDef = resolveSettingsModuleDefinition("returntomoria");
  const threads = moria.schema.fields.find((entry) => entry.key === "server_worker_threads");
  assert.equal(threads.type, "integer");
  assert.equal(threads.defaultValue, undefined);
  assert.equal(threads.minimum, 1);
  assert.equal(threads.maximum, 4);
  for (const value of [1, 2, 3, 4]) assert.deepEqual(issuesFor(moria.schema, threads.key, value), []);
  for (const value of [0, 5, 2.5]) assert.ok(issuesFor(moria.schema, threads.key, value).length);
  const moriaGroups = moriaDef.buildFieldGroups("advanced", moria.schema.fields.filter((entry) => entry.sectionId === "advanced"), "en-US", moria.t);
  assert.ok(moriaGroups.find((group) => group.id === "performance").fields.some((entry) => entry.key === threads.key));

  const rimworld = parsed("rimworld");
  const imageUrl = rimworld.schema.fields.find((entry) => entry.key === "server_image_url");
  assert.equal(imageUrl.defaultValue, undefined);
  assert.equal(imageUrl.maxLength, 2048);
  for (const value of ["", "http://images.example.test/server.png", "https://images.example.test/殖民地.png?size=400"]) {
    assert.deepEqual(issuesFor(rimworld.schema, imageUrl.key, value), []);
  }
  for (const value of ["file:///image.png", "javascript:alert(1)", "https://", "https://images.example.test/a b.png", "https://images.example.test/\nimage.png", "https://images.example.test/\0image.png", "https://images.example.test/\x9fimage.png"]) {
    assert.ok(issuesFor(rimworld.schema, imageUrl.key, value).length, JSON.stringify(value));
  }
  assert.deepEqual(writeGuidedFieldValue({}, imageUrl, ""), { server_image_url: "" });
});

test("ASA v97 controls are localized, optional and grouped once by behavior", () => {
  const moduleId = "arksurvivalascended";
  const definition = resolveSettingsModuleDefinition(moduleId);
  const descriptions = JSON.parse(fs.readFileSync(path.join(root, "scripts/ark_field_descriptions.json"), "utf8"))[moduleId];
  const expected = {
    num_gachas_per_tribe_for_penalty: ["limits", "gacha", -1],
    gachas_over_tribe_limit_penalty: ["limits", "gacha", 3],
    limit_replicators_per_saddle: ["building", "structure-rules", 0],
    limit_cloning_chambers_range: ["limits", "cloning", 10000],
    limit_cloning_chambers_in_range_num: ["limits", "cloning", 0],
    limit_cloning_chambers_in_tribe_num: ["limits", "cloning", -1]
  };
  for (const locale of ["en-US", "zh-CN"]) {
    const { schema, t, catalog } = parsed(moduleId, locale);
    assert.deepEqual(
      schema.fields.filter((field) => field.sourceId === "asa_server_patch_notes_v97").map((field) => field.key).sort(),
      Object.keys(expected).sort()
    );
    for (const [key, [sectionId, groupId, value]] of Object.entries(expected)) {
      const field = schema.fields.find((candidate) => candidate.key === key);
      assert.equal(field.required, false, key);
      assert.equal(field.defaultValue, undefined, key);
      assert.equal(field.control, "number", key);
      assert.equal(field.sectionId, sectionId, key);
      const groups = definition.buildFieldGroups(sectionId, schema.fields.filter((candidate) => candidate.sectionId === sectionId), locale, t);
      assert.equal(groups.flatMap((group) => group.fields).filter((candidate) => candidate.key === key).length, 1, key);
      assert.ok(groups.find((group) => group.id === groupId).fields.includes(field), key);

      const filled = writeGuidedFieldValue({ server_name: "ASA" }, field, String(value));
      assert.equal(filled[key], value, key);
      assert.deepEqual(writeGuidedFieldValue(filled, field, ""), { server_name: "ASA" }, key);
      assert.equal(catalog[`settings.schema.${moduleId}.${key}.description`], descriptions[key][locale === "en-US" ? "en" : "zh-CN"], key);
    }
  }
});

test("Enshrouded 1.0 Flame level, encounter scaling, custom preset guidance and room password search stay aligned", () => {
  const { schema, t } = parsed("enshrouded");
  const enshroudedDef = resolveSettingsModuleDefinition("enshrouded");
  const flame = schema.fields.find((field) => field.key === "flame_level_fog_removal");
  assert.equal(flame.defaultValue, 11);
  const flameSchema = { ...schema, fields: [flame] };
  for (const [value, accepted] of [[1, true], [11, true], [0, false], [12, false], [20, false], [-1, false], [10.5, false]]) {
    const settings = { flame_level_fog_removal: value };
    assert.equal(validateGuidedSettingsObject(flameSchema, settings).length, accepted ? 0 : 1, String(value));
    assert.equal(settings.flame_level_fog_removal, value);
  }

  const amount = schema.fields.find((field) => field.key === "random_spawner_amount");
  const attacks = schema.fields.find((field) => field.key === "aggro_pool_amount");
  const combatSchema = { ...schema, fields: [amount, attacks] };
  for (const value of ["Few", "Normal", "Many", "Extreme", "Automatic"]) {
    assert.deepEqual(validateGuidedSettingsObject(combatSchema, { random_spawner_amount: value, aggro_pool_amount: "Normal" }), []);
  }
  const combatIssues = validateGuidedSettingsObject(combatSchema, { random_spawner_amount: "Normal", aggro_pool_amount: "Automatic" });
  assert.equal(combatIssues.length, 1);
  assert.equal(combatIssues[0].fieldKey, "aggro_pool_amount");

  assert.equal(schema.presentationFields.find((field) => field.key === "banned_player_ids")?.presentation.owner, "player_access");
  for (const section of schema.sections) {
    const fields = schema.fields.filter((field) => field.sectionId === section.id);
    const groups = enshroudedDef.buildFieldGroups(section.id, fields, "en-US", t);
    const groupedKeys = groups.flatMap((group) => group.fields.map((field) => field.key));
    assert.equal(new Set(groupedKeys).size, groupedKeys.length, section.id);
    assert.deepEqual(groupedKeys.slice().sort(), fields.map((field) => field.key).sort());
  }

  for (const locale of ["en-US", "zh-CN"]) {
    const localized = parsed("enshrouded", locale);
    const requirement = localized.t("enshrouded.settings.customPresetRequirement");
    for (const id of ["players", "survival", "world", "progression", "combat"]) {
      const section = localized.schema.sections.find((entry) => entry.id === id);
      assert.ok(section?.description?.includes(requirement), `${locale}.${id}`);
    }
  }

  const zhModel = buildConfigurationWorkspaceModel(parsed("enshrouded", "zh-CN").schema);
  for (const query of ["房间密码", "服务器密码", "加入密码"]) {
    const matches = searchConfigurationItems(zhModel, query, "zh-CN");
    for (const role of ["admin", "friend", "guest", "visitor"]) {
      assert.ok(matches.some((item) => item.fieldKey === `${role}_password` && item.sectionId === "room"));
    }
  }
});

test("HumanitZ optional native settings preserve unset defaults, support fractions and render localized groups", () => {
  const { schema } = parsed("humanitz");
  const humanitzDef = resolveSettingsModuleDefinition("humanitz");
  for (const [key, section, nativeKey] of [
    ["backpack_cleanup_days", "loot", "BackpackCleanup"],
    ["weapon_durability_multiplier", "world", "WeaponDurabilityMulti"]
  ]) {
    const candidate = schema.fields.find((entry) => entry.key === key);
    assert.equal(candidate.sectionId, section);
    assert.equal(candidate.sourceKey, nativeKey);
    assert.equal(readGuidedFieldValue(candidate, {}), null);
    const cleared = writeGuidedFieldValue({ [key]: 2 }, candidate, "");
    assert.equal(Object.hasOwn(cleared, key), false);
    assert.equal(readGuidedFieldValue(candidate, cleared), null);
  }
  const headshot = schema.fields.find((entry) => entry.key === "headshot_only_mode");
  assert.equal(headshot.sectionId, "difficulty");
  assert.equal(readGuidedFieldValue(headshot, {}), "preserve");
  assert.deepEqual(headshot.enumOptions.map((option) => option.value), ["preserve", "enabled", "disabled"]);

  const durability = schema.fields.find((entry) => entry.key === "weapon_durability_multiplier");
  const backpack = schema.fields.find((entry) => entry.key === "backpack_cleanup_days");
  let settings = writeGuidedFieldValue({ rcon_password: "fixture-password" }, durability, "0.5");
  settings = writeGuidedFieldValue(settings, backpack, "0");
  assert.equal(settings.weapon_durability_multiplier, 0.5);
  assert.equal(settings.backpack_cleanup_days, 0);
  assert.deepEqual(validateGuidedSettingsObject(schema, settings), []);

  for (const [locale, preserveLabel] of [["en-US", "Keep native setting"], ["zh-CN", "保持原生设置"]]) {
    const localized = parsed("humanitz", locale);
    for (const [key, groupId] of [
      ["backpack_cleanup_days", "loot-rules"],
      ["weapon_durability_multiplier", "survival-rules"],
      ["headshot_only_mode", "threat-scaling"]
    ]) {
      const candidate = localized.schema.fields.find((value) => value.key === key);
      const groups = humanitzDef.buildFieldGroups(candidate.sectionId, localized.schema.fields.filter((value) => value.sectionId === candidate.sectionId), locale, localized.t);
      assert.ok(groups.find((group) => group.id === groupId).fields.includes(candidate));
      const markup = renderToStaticMarkup(React.createElement(ConfigurationField, {
        field: candidate, settings: {}, value: readGuidedFieldValue(candidate, {}),
        onPatch() {}, t: localized.t, copy: { restartScopes: { server: "Restart server" } }
      }));
      if (key === "headshot_only_mode") assert.ok(markup.includes(preserveLabel));
    }
  }
});

test("Project Zomboid Build 42 validates scoreboard enums, groups new fields and isolates retired policy review", () => {
  const pzSchema = readSchema("projectzomboid");
  const inventory = require(path.join(root, "modules/projectzomboid/server-options-build-42.json"));
  const retired = require(path.join(root, "modules/projectzomboid/retired-server-options.json"));
  const managed = new Set(["DefaultPort", "UDPPort", "RCONPort", "ResetID", "ServerPlayerID"]);
  const modeled = new Set(Object.values(pzSchema.properties).map((field) => field["x-lsgm-source-key"]));
  assert.equal(inventory.options.length, 144);
  for (const option of inventory.options) assert.ok(modeled.has(option.key) || managed.has(option.key), option.key);
  for (const key of Object.keys(retired)) assert.equal(pzSchema.properties[key], undefined, key);

  const { schema } = parsed("projectzomboid");
  for (const value of ["true", "false"]) {
    assert.deepEqual(validateGuidedSettingsObject(schema, { steam_scoreboard: value }).filter((issue) => issue.fieldKey === "steam_scoreboard"), []);
  }
  const legacy = { steam_scoreboard: "admin", server_description: "Keep this description" };
  const legacyIssues = validateGuidedSettingsObject(schema, legacy).filter((issue) => issue.fieldKey === "steam_scoreboard");
  assert.equal(legacyIssues.length, 1);
  assert.equal(legacyIssues[0].reason, "enum");

  const model = buildConfigurationWorkspaceModel(schema);
  const definition = resolveSettingsModuleDefinition("projectzomboid");
  for (const [key, field] of Object.entries(pzSchema.properties)) {
    if (field["x-lsgm-order"] < 900) continue;
    assert.equal(model.items.find((item) => item.fieldKey === key).owner, "configuration", key);
    const fields = schema.fields.filter((entry) => entry.sectionId === field["x-lsgm-section"]);
    const groups = definition.buildFieldGroups(field["x-lsgm-section"], fields, "en-US", (_key, _params, fallback) => fallback);
    assert.ok(groups.some((group) => group.id !== "additional" && group.fields.some((entry) => entry.key === key)), key);
  }

  const reviewFile = path.resolve(__dirname, "../src/views/settings/ProjectZomboidPolicyReview.tsx");
  function loadReview(catalog) {
    const loaded = new Module(reviewFile, module);
    loaded.filename = reviewFile;
    const fromFile = Module.createRequire(reviewFile);
    loaded.require = (id) => id === "../../i18n" ? { useI18n: () => ({ t: (key, _params, fallback) => catalog[key] ?? fallback }) } : fromFile(id);
    loaded._compile(transpileTypeScript(fs.readFileSync(reviewFile, "utf8"), reviewFile), reviewFile);
    return loaded.exports;
  }
  function descendants(node) {
    return React.isValidElement(node) ? [node, ...React.Children.toArray(node.props.children).flatMap(descendants)] : [];
  }
  const { ProjectZomboidPolicyReview: Review } = loadReview(EN_US_MESSAGES);
  const defaults = Object.fromEntries(Object.entries(retired).map(([key, entry]) => [key, entry.default]));
  for (const settings of [{}, defaults]) assert.equal(Review({ settings, disabled: false, onPatch() {} }), null);
  const reviewHtml = renderToStaticMarkup(Review({ settings: { discord_channel_id: "private-channel-identifier" }, disabled: false, onPatch() {} }));
  assert.ok(reviewHtml.includes("DiscordChannelID"));
  assert.ok(!reviewHtml.includes("private-channel-identifier"));

  for (const catalog of [EN_US_MESSAGES, ZH_CN_MESSAGES]) {
    const { ProjectZomboidPolicyReview: LocalizedReview, PROJECT_ZOMBOID_REVIEW_KEY: key } = loadReview(catalog);
    const patches = [], settings = { anti_cheat_protection_type_1: false, mods: "A" };
    const props = { settings, disabled: false, onPatch(patch) { patches.push(patch); Object.assign(settings, patch); } };
    let input = descendants(LocalizedReview(props)).find((node) => node.type === "input");
    assert.equal(input.props.checked, false);
    input.props.onChange({ currentTarget: { checked: true } });
    assert.deepEqual(patches[0], { [key]: true });
    input = descendants(LocalizedReview(props)).find((node) => node.type === "input");
    assert.equal(input.props.checked, true);
    input.props.onChange({ currentTarget: { checked: false } });
    assert.deepEqual(patches[1], { [key]: false });
    assert.equal(settings.anti_cheat_protection_type_1, false);
    assert.equal(settings.mods, "A");
    props.disabled = true;
    assert.equal(descendants(LocalizedReview(props)).find((node) => node.type === "input").props.disabled, true);
  }
});
