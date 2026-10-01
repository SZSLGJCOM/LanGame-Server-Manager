const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
const {
  expectedPathBehaviors,
  expectedRawBehaviors
} = require("./configuration-presentation-audit-fixtures.cjs");

const repositoryRoot = path.resolve(__dirname, "..", "..", "..");
const desktopRoot = path.join(repositoryRoot, "apps", "desktop");
const modulesRoot = path.join(repositoryRoot, "modules");

function registerTypeScriptRequireExtension(extension) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    const source = fs.readFileSync(filename, "utf8");
    module._compile(transpileTypeScript(source, filename), filename);
  };
}

registerTypeScriptRequireExtension(".ts");
registerTypeScriptRequireExtension(".tsx");
require.extensions[".css"] = function compileEmptyCss(module) {
  module._compile("", module.filename);
};

const {
  resolveConfigurationFieldPresentation,
  resolveConfigurationRendererContract,
  validateConfigurationPresentationDefinition
} = require(path.join(
  desktopRoot,
  "src",
  "views",
  "settings",
  "configuration-presentation.ts"
));
const {
  listSettingsModuleIds,
  resolveSettingsModuleDefinition
} = require(path.join(desktopRoot, "src", "views", "settings", "module-registry.ts"));

const translate = (key, _params, fallback) => fallback ?? key;
const { buildGuidedSections } = require("../src/views/settings/configuration-sections.ts");

function readSchema(moduleId) {
  return JSON.parse(fs.readFileSync(path.join(modulesRoot, moduleId, "schema.json"), "utf8"));
}

function sectionDefinitions(moduleId, definition, properties) {
  const declared = definition.getSections?.(translate, "en-US") ?? definition.sections ?? [];
  const sections = buildGuidedSections(declared, translate)
    .map((section, index) => ({ ...section, order: section.order ?? index }));
  const known = new Set(sections.map((section) => section.id));
  for (const property of Object.values(properties)) {
    const sectionId = property?.["x-lsgm-section"];
    if (typeof sectionId === "string" && sectionId && !known.has(sectionId)) {
      known.add(sectionId);
      sections.push({ id: sectionId, title: sectionId, order: sections.length });
    }
  }
  return sections;
}

test("all 32 bundled schemas have one valid explicit presentation per property", () => {
  const moduleIds = listSettingsModuleIds();
  assert.equal(moduleIds.length, 32);
  assert.deepEqual(
    moduleIds,
    fs.readdirSync(modulesRoot)
      .filter((moduleId) => fs.existsSync(path.join(modulesRoot, moduleId, "schema.json")))
      .sort()
  );

  for (const moduleId of moduleIds) {
    const definition = resolveSettingsModuleDefinition(moduleId);
    assert.ok(definition, `${moduleId} is missing a presentation definition`);
    const properties = readSchema(moduleId).properties ?? {};
    const presentations = validateConfigurationPresentationDefinition({
      definition,
      properties,
      sections: sectionDefinitions(moduleId, definition, properties)
    });
    assert.deepEqual(Object.keys(presentations).sort(), Object.keys(properties).sort());
  }
});

test("default, player-access, and listener ownership use exact contracts", () => {
  const definition = { id: "synthetic" };
  assert.deepEqual(
    resolveConfigurationFieldPresentation("steam_workshop_url", {
      type: "string",
      "x-lsgm-section": "advanced",
      "x-lsgm-source": "server_config"
    }, definition),
    {
      state: "editable",
      owner: "configuration",
      sectionId: "advanced"
    }
  );

  assert.deepEqual(
    resolveConfigurationRendererContract("player-access-roster", {
      id: "cannot-shadow-built-in",
      specializedRenderers: {
        "player-access-roster": { kind: "workspace", workspace: "mods" }
      }
    }),
    { kind: "workspace", workspace: "player_access" }
  );

  assert.deepEqual(
    resolveConfigurationFieldPresentation("blocked_players", {
      type: "string",
      format: "textarea",
      "x-lsgm-section": "access",
      "x-lsgm-player-access-kind": "block"
    }, definition),
    {
      state: "specialized",
      owner: "player_access",
      sectionId: "access",
      rendererId: "player-access-roster"
    }
  );

  assert.deepEqual(
    resolveConfigurationFieldPresentation("bind_ip", {
      type: "string",
      "x-lsgm-section": "network"
    }, definition),
    {
      state: "derived",
      owner: "instance_network",
      sectionId: "network",
      reason: "Listener bind address is managed by the instance network editor."
    }
  );
});

