const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function compileTypeScript(module, filename) {
  const source = fs.readFileSync(filename, "utf8");
  const outputText = transpileTypeScript(source, filename);
  module._compile(outputText, filename);
}
require.extensions[".ts"] = compileTypeScript;
require.extensions[".tsx"] = compileTypeScript;
require.extensions[".css"] = function compileEmptyCss(module) {
  module._compile("module.exports = {};", module.filename);
};

const desktopRoot = path.resolve(__dirname, "..");
const {
  buildConfigurationWorkspaceModel,
  configurationNavigationRoots,
  resolveConfigurationSectionId,
  searchConfigurationItems
} = require(path.join(
  desktopRoot,
  "src",
  "views",
  "settings",
  "configuration-workspace-model.ts"
));
const { parseGuidedSettingsSchema } = require(path.join(
  desktopRoot,
  "src",
  "views",
  "settings",
  "guided-settings.ts"
));
const { listSettingsModuleIds, resolveSettingsModuleDefinition } = require(path.join(
  desktopRoot,
  "src",
  "views",
  "settings",
  "module-registry.ts"
));
const { resolveConfigurationRendererContract } = require(path.join(
  desktopRoot,
  "src",
  "views",
  "settings",
  "configuration-presentation.ts"
));

function section(id, title = id, options = {}) {
  return { id, title, ...options };
}

function field(key, sectionId, options = {}) {
  const state = options.state ?? "editable";
  const presentation = {
    state,
    owner: options.owner ?? "configuration",
    sectionId,
    ...(options.behavior ? { behavior: options.behavior } : {}),
    ...(options.aliases ? { aliases: options.aliases } : {}),
    ...(options.reason ? { reason: options.reason } : {})
  };
  return {
    key,
    title: options.title ?? key,
    description: options.description ?? null,
    type: options.type ?? "string",
    control: options.control ?? "text",
    sectionId,
    sortWeight: options.sortWeight,
    required: false,
    sourceKey: options.sourceKey ?? null,
    presentation
  };
}

function schema(sections, fields = []) {
  return { title: "Test", sections, fields, parseError: null };
}

function flattenNodes(nodes) {
  return nodes.flatMap((node) => [node, ...flattenNodes(node.children)]);
}

test("hides an empty room and selects the first actionable module section", () => {
  const model = buildConfigurationWorkspaceModel(schema([
    section("room", "Room", { order: -300 }),
    section("basics", "Basics", { order: 10 }),
    section("empty", "Empty", { order: 20 })
  ], [field("server_name", "basics", { title: "Server name" })]));

  assert.deepEqual(model.roots.map((node) => node.id), ["basics"]);
  assert.deepEqual(model.actionableSectionIds, ["basics"]);
  assert.equal(resolveConfigurationSectionId(model), "basics");
  assert.equal(model.items[0].fieldKey, "server_name");
});

test("keeps the network editor and hides a runtime section with no native settings", () => {
  const model = buildConfigurationWorkspaceModel(schema([
    section("room", "Room", { order: -300 }),
    section("network", "Network", { order: -200 }),
    section("runtime", "Runtime", { order: -100 })
  ]));

  assert.deepEqual(model.roots.map((node) => node.id), ["network"]);
  assert.deepEqual(model.roots.map((node) => node.builtInEditor), ["instance-network"]);
  assert.deepEqual(model.actionableSectionIds, ["network"]);
});

test("all 32 modules leave autostart out of Configuration while preserving native runtime settings", () => {
  const moduleIds = listSettingsModuleIds();
  assert.equal(moduleIds.length, 32);
  for (const moduleId of moduleIds) {
    const parsed = parseGuidedSettingsSchema({
      summary: { id: moduleId, name: moduleId },
      schema_json: fs.readFileSync(path.join(desktopRoot, "..", "..", "modules", moduleId, "schema.json"), "utf8")
    });
    const model = buildConfigurationWorkspaceModel(parsed);
    const nodes = flattenNodes(model.roots);
    assert.ok(nodes.every((node) => node.builtInEditor !== "instance-runtime"), moduleId);
    const runtimeFields = parsed.fields.filter((candidate) => candidate.sectionId === "runtime");
    for (const candidate of runtimeFields) {
      assert.ok(model.items.some((item) => item.fieldKey === candidate.key), `${moduleId}.${candidate.key}`);
      assert.ok(nodes.some((node) => node.id === "runtime"), moduleId);
    }
    if (!parsed.sections.some((node) => node.parentId === "runtime") && runtimeFields.length === 0) {
      assert.ok(!nodes.some((node) => node.id === "runtime"), moduleId);
    }
  }
});