test("sensitive, path, and multiline behavior is exact instead of key-inferred", () => {
  for (const [qualifiedKey, expectedBehavior] of [
    ["abioticfactor.server_password", "secret"],
    ["minecraft.management_server_tls_keystore", "path"],
    ["projectzomboid.server_description", "multiline"]
  ]) {
    const [moduleId, fieldKey] = qualifiedKey.split(".");
    const presentation = resolveConfigurationFieldPresentation(
      fieldKey,
      readSchema(moduleId).properties[fieldKey],
      resolveSettingsModuleDefinition(moduleId)
    );
    assert.equal(presentation.behavior, expectedBehavior, qualifiedKey);
  }

  for (const qualifiedKey of ["dontstarve.caves_monkey"]) {
    const [moduleId, fieldKey] = qualifiedKey.split(".");
    const presentation = resolveConfigurationFieldPresentation(
      fieldKey,
      readSchema(moduleId).properties[fieldKey],
      resolveSettingsModuleDefinition(moduleId)
    );
    assert.equal(presentation.behavior, undefined, `${qualifiedKey} must not be treated as a secret by substring`);
  }
});

test("credential acquisition links cover only fields with official external resources", () => {
  const linked = {};
  for (const moduleId of listSettingsModuleIds()) {
    const definition = resolveSettingsModuleDefinition(moduleId);
    for (const [key, property] of Object.entries(readSchema(moduleId).properties)) {
      const presentation = resolveConfigurationFieldPresentation(key, property, definition);
      if (!presentation.resourceUrl) continue;
      linked[`${moduleId}.${key}`] = presentation.resourceUrl;
      assert.equal(presentation.behavior, "secret");
      assert.equal(presentation.state, "editable");
    }
  }
  assert.deepEqual(linked, {
    "dontstarve.cluster_token": "https://accounts.klei.com/account/game/servers?game=DontStarveTogether",
    "projectzomboid.discord_token": "https://discord.com/developers/applications/select/bot",
    "theforest.steam_account_token": "https://steamcommunity.com/dev/managegameservers",
    "unturned.game_server_login_token": "https://steamcommunity.com/dev/managegameservers"
  });
});

test("raw and path behaviors cover the exact audited native-value fields", () => {
  const observedRaw = [];
  const observedPaths = [];

  for (const moduleId of listSettingsModuleIds()) {
    const definition = resolveSettingsModuleDefinition(moduleId);
    for (const [fieldKey, property] of Object.entries(readSchema(moduleId).properties ?? {})) {
      const behavior = resolveConfigurationFieldPresentation(fieldKey, property, definition).behavior;
      if (behavior === "raw") observedRaw.push(`${moduleId}.${fieldKey}`);
      if (behavior === "path") observedPaths.push(`${moduleId}.${fieldKey}`);
    }
  }

  assert.deepEqual(observedRaw.sort(), [...expectedRawBehaviors].sort());
  assert.deepEqual(observedPaths.sort(), [...expectedPathBehaviors].sort());
});

test("non-scalar fields fail closed or use their real workspace and module-addon renderers", () => {
  const expected = new Map([
    ["arksurvivalascended.additional_maps", ["module-addon", null]],
    ["arksurvivalevolved.additional_maps", ["module-addon", null]],
    ["sevendaystodie.admin_users", ["workspace", "player_access"]],
    ["sevendaystodie.admin_groups", ["workspace", "player_access"]],
    ["sevendaystodie.whitelist_users", ["workspace", "player_access"]],
    ["sevendaystodie.whitelist_groups", ["workspace", "player_access"]],
    ["sevendaystodie.blacklist_entries", ["workspace", "player_access"]],
    ["sevendaystodie.command_permissions", ["module-addon", null]],
    ["dontstarve.master_mod_configuration_options", ["workspace", "mods"]],
    ["dontstarve.caves_mod_configuration_options", ["workspace", "mods"]],
    ["dontstarve.islands_mod_configuration_options", ["workspace", "mods"]],
    ["dontstarve.volcano_mod_configuration_options", ["workspace", "mods"]],
    ["scum.server_general", ["module-addon", null]],
    ["scum.server_world", ["module-addon", null]],
    ["scum.server_features", ["module-addon", null]],
    ["scum.server_respawn", ["module-addon", null]],
    ["scum.server_vehicles", ["module-addon", null]],
    ["scum.server_damage", ["module-addon", null]],
    ["scum.economy_override", ["module-addon", null]],
    ["scum.raid_times", ["module-addon", null]],
    ["scum.notifications", ["module-addon", null]]
  ]);
  const observed = [];

  for (const moduleId of listSettingsModuleIds()) {
    const definition = resolveSettingsModuleDefinition(moduleId);
    for (const [fieldKey, property] of Object.entries(readSchema(moduleId).properties ?? {})) {
      if (property.type !== "array" && property.type !== "object") continue;
      const qualifiedKey = `${moduleId}.${fieldKey}`;
      const presentation = resolveConfigurationFieldPresentation(fieldKey, property, definition);
      assert.notEqual(presentation.state, "editable", qualifiedKey);
      assert.equal(presentation.state, "specialized", qualifiedKey);
      assert.equal(presentation.reason, undefined, qualifiedKey);
      const renderer = resolveConfigurationRendererContract(presentation.rendererId, definition);
      assert.ok(renderer, qualifiedKey);
      const expectedRenderer = expected.get(qualifiedKey);
      assert.ok(expectedRenderer, `${qualifiedKey} lacks an audited renderer expectation`);
      assert.equal(renderer.kind, expectedRenderer[0], qualifiedKey);
      if (renderer.kind === "workspace") assert.equal(renderer.workspace, expectedRenderer[1], qualifiedKey);
      if (renderer.kind === "module-addon") assert.equal(typeof renderer.Renderer, "function", qualifiedKey);
      observed.push(qualifiedKey);
    }
  }

  assert.deepEqual(observed.sort(), [...expected.keys()].sort());
  assert.match(
    fs.readFileSync(path.join(desktopRoot, "src", "views", "settings", "SevenDaysServerAdminPanel.tsx"), "utf8"),
    /settings\.command_permissions[\s\S]*command_permissions: nextEntries/
  );
  assert.match(
    fs.readFileSync(
      path.join(desktopRoot, "src", "views", "servers", "player-center", "player-access-roster-model.ts"),
      "utf8"
    ),
    /x-lsgm-player-access-kind/
  );
});

test("every editable or specialized presentation resolves to a render path", () => {
  const scalarTypes = new Set(["string", "integer", "number", "boolean"]);
  const behaviors = new Set([undefined, "plain", "multiline", "path", "secret", "raw"]);
  const editorVariants = new Set(["workshop-id-list", "enum-check-list", "string-list"]);

  for (const moduleId of listSettingsModuleIds()) {
    const definition = resolveSettingsModuleDefinition(moduleId);
    for (const [fieldKey, property] of Object.entries(readSchema(moduleId).properties ?? {})) {
      const qualifiedKey = `${moduleId}.${fieldKey}`;
      const presentation = resolveConfigurationFieldPresentation(fieldKey, property, definition);
      if (presentation.state === "editable") {
        const types = Array.isArray(property.type) ? property.type.filter((type) => type !== "null") : [property.type];
        assert.equal(types.length, 1, `${qualifiedKey} has ambiguous scalar types`);
        assert.ok(scalarTypes.has(types[0]), `${qualifiedKey} has no scalar form control`);
        assert.ok(behaviors.has(presentation.behavior), `${qualifiedKey} has an unsupported behavior`);
      }
      if (presentation.state === "specialized") {
        const renderer = resolveConfigurationRendererContract(presentation.rendererId, definition);
        assert.ok(renderer, `${qualifiedKey} has no renderer contract`);
        if (renderer.kind === "guided-field") {
          assert.ok(editorVariants.has(renderer.editorVariant), `${qualifiedKey} has no guided editor`);
        } else if (renderer.kind === "module-addon") {
          assert.equal(typeof renderer.Renderer, "function", `${qualifiedKey} has no registered renderer`);
        } else {
          assert.ok(["mods", "player_access"].includes(renderer.workspace), `${qualifiedKey} has no workspace`);
        }
      }
    }
  }
});