test("preserves a valid requested selection and falls back deterministically", () => {
  const model = buildConfigurationWorkspaceModel(schema([
    section("first", "First", { order: 10 }),
    section("second", "Second", { order: 20 })
  ], [field("one", "first"), field("two", "second")]));

  assert.equal(resolveConfigurationSectionId(model, "second"), "second");
  assert.equal(resolveConfigurationSectionId(model, "missing"), "first");
  assert.equal(resolveConfigurationSectionId(model, ""), "first");
});

test("retains parent-only nodes when an actionable descendant exists", () => {
  const model = buildConfigurationWorkspaceModel(schema([
    section("worlds", "Worlds", { order: 10 }),
    section("master", "Master", { parentId: "worlds", order: 10 }),
    section("rules", "Rules", { parentId: "master", order: 10 })
  ], [field("day_length", "rules")]));

  assert.equal(model.roots.length, 1);
  assert.equal(model.roots[0].id, "worlds");
  assert.equal(model.roots[0].actionable, false);
  assert.equal(model.roots[0].children[0].id, "master");
  assert.equal(model.roots[0].children[0].children[0].id, "rules");
  assert.deepEqual(model.actionableSectionIds, ["rules"]);
  assert.deepEqual(model.items[0].breadcrumb, ["Worlds", "Master", "Rules"]);
});

test("rejects orphan sections, orphan fields, duplicate ids, and cycles", () => {
  assert.throws(
    () => buildConfigurationWorkspaceModel(schema([
      section("child", "Child", { parentId: "missing" })
    ])),
    /unknown parent missing/
  );
  assert.throws(
    () => buildConfigurationWorkspaceModel(schema([section("known")], [field("lost", "missing")])),
    /field lost references unknown section missing/
  );
  assert.throws(
    () => buildConfigurationWorkspaceModel(schema([section("same"), section("same")])),
    /duplicate configuration section same/
  );
  assert.throws(
    () => buildConfigurationWorkspaceModel(schema([
      section("a", "A", { parentId: "b" }),
      section("b", "B", { parentId: "a" })
    ])),
    /configuration section cycle/
  );
});

test("disambiguates duplicate DST child labels with complete breadcrumbs", () => {
  const model = buildConfigurationWorkspaceModel(schema([
    section("worlds", "Worlds", { order: 10 }),
    section("master", "Master", { parentId: "worlds", order: 10 }),
    section("caves", "Caves", { parentId: "worlds", order: 20 }),
    section("master-generation", "World Generation", { parentId: "master", order: 10 }),
    section("caves-generation", "World Generation", { parentId: "caves", order: 10 })
  ], [
    field("master_preset", "master-generation"),
    field("caves_preset", "caves-generation")
  ]));

  assert.deepEqual(model.items.map((item) => item.breadcrumb), [
    ["Worlds", "Master", "World Generation"],
    ["Worlds", "Caves", "World Generation"]
  ]);
  assert.deepEqual(model.actionableSectionIds, ["master-generation", "caves-generation"]);
});

test("supports ARK-depth navigation without flattening the hierarchy", () => {
  const model = buildConfigurationWorkspaceModel(schema([
    section("ark", "ARK", { order: 10 }),
    section("world", "World", { parentId: "ark", order: 10 }),
    section("creatures", "Creatures", { parentId: "world", order: 10 }),
    section("breeding", "Breeding", { parentId: "creatures", order: 10 }),
    section("imprinting", "Imprinting", { parentId: "breeding", order: 10 })
  ], [field("baby_imprint_amount", "imprinting")]));

  const nodes = flattenNodes(model.roots);
  assert.deepEqual(nodes.map((node) => node.id), [
    "ark",
    "world",
    "creatures",
    "breeding",
    "imprinting"
  ]);
  assert.deepEqual(model.items[0].breadcrumb, [
    "ARK",
    "World",
    "Creatures",
    "Breeding",
    "Imprinting"
  ]);
});

test("returns null when there is content but no actionable field", () => {
  const model = buildConfigurationWorkspaceModel(schema([
    section("identity", "Identity")
  ], [field("save_id", "identity", {
    state: "generated",
    reason: "Generated from the instance identity."
  })]));

  assert.deepEqual(model.roots.map((node) => node.id), ["identity"]);
  assert.deepEqual(model.actionableSectionIds, []);
  assert.equal(resolveConfigurationSectionId(model, "identity"), null);
  assert.equal(model.items.length, 1);
});

test("external specialized owners remain evidence but never become actionable in Configuration", () => {
  const model = buildConfigurationWorkspaceModel(schema([
    section("mods", "Mods")
  ], [field("workshop_ids", "mods", {
    state: "specialized",
    owner: "mods",
    title: "Workshop IDs"
  })]));

  assert.deepEqual(model.actionableSectionIds, []);
  assert.deepEqual(configurationNavigationRoots(model.roots), []);
  assert.deepEqual(model.items.map((item) => ({
    fieldKey: item.fieldKey,
    owner: item.owner,
    state: item.state
  })), [{ fieldKey: "workshop_ids", owner: "mods", state: "specialized" }]);
});

test("Workshop lists have no duplicate Configuration controls or empty Mod categories", () => {
  const fieldsByModule = {
    barotrauma: ["mod_workshop_ids"],
    conanexiles: ["mod_workshop_ids"],
    projectzomboid: ["workshop_items", "mods", "map_name"],
    soulmask: ["mod_workshop_ids"]
  };
  for (const [moduleId, fieldKeys] of Object.entries(fieldsByModule)) {
    const parsed = parseGuidedSettingsSchema({
      summary: { id: moduleId, name: moduleId },
      schema_json: fs.readFileSync(path.join(desktopRoot, "..", "..", "modules", moduleId, "schema.json"), "utf8")
    }, "en-US", (_key, _params, fallback) => fallback ?? "");
    assert.equal(parsed.parseError, null, moduleId);
    const model = buildConfigurationWorkspaceModel(parsed);
    const navigation = flattenNodes(configurationNavigationRoots(model.roots));
    assert.equal(navigation.some((node) => node.id === "mods"), false, moduleId);
    assert.equal(model.actionableSectionIds.includes("mods"), false, moduleId);
    for (const fieldKey of fieldKeys) {
      assert.equal(parsed.fields.some((field) => field.key === fieldKey), false, `${moduleId}.${fieldKey}`);
      assert.equal(model.items.find((item) => item.fieldKey === fieldKey)?.owner, "mods", `${moduleId}.${fieldKey}`);
      assert.equal(searchConfigurationItems(model, fieldKey, "en-US")
        .some((item) => item.owner === "configuration"), false, `${moduleId}.${fieldKey}`);
    }
  }
});