test("nullable numeric settings remain editable while structural or ambiguous unions do not", () => {
  for (const type of [["number", "null"], ["null", "integer"]]) {
    assert.equal(resolveConfigurationFieldPresentation("optional_rate", { type, "x-lsgm-section": "rates" }).state, "editable");
  }
  for (const type of [["number", "object"], ["number", "string"], ["null"]]) {
    assert.equal(resolveConfigurationFieldPresentation("ambiguous", { type, "x-lsgm-section": "rates" }).state, "excluded");
  }
});

test("invalid exact overrides and section graphs fail closed", () => {
  const properties = {
    server_name: { type: "string", "x-lsgm-section": "room" }
  };
  const sections = [
    { id: "room", title: "Room", order: 0 },
    { id: "child", title: "Child", order: 1, parentId: "room" }
  ];

  assert.throws(
    () => validateConfigurationPresentationDefinition({
      definition: {
        id: "unknown-field",
        fieldPresentationOverrides: {
          missing: { state: "editable", owner: "configuration", sectionId: "room" }
        }
      },
      properties,
      sections
    }),
    /unknown property missing/
  );
  assert.throws(
    () => validateConfigurationPresentationDefinition({
      definition: {
        id: "unknown-section",
        fieldPresentationOverrides: {
          server_name: { state: "editable", owner: "configuration", sectionId: "missing" }
        }
      },
      properties,
      sections
    }),
    /unknown section missing/
  );
  assert.throws(
    () => validateConfigurationPresentationDefinition({
      definition: {
        id: "missing-renderer",
        fieldPresentationOverrides: {
          server_name: {
            state: "specialized",
            owner: "mods",
            sectionId: "room",
            rendererId: "unknown-renderer"
          }
        }
      },
      properties,
      sections
    }),
    /unregistered renderer unknown-renderer/
  );
  assert.throws(
    () => validateConfigurationPresentationDefinition({
      definition: {
        id: "workspace-owner-mismatch",
        fieldPresentationOverrides: {
          server_name: {
            state: "specialized",
            owner: "configuration",
            sectionId: "room",
            rendererId: "mods-renderer"
          }
        },
        specializedRenderers: {
          "mods-renderer": { kind: "workspace", workspace: "mods" }
        }
      },
      properties,
      sections
    }),
    /renderer workspace must match owner configuration/
  );
  assert.throws(
    () => validateConfigurationPresentationDefinition({
      definition: {
        id: "addon-owner-mismatch",
        fieldPresentationOverrides: {
          server_name: {
            state: "specialized",
            owner: "mods",
            sectionId: "room",
            rendererId: "addon-renderer"
          }
        },
        specializedRenderers: {
          "addon-renderer": {
            kind: "module-addon",
            sectionId: "room",
            fieldKey: "server_name",
            Renderer: () => null
          }
        }
      },
      properties,
      sections
    }),
    /module-addon renderer must be owned by Configuration/
  );
  assert.throws(
    () => validateConfigurationPresentationDefinition({
      definition: {
        id: "non-scalar-editable",
        fieldPresentationOverrides: {
          server_name: { state: "editable", owner: "configuration", sectionId: "room" }
        }
      },
      properties: { server_name: { type: "array" } },
      sections
    }),
    /non-scalar presentation requires a specialized renderer/
  );
  assert.throws(
    () => validateConfigurationPresentationDefinition({
      definition: {
        id: "missing-reason",
        fieldPresentationOverrides: {
          server_name: { state: "excluded", owner: "configuration", sectionId: "room" }
        }
      },
      properties,
      sections
    }),
    /requires a reason/
  );
  assert.throws(
    () => validateConfigurationPresentationDefinition({
      definition: { id: "cycle" },
      properties,
      sections: [
        { id: "room", title: "Room", order: 0, parentId: "child" },
        { id: "child", title: "Child", order: 1, parentId: "room" }
      ]
    }),
    /section cycle/
  );
  assert.throws(
    () => validateConfigurationPresentationDefinition({
      definition: { id: "duplicate-section" },
      properties,
      sections: [
        { id: "room", title: "Room", order: 0 },
        { id: "room", title: "Room again", order: 1 }
      ]
    }),
    /duplicate configuration section room/
  );
});