test("real DST parser preserves world controls and advanced Lua while Mod editing has one external owner", () => {
  const moduleDetails = {
    summary: { id: "dontstarve", name: "Don't Starve Together" },
    schema_json: fs.readFileSync(path.join(desktopRoot, "..", "..", "modules", "dontstarve", "schema.json"), "utf8")
  };
  const parsed = parseGuidedSettingsSchema(moduleDetails, "en-US", (_key, _params, fallback) => fallback ?? "");
  const structuredKeys = ["master_mod_configuration_options", "caves_mod_configuration_options"];

  assert.deepEqual(
    structuredKeys.filter((key) => parsed.fields.some((field) => field.key === key)),
    [],
    "non-scalar structured settings must not masquerade as scalar controls"
  );
  assert.deepEqual(
    structuredKeys.filter((key) => parsed.presentationFields?.some((field) => field.key === key)),
    structuredKeys
  );
  const model = buildConfigurationWorkspaceModel(parsed);
  assert.deepEqual(
    model.items.find((item) => item.fieldKey === "master_world_size")?.breadcrumb,
    ["Surface / Master", "World generation"]
  );
  assert.deepEqual(
    model.items.find((item) => item.fieldKey === "master_day")?.breadcrumb,
    ["Surface / Master", "World settings"]
  );
  assert.deepEqual(
    model.items.find((item) => item.fieldKey === "caves_world_size")?.breadcrumb,
    ["Caves", "World generation"]
  );
  assert.deepEqual(
    model.items.find((item) => item.fieldKey === "caves_weather")?.breadcrumb,
    ["Caves", "World settings"]
  );
  for (const key of structuredKeys) {
    const item = model.items.find((candidate) => candidate.fieldKey === key);
    assert.equal(item?.owner, "mods");
    assert.equal(item?.state, "specialized");
    assert.deepEqual(item?.breadcrumb, ["Server mods"]);
    const definition = resolveSettingsModuleDefinition("dontstarve");
    const presentationField = parsed.presentationFields.find((field) => field.key === key);
    const renderer = resolveConfigurationRendererContract(presentationField?.presentation.rendererId, definition);
    assert.deepEqual(renderer, { kind: "workspace", workspace: "mods" });
  }
  const navigation = flattenNodes(configurationNavigationRoots(model.roots));
  assert.equal(navigation.some((node) => node.id === "mods"), false);
  assert.equal(model.actionableSectionIds.includes("mods"), false);
  assert.equal(parsed.fields.some((field) => field.sectionId === "mods"), false);
  for (const key of ["master_modoverrides_lua", "caves_modoverrides_lua"]) {
    const field = parsed.fields.find((candidate) => candidate.key === key);
    assert.equal(field?.sectionId, "advanced");
    assert.equal(field?.presentation.owner, "configuration");
    assert.equal(field?.presentation.behavior, "raw");
  }
  assert.ok(navigation.some((node) => node.id === "advanced"));
});

test("navigation removes external-only branches while preserving mixed groups, generated notes and built-in editors", () => {
  const model = buildConfigurationWorkspaceModel(schema([
    section("network", "Network"),
    section("group", "Group"),
    section("external", "Mods", { parentId: "group" }),
    section("mixed", "Mixed", { parentId: "group" }),
    section("generated", "Generated", { parentId: "group" })
  ], [
    field("external_mods", "external", { owner: "mods", state: "specialized" }),
    field("mixed_mods", "mixed", { owner: "mods", state: "specialized" }),
    field("local_option", "mixed"),
    field("generated_note", "generated", { state: "generated", reason: "Generated locally." })
  ]));
  const original = structuredClone(model.roots);
  const navigation = configurationNavigationRoots(model.roots);
  assert.deepEqual(navigation.map((node) => node.id), ["network", "group"]);
  assert.deepEqual(navigation[1].children.map((node) => node.id), ["mixed", "generated"]);
  assert.deepEqual(model.roots, original);
  assert.equal(searchConfigurationItems(model, "external_mods", "en-US").length, 1,
    "the complete ownership evidence remains available to model consumers");
});

test("all actionable Configuration evidence resolves to a complete renderable field", () => {
  const modulesRoot = path.join(desktopRoot, "..", "..", "modules");
  for (const moduleId of listSettingsModuleIds()) {
    const parsed = parseGuidedSettingsSchema({
      summary: { id: moduleId, name: moduleId },
      schema_json: fs.readFileSync(path.join(modulesRoot, moduleId, "schema.json"), "utf8")
    });
    const fieldsByKey = new Map(parsed.fields.map((candidate) => [candidate.key, candidate]));
    const model = buildConfigurationWorkspaceModel(parsed);
    for (const item of model.items) {
      const isActionable = item.owner === "configuration" &&
        (item.state === "editable" || item.state === "specialized");
      if (!isActionable) continue;
      const completeField = fieldsByKey.get(item.fieldKey);
      if (!completeField) {
        const definition = resolveSettingsModuleDefinition(moduleId);
        const renderer = resolveConfigurationRendererContract(item.field.presentation.rendererId, definition);
        assert.equal(item.state, "specialized", `${moduleId}.${item.fieldKey} lacks a scalar field`);
        assert.equal(renderer?.kind, "module-addon", `${moduleId}.${item.fieldKey} lacks a specialized renderer`);
        assert.equal(typeof renderer?.Renderer, "function", `${moduleId}.${item.fieldKey} lacks a registered renderer`);
        assert.equal(renderer?.sectionId, item.sectionId, `${moduleId}.${item.fieldKey} renderer section mismatch`);
        const backingKey = item.field.presentation.rendererFieldKey ?? item.fieldKey;
        if (renderer.fieldKey) {
          assert.equal(renderer.fieldKey, backingKey, `${moduleId}.${item.fieldKey} renderer ownership mismatch`);
        } else {
          // Section add-ons may edit a subset of an object owned by another renderer.
          const backingField = parsed.presentationFields.find((candidate) => candidate.key === backingKey);
          assert.ok(backingField, `${moduleId}.${item.fieldKey} lacks a backing field`);
          assert.equal(backingField.presentation.state, "specialized", `${moduleId}.${item.fieldKey}`);
          assert.equal(backingField.presentation.owner, "configuration", `${moduleId}.${item.fieldKey}`);
        }
        continue;
      }
      assert.equal(completeField.presentation.state, item.state, `${moduleId}.${item.fieldKey}`);
      assert.equal(completeField.sectionId, item.sectionId, `${moduleId}.${item.fieldKey}`);
    }
  }
});