test("ownership no longer depends on mod-like key or source regexes", () => {
  const guidedSettingsSource = fs.readFileSync(path.join(desktopRoot, "src", "views", "settings", "guided-settings.ts"), "utf8");
  assert.doesNotMatch(guidedSettingsSource, /MOD_SURFACE_TOKEN_RE/);
  assert.doesNotMatch(guidedSettingsSource, /textContainsModSurfaceToken/);
  assert.doesNotMatch(guidedSettingsSource, /isModWorkbenchOwnedField/);
  assert.doesNotMatch(guidedSettingsSource, /password\|token\|secret\|key/);
  assert.doesNotMatch(guidedSettingsSource, /description\|message\|notes\|motd/);
});

test("Soulmask fixed native coefficient is preserved without an editable control", () => {
  const React = require("react");
  const { renderToStaticMarkup } = require("react-dom/server");
  const { ConfigurationField } = require("../src/views/settings/ConfigurationField.tsx");
  const { parseGuidedSettingsSchema, validateGuidedSettingsObject } = require("../src/views/settings/guided-settings.ts");
  const schema = readSchema("soulmask");
  const property = schema.properties.xishu_xi_shu_wei_ling;
  assert.equal(property.default, 0);
  assert.equal(property.minimum, 0);
  assert.equal(property.maximum, 0);
  const definition = resolveSettingsModuleDefinition("soulmask");
  const presentation = resolveConfigurationFieldPresentation(
    "xishu_xi_shu_wei_ling",
    property,
    definition
  );
  assert.equal(presentation.state, "editable");
  assert.equal(presentation.owner, "configuration");
  const guided = parseGuidedSettingsSchema({
    summary: { id: "soulmask", name: "Soulmask" }, schema_json: JSON.stringify(schema)
  }, "en-US", translate);
  const fields = guided.fields.filter((field) => field.key === "xishu_xi_shu_wei_ling");
  assert.equal(fields.length, 1, "The reserved native value retains one configuration owner");
  const field = fields[0];
  assert.equal(field.sectionId, "xishu_general");
  assert.equal(definition.isFieldDisabled(field, {}), true);
  assert.equal(definition.isFieldDisabled({ ...field, key: "xishu_xin_qing_jian_shao" }, {}), false);
  const markup = renderToStaticMarkup(React.createElement(ConfigurationField, {
    field, settings: {}, value: 0, disabled: definition.isFieldDisabled(field, {}), t: translate,
    onPatch: () => assert.fail("The fixed native coefficient must not produce an edit"),
    copy: { concealSecret: "Hide", revealSecret: "Show", restartScopes: {} }
  }));
  assert.equal(field.control, "number");
  assert.match(markup, /<input[^>]*type="text"[^>]*inputMode="decimal"[^>]*disabled=""[^>]*value="0"/);
  assert.deepEqual(validateGuidedSettingsObject(guided, { [field.key]: 0 }).filter((issue) => issue.fieldKey === field.key), []);
  for (const value of [-1, 1, 0.5]) {
    assert.ok(validateGuidedSettingsObject(guided, { [field.key]: value }).some((issue) => issue.fieldKey === field.key));
  }
});