test("searches all presentation evidence with locale-aware Unicode normalization", () => {
  const model = buildConfigurationWorkspaceModel(schema([
    section("identity", "公开大厅", { order: 10 }),
    section("advanced", "Advanced", { order: 20 })
  ], [
    field("server_name", "identity", {
      title: "Server Name",
      description: "公开\u3000大厅显示名称",
      sourceKey: "SessionName",
      aliases: ["friendly\u00a0room", "大厅名称"]
    }),
    field("world_id", "advanced", {
      title: "World Identity",
      state: "excluded",
      reason: "Managed by generated identity",
      sourceKey: "WorldId"
    })
  ]));

  const cases = [
    ["SERVER NAME", "server_name"],
    ["公开 大厅", "server_name"],
    ["sessionname", "server_name"],
    ["friendly room", "server_name"],
    ["大厅名称", "server_name"],
    ["generated\u3000identity", "world_id"]
  ];
  for (const [query, fieldKey] of cases) {
    assert.deepEqual(
      searchConfigurationItems(model, query, "zh-CN").map((result) => result.fieldKey),
      [fieldKey],
      query
    );
  }

  const [result] = searchConfigurationItems(model, "generated identity", "en-US");
  assert.deepEqual({
    sectionId: result.sectionId,
    fieldKey: result.fieldKey,
    breadcrumb: result.breadcrumb,
    owner: result.owner,
    state: result.state
  }, {
    sectionId: "advanced",
    fieldKey: "world_id",
    breadcrumb: ["Advanced"],
    owner: "configuration",
    state: "excluded"
  });
  assert.deepEqual(searchConfigurationItems(model, "\u00a0\u3000", "en-US"), []);
});

test("all games keep remote-console settings together in Network without duplicate controls", () => {
  const { isRemoteConsoleField } = require(path.join(desktopRoot, "src", "views", "settings", "configuration-field-groups.ts"));
  for (const moduleId of listSettingsModuleIds()) {
    const parsed = parseGuidedSettingsSchema({
      summary: { id: moduleId, name: moduleId },
      schema_json: fs.readFileSync(path.join(desktopRoot, "..", "..", "modules", moduleId, "schema.json"), "utf8")
    }, "en-US", (_key, _params, fallback) => fallback ?? "");
    const remoteFields = parsed.fields.filter((field) => isRemoteConsoleField(field.key));
    if (remoteFields.length === 0) continue;
    for (const field of remoteFields) assert.equal(field.sectionId, "network", `${moduleId}.${field.key}`);
    const networkFields = parsed.fields.filter((field) => field.sectionId === "network");
    const groups = resolveSettingsModuleDefinition(moduleId).buildFieldGroups("network", networkFields, "en-US", (_key, _params, fallback) => fallback ?? "");
    const keys = groups.flatMap((group) => group.fields.map((field) => field.key));
    assert.deepEqual([...keys].sort(), networkFields.map((field) => field.key).sort(), moduleId);
    assert.deepEqual(groups.find((group) => group.id === "remote-console").fields.map((field) => field.key), remoteFields.map((field) => field.key), moduleId);
  }
});
