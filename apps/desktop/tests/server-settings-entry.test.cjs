const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const root = path.resolve(__dirname, "..", "..", "..");
const desktopRoot = path.join(root, "apps", "desktop");

function registerTypeScriptRequireExtension(extension) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    const source = fs.readFileSync(filename, "utf8");
    const outputText = transpileTypeScript(source, filename);
    module._compile(outputText, filename);
  };
}

registerTypeScriptRequireExtension(".ts");
registerTypeScriptRequireExtension(".tsx");
require.extensions[".css"] = function compileEmptyCss(module) {
  module._compile("", module.filename);
};

const { ConfigurationField } = require(path.join(
  desktopRoot, "src", "views", "settings", "ConfigurationField.tsx"
));
const { parseGuidedSettingsSchema, validateGuidedSettingsObject } = require(path.join(
  desktopRoot,
  "src",
  "views",
  "settings",
  "guided-settings.ts"
));
function moduleDetailsFromSchema(moduleId, schemaJson, overrides = {}) {
  const base = {
    summary: {
      id: moduleId,
      name: moduleId,
      version: "0.0.0",
      description: null,
      steam_app_id: null,
      install_state: "NotInstalled",
      supported_platforms: ["windows"]
    },
    default_ports: [],
    runtime: {},
    schema_json: schemaJson
  };

  return {
    ...base,
    ...overrides,
    summary: {
      ...base.summary,
      ...(overrides.summary ?? {})
    },
    default_ports: overrides.default_ports ?? base.default_ports,
    runtime: overrides.runtime ?? base.runtime
  };
}

function moduleDetailsWithSteamMods(moduleId, schemaJson) {
  return moduleDetailsFromSchema(moduleId, schemaJson, {
    workshop: {
      provider: "steam",
      consumer_app_id: 108600,
      supports_collections: true
    },
    mods: {
      source: {
        provider: "steam",
        label: "Steam Workshop",
        url: "https://steamcommunity.com/workshop/"
      },
      manual_staging: null,
      enablement: null
    }
  });
}

function parseModuleGuidedSchema(moduleId) {
  const schemaJson = readSource("modules", moduleId, "schema.json");
  return {
    rawSchema: JSON.parse(schemaJson),
    guidedSchema: parseGuidedSettingsSchema(
      moduleDetailsFromSchema(moduleId, schemaJson),
      "en-US",
      (_key, _params, fallback) => fallback ?? ""
    )
  };
}

function isEditableScalarSchemaProperty(key, property) {
  if (!property || typeof property !== "object") {
    return false;
  }

  const types = Array.isArray(property.type) ? property.type : [property.type];
  return types.some((type) => ["string", "integer", "number", "boolean"].includes(type));
}

test("all bundled schemas preserve explicit sections and expose core tabs", () => {
  const moduleIds = fs.readdirSync(path.join(root, "modules"), { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && fs.existsSync(path.join(root, "modules", entry.name, "module.toml")))
    .map((entry) => entry.name)
    .sort();

  assert.equal(moduleIds.length, 32, "the bundled schema contract covers exactly 32 modules");

  for (const moduleId of moduleIds) {
    const schemaJson = readSource("modules", moduleId, "schema.json");
    const rawSchema = JSON.parse(schemaJson);
    const moduleDetails = moduleDetailsWithSteamMods(moduleId, schemaJson);
    const schemasBySurface = Object.fromEntries(
      ["general", "mods", "player_access", "maintenance"].map((surface) => [surface,
        parseGuidedSettingsSchema(moduleDetails, "en-US", (_key, _params, fallback) => fallback ?? "", { surface })
      ])
    );
    const generalSchema = schemasBySurface.general;
    for (const [surface, guidedSchema] of Object.entries(schemasBySurface)) {
      assert.equal(guidedSchema.parseError, null, `${moduleId} ${surface} must parse`);
      const coreSectionIds = guidedSchema.sections
        .filter((section) => ["room", "network", "access", "runtime"].includes(section.id))
        .map((section) => section.id);

      assert.deepEqual(
        coreSectionIds,
        ["room", "network", "access", "runtime"],
        `${moduleId} ${surface} surface must expose ordered core tabs`
      );
    }

    for (const [key, property] of Object.entries(rawSchema.properties ?? {})) {
      if (!isEditableScalarSchemaProperty(key, property)) {
        continue;
      }

      const ownedFields = Object.entries(schemasBySurface).flatMap(([surface, schema]) =>
        schema.fields.filter((field) => field.key === key).map((field) => ({ surface, field }))
      );

      if (key === "bind_ip") {
        assert.ok(
          ["dontstarve", "rimworld"].includes(moduleId),
          `${moduleId}.${key} must be an explicitly reviewed InstanceSummary-owned field`
        );
        assert.equal(property["x-lsgm-section"], "network", `${moduleId}.${key} must declare the canonical network tab`);
        assert.deepEqual(ownedFields, [], `${moduleId}.${key} must not duplicate the InstanceSummary editor`);
        assert.ok(
          generalSchema.sections.some((section) => section.id === "network" && section.showWhenEmpty),
          `${moduleId}.${key} must retain the canonical network infrastructure tab`
        );
        continue;
      }

      assert.equal(
        ownedFields.length,
        1,
        `${moduleId}.${key} must be owned by exactly one settings surface`
      );
      const { surface, field } = ownedFields[0];
      assert.equal(
        surface,
        { configuration: "general", mods: "mods", player_access: "player_access", maintenance: "maintenance" }[field.presentation.owner],
        `${moduleId}.${key} must be routed to its declared owner`
      );
      assert.equal(field.sectionId, property["x-lsgm-section"], `${moduleId}.${key} must preserve its explicit schema section on ${surface}`);
    }
  }
});

test("explicit sections are never reassigned by field keywords", () => {
  const schemaJson = JSON.stringify({
    title: "Synthetic server",
    type: "object",
    properties: {
      server_name: { type: "string", "x-lsgm-section": "advanced" },
      description: { type: "string", "x-lsgm-section": "access" },
      language: { type: "string", "x-lsgm-section": "room" },
      unsectioned_scalar: { type: "boolean" }
    }
  });
  const guidedSchema = parseGuidedSettingsSchema(
    moduleDetailsFromSchema("section-contract", schemaJson),
    "en-US",
    (_key, _params, fallback) => fallback ?? ""
  );

  assert.deepEqual(
    Object.fromEntries(guidedSchema.fields.map((field) => [field.key, field.sectionId])),
    {
      server_name: "advanced",
      description: "access",
      language: "room",
      unsectioned_scalar: "advanced"
    }
  );
});

test("multi-value schema suggestions remain localized enum check lists", () => {
  for (const moduleId of ["enshrouded", "squad"]) {
    const { rawSchema, guidedSchema } = parseModuleGuidedSchema(moduleId);
    const fieldKey = moduleId === "enshrouded" ? "server_tags" : "admin_permissions";
    const rawField = rawSchema.properties[fieldKey];
    const guidedField = guidedSchema.fields.find((field) => field.key === fieldKey);

    assert.equal(rawField.enum, undefined, `${moduleId}.${fieldKey} must not model a multi-value string as a scalar enum`);
    assert.ok(rawField["x-lsgm-suggestions"].length > 1);
    assert.equal(guidedField.editorVariant, "enum-check-list");
    assert.equal(guidedField.enumOptions, undefined);
    assert.equal(guidedField.suggestions.length, rawField["x-lsgm-suggestions"].length);
  }

  const enshroudedSchemaJson = readSource("modules", "enshrouded", "schema.json");
  const localized = parseGuidedSettingsSchema(
    moduleDetailsFromSchema("enshrouded", enshroudedSchemaJson),
    "en-US",
    (key, _params, fallback) => key === "enshrouded.settings.tags.English" ? "Localized English" : fallback ?? ""
  );
  const serverTags = localized.fields.find((field) => field.key === "server_tags");
  assert.equal(
    serverTags.suggestions.find((option) => option.value === "English").label,
    "Localized English"
  );

  const fieldSource = readSource("apps", "desktop", "src", "views", "settings", "ConfigurationField.tsx");
  assert.match(fieldSource, /props\.field\.enumOptions \?\? props\.field\.suggestions \?\? \[\]/);
  assert.match(fieldSource, /\.join\("\\n"\)/, "check-list values must remain a newline-delimited string");
});

function readSource(...segments) {
  return fs.readFileSync(path.join(root, ...segments), "utf8");
}

function readWorkbenchOperationsCss() {
  const cssPath = path.join(
    root,
    "apps",
    "desktop",
    "src",
    "views",
    "servers",
    "workbench",
    "operations.css"
  );
  const visited = new Set();

  function readCssTree(filePath) {
    const resolvedPath = path.resolve(filePath);
    if (visited.has(resolvedPath)) {
      return "";
    }
    visited.add(resolvedPath);

    const source = fs.readFileSync(resolvedPath, "utf8");
    const imported = [...source.matchAll(/@import\s+"([^"]+)";/g)]
      .map((match) => readCssTree(path.resolve(path.dirname(resolvedPath), match[1])));
    return [source, ...imported].join("\n");
  }

  return readCssTree(cssPath);
}

const settingsModalFiles = ["ConfigurationWorkspace.tsx"];

test("instance settings entry is embedded in the detail tabs", () => {
  const serversView = readSource("apps", "desktop", "src", "views", "ServersView.tsx");
  const serverDetailTabs = readSource("apps", "desktop", "src", "views", "servers", "ServerDetailTabs.tsx");
  const tabSpecs = readSource("apps", "desktop", "src", "views", "servers", "server-detail-tab-specs.ts");

  assert.match(serversView, /import \{ ConfigurationWorkspace \} from "\.\/settings\/ConfigurationWorkspace";/);
  assert.match(serverDetailTabs, /type ServerDetailTab = "runtime" \| "players" \| "settings"/);
  assert.match(serversView, /const detailTabs = buildServerDetailTabSpecs\(/);
  assert.match(tabSpecs, /id: "settings"[\s\S]*?label: t\("servers\.tabs\.settings"/);
  assert.match(serversView, /activeDetailTab === "settings"[\s\S]*?<ConfigurationWorkspace/);
  assert.doesNotMatch(serversView, /SettingsModalRouter|server-settings-inline|onClose=\{\(\) => undefined\}/);
  assert.match(serversView, /onWorkspaceSectionChange\(tab\.id === "settings" \? "settings" : "overview"\)/);
  assert.doesNotMatch(serversView, /ServerJoinAddressCard/);
});

test("instance card no longer exposes a separate settings gear", () => {
  const serversView = readSource("apps", "desktop", "src", "views", "ServersView.tsx");

  assert.doesNotMatch(serversView, /aria-label=\{props\.t\("common\.configure"\)\}/);
});

test("server workspace settings no longer opens a global modal", () => {
  const app = readSource("apps", "desktop", "src", "App.tsx");
  const uiState = readSource("apps", "desktop", "src", "hooks", "useDesktopUiState.ts");

  assert.doesNotMatch(app, /settingsModalOpen/);
  assert.doesNotMatch(app, /<SettingsModalRouter/);
  assert.doesNotMatch(uiState, /setSettingsModalOpen/);
  assert.match(uiState, /openServerWorkspace\(section\)/);
});

test("inline instance settings autosave without save footers", () => {
  for (const fileName of settingsModalFiles) {
    const source = readSource("apps", "desktop", "src", "views", "settings", fileName);

    assert.match(source, /useAutoSaveInstanceSettings\(/, `${fileName} should use the shared autosave hook`);
    assert.doesNotMatch(source, /<footer className="modal-footer"[\s\S]*?settings\.details\.save/, `${fileName} should not render a save footer`);
    assert.doesNotMatch(source, /function submit\(/, `${fileName} should not retain an unreachable parallel save path`);
  }
});

test("instance settings autosave serializes drafts against the last confirmed baseline", () => {
  const hookSource = readSource(
    "apps",
    "desktop",
    "src",
    "views",
    "settings",
    "useAutoSaveInstanceSettings.ts"
  );
  const queueSource = readSource(
    "apps",
    "desktop",
    "src",
    "views",
    "settings",
    "instance-settings-save-queue.ts"
  );

  assert.match(hookSource, /new InstanceSettingsSaveQueue/);
  assert.match(hookSource, /expectedSettingsJson,/);
  assert.match(hookSource, /throwOnError:\s*true/);
  assert.match(queueSource, /const expectedSettingsJson = this\.settingsBaseline/);
  assert.match(queueSource, /this\.settingsBaseline = savedSettingsJson/);
  assert.match(queueSource, /\.finally\(\(\) => \{[\s\S]*?this\.flush\(\)/);
  assert.match(queueSource, /generation === this\.generation && instanceId === this\.instanceId/);
  assert.doesNotMatch(
    queueSource,
    /this\.pending = \{[^}]*expectedSettingsJson/,
    "queued drafts must resolve their expected baseline only when the serialized save begins"
  );
});

test("settings saves never borrow a newer global player-access baseline", () => {
  const source = readSource("apps", "desktop", "src", "hooks", "useDesktopActions.ts");

  assert.match(source, /const expectedSettingsJson = saveOptions\.expectedSettingsJson;/);
  assert.doesNotMatch(source, /expectedSettingsJson[\s\S]{0,120}\?\?\s*options\.instanceDetailsById/);
  assert.match(source, /if \(expectedSettingsJson == null\)/);
});

test("instance settings sidebar mixes basic and game sections without group headings", () => {
  const workspace = readSource("apps", "desktop", "src", "views", "settings", "ConfigurationWorkspace.tsx");
  const navigation = readSource("apps", "desktop", "src", "views", "settings", "ConfigurationSectionNavigation.tsx");

  const searchNavigation = readSource("apps", "desktop", "src", "views", "settings", "ConfigurationSearchNavigation.tsx");
  assert.match(workspace, /<ConfigurationSearchNavigation\s+model=\{model\}/);
  assert.match(searchNavigation, /configurationNavigationRoots\(props\.model\.roots\)/);
  assert.match(searchNavigation, /<ConfigurationSectionNavigation\s+roots=\{navigationRoots\}/);
  assert.match(navigation, /roots\.map/);
  assert.doesNotMatch(navigation, /modal-nav-group|modal-nav-label/);
  assert.doesNotMatch(navigation, /settings\.sections\.groupApp|settings\.sections\.groupModule/);
});

test("core settings panes keep room identity separate from infrastructure", () => {
  const workspace = readSource("apps", "desktop", "src", "views", "settings", "ConfigurationWorkspace.tsx");
  const model = readSource("apps", "desktop", "src", "views", "settings", "configuration-workspace-model.ts");

  assert.match(workspace, /activeNode\.builtInEditor === "instance-network"/);
  assert.doesNotMatch(workspace, /instance-runtime|InstanceRuntimeSettingsPanel/);
  assert.match(model, /network:\s*"instance-network"/);
  assert.doesNotMatch(model, /runtime:\s*"instance-runtime"/);
  assert.doesNotMatch(workspace, /settings\.sections\.basicDescription|Instance base|Instance foundation|Launch, bind, and backup/);

  const englishMessages = readSource("apps", "desktop", "src", "i18n-messages.ts");
  const chineseMessages = readSource("apps", "desktop", "src", "i18n-messages-zh-settings.ts");
  const sevenDaysEnglishMessages = readSource("apps", "desktop", "src", "i18n", "games", "sevendaystodie.en.ts");
  const sevenDaysChineseMessages = readSource("apps", "desktop", "src", "i18n", "games", "sevendaystodie.zh-cn.ts");

  assert.doesNotMatch(englishMessages, /settings\.sections\.basicDescription/);
  assert.doesNotMatch(chineseMessages, /settings\.sections\.basicDescription/);
  assert.doesNotMatch(sevenDaysEnglishMessages, /settings\.7dtd\.modal\.copy\.instanceBase/);
  assert.doesNotMatch(sevenDaysChineseMessages, /settings\.7dtd\.modal\.copy\.instanceBase/);
});

test("guided schema keeps explicit sections authoritative", () => {
  const guidedSettings = readSource("apps", "desktop", "src", "views", "settings", "guided-settings.ts");
  const englishMessages = readSource("apps", "desktop", "src", "i18n-messages.ts");
  const chineseMessages = readSource("apps", "desktop", "src", "i18n-messages-zh-settings.ts");

  assert.doesNotMatch(guidedSettings, /isRoomFieldKey/, "field keywords must not rewrite declared section ownership");
  assert.doesNotMatch(guidedSettings, /isRoomEndpointSource/, "endpoint source names must not rewrite declared section ownership");
  assert.doesNotMatch(guidedSettings, /normalizeGuidedFieldSectionId/, "declared section ids must not be normalized");
  assert.match(guidedSettings, /buildGuidedSections/, "guided schema should use the shared core section builder");
  assert.match(englishMessages, /"settings\.sections\.room": "Room Settings"/);
  assert.match(englishMessages, /"settings\.sections\.network": "Network"/);
  assert.match(englishMessages, /"settings\.sections\.runtime": "Runtime & Advanced"/);
  assert.match(chineseMessages, /"settings\.sections\.room": "房间配置"/);
  assert.match(chineseMessages, /"settings\.sections\.network": "网络"/);
  assert.match(chineseMessages, /"settings\.sections\.runtime": "运行与高级"/);
});

test("ARK launch map selection is exposed in Room Settings", () => {
  for (const moduleId of ["arksurvivalascended", "arksurvivalevolved"]) {
    const schemaJson = readSource("modules", moduleId, "schema.json");
    const schema = JSON.parse(schemaJson);
    const mapField = schema.properties?.map_name;
    const guidedSchema = parseGuidedSettingsSchema(
      moduleDetailsFromSchema(moduleId, schemaJson),
      "en-US",
      (_key, _params, fallback) => fallback ?? ""
    );
    const guidedMapField = guidedSchema.fields.find((field) => field.key === "map_name");

    assert.ok(mapField, `${moduleId} should expose a launch map field`);
    assert.equal(mapField["x-lsgm-source-surface"], "launch_arg", `${moduleId} map should drive launch args`);
    assert.equal(mapField["x-lsgm-source-key"], "server_url.map_name", `${moduleId} map should feed the server URL`);
    assert.equal(mapField["x-lsgm-section"], "room", `${moduleId} map should render in Room Settings`);
    assert.equal(guidedMapField?.sectionId, "room", `${moduleId} parsed map field should render in Room Settings`);
  }
});

test("map entry fields preserve their declared sections", () => {
  for (const [moduleId, key] of [
    ["unturned", "map"],
    ["sevendaystodie", "game_world"],
    ["minecraft", "level_name"]
  ]) {
    const { rawSchema, guidedSchema } = parseModuleGuidedSchema(moduleId);
    const guidedFields = new Map(guidedSchema.fields.map((field) => [field.key, field.sectionId]));
    assert.ok(rawSchema.properties?.[key], `${moduleId}.${key} should exist in schema`);
    assert.equal(
      guidedFields.get(key),
      rawSchema.properties[key]["x-lsgm-section"],
      `${moduleId}.${key} should preserve its schema section`
    );
  }

  for (const [moduleId, key] of [
    ["minecraft", "level_seed"],
    ["sevendaystodie", "world_seed"],
    ["rust", "world_size"]
  ]) {
    const { rawSchema, guidedSchema } = parseModuleGuidedSchema(moduleId);
    const guidedFields = new Map(guidedSchema.fields.map((field) => [field.key, field.sectionId]));
    assert.equal(
      guidedFields.get(key),
      rawSchema.properties[key]["x-lsgm-section"],
      `${moduleId}.${key} should preserve its schema section`
    );
  }
});

test("basic settings toggle rows stay compact without helper copy", () => {
  for (const fileName of settingsModalFiles) {
    const source = readSource("apps", "desktop", "src", "views", "settings", fileName);

    assert.doesNotMatch(source, /settings\.details\.autostartHint/, `${fileName} should not render autostart helper copy`);
    assert.doesNotMatch(source, /Start this instance automatically when the desktop app launches/, `${fileName} should keep autostart as a compact row`);
  }

  const englishMessages = readSource("apps", "desktop", "src", "i18n-messages.ts");
  const chineseMessages = readSource("apps", "desktop", "src", "i18n-messages-zh-settings.ts");

  assert.doesNotMatch(englishMessages, /settings\.details\.autostartHint/);
  assert.doesNotMatch(chineseMessages, /settings\.details\.autostartHint/);
});

test("settings surface uses one compact control design language", () => {
  const workspaceCss = readSource("apps", "desktop", "src", "styles", "configuration-workspace.css");
  const guidedCss = readSource("apps", "desktop", "src", "styles", "settings-guided.css");
  const overlaysCss = readSource("apps", "desktop", "src", "styles", "overlays.css");
  const controlsCss = readSource("apps", "desktop", "src", "styles", "settings-controls.css");
  const selectControlCss = readSource("apps", "desktop", "src", "styles", "select-control.css");
  const connectionCss = readSource("apps", "desktop", "src", "views", "settings", "InstanceConnectionSettingsPanel.css");

  assert.match(workspaceCss, /\.configuration-section-navigation__button\.is-active/, "sidebar tabs should use a consistent active state");
  assert.match(controlsCss, /--settings-control-height:\s*38px/, "fields should use the shared 38px size");
  assert.match(controlsCss, /\.settings-schema-input,[\s\S]*?\.settings-schema-select,[\s\S]*?\.settings-schema-textarea/, "text inputs, selects, and textareas should share a rule");
  assert.match(controlsCss, /\.settings-toggle-card input\[type="checkbox"\]/, "checkboxes should use the shared settings style");
  for (const source of [guidedCss, overlaysCss]) {
    assert.doesNotMatch(source, /--settings-control-height\s*:/, "field sizing must have one owner");
    assert.doesNotMatch(source, /\.settings-schema-input,/, "layout styles must not duplicate the common field rule");
  }
  assert.match(readSource("apps", "desktop", "src", "app.css"), /@import "\.\/styles\/settings-controls\.css";/,
    "the shared field controls must be loaded by the application");
  assert.match(selectControlCss, /select\s*\{[\s\S]*?background-repeat: no-repeat !important;/, "all selects should share a non-repeating global arrow rule");
  assert.doesNotMatch(guidedCss, /select\.settings-schema-input\s*\{/, "guided controls should not own a second select arrow");
  assert.doesNotMatch(overlaysCss, /select\.settings-schema-select/, "overlay controls should not own a second select arrow");
  assert.match(connectionCss, /\.instance-port-fields__input/, "player port input should match compact settings controls");
  assert.match(connectionCss, /minmax\(276px, 420px\)/, "player port group should leave enough width for inline secondary ports");
  assert.match(connectionCss, /width: 96px;/, "inline port items should fit several listeners beside the listen address");
  assert.match(connectionCss, /\.instance-connection-settings__grid/, "network settings should use a deliberate focused layout");
  assert.match(connectionCss, /\.instance-port-fields/, "player ports should stay inside the connection row");
});

test("guided suggestion inputs show every option without native datalist filtering", () => {
  const formSource = readSource("apps", "desktop", "src", "views", "settings", "ConfigurationField.tsx");
  const guidedCss = readSource("apps", "desktop", "src", "styles", "settings-guided.css");

  assert.match(formSource, /settings-suggestion-combobox/, "suggestions should render through the custom combo input");
  assert.match(formSource, /role="listbox"/, "suggestions should expose an explicit option list");
  assert.match(formSource, /suggestions\.map/, "all configured suggestions should be rendered, not browser-filtered");
  assert.doesNotMatch(formSource, /list=\{field\.suggestions/, "native datalist filters by the current input value");
  assert.doesNotMatch(formSource, /<datalist/, "native datalist should not be used for map suggestions");
  assert.match(guidedCss, /\.settings-suggestion-menu/, "custom suggestion menu should have stable panel styling");
});

test("guided string fields propagate schema length constraints into native inputs", () => {
  const schemaJson = JSON.stringify({
    title: "Synthetic server",
    type: "object",
    properties: {
      owner_id: {
        type: "string",
        title: "Owner ID",
        minLength: 1,
        maxLength: 32,
        pattern: ".*\\S.*"
      },
      server_name: {
        type: "string",
        title: "Server Name",
        maxLength: 15
      },
      notes: {
        type: "string",
        format: "textarea",
        title: "Notes",
        minLength: 2,
        maxLength: 120
      }
    }
  });
  const guidedSchema = parseGuidedSettingsSchema(
    moduleDetailsFromSchema("synthetic", schemaJson),
    "en-US",
    (_key, _params, fallback) => fallback ?? ""
  );
  const fields = new Map(guidedSchema.fields.map((field) => [field.key, field]));
  assert.equal(fields.get("owner_id")?.minLength, 1);
  assert.equal(fields.get("owner_id")?.maxLength, 32);
  assert.equal(fields.get("owner_id")?.pattern, ".*\\S.*");
  assert.equal(fields.get("server_name")?.minLength, undefined);
  assert.equal(fields.get("server_name")?.maxLength, 15);
  assert.equal(fields.get("notes")?.minLength, 2);
  assert.equal(fields.get("notes")?.maxLength, 120);
  for (const field of fields.values()) {
    const variants = field.control === "textarea" ? ["multiline"] : ["plain", "secret", "suggestions"];
    for (const variant of variants) {
      const html = renderToStaticMarkup(React.createElement(ConfigurationField, {
        field: {
          ...field,
          presentation: { ...field.presentation, behavior: variant === "suggestions" ? "plain" : variant },
          suggestions: variant === "suggestions" ? [{ label: "Suggested value", value: "Server" }] : undefined
        },
        copy: { concealSecret: "Hide", revealSecret: "Show", restartScopes: {} },
        settings: {}, value: "", onPatch() {}
      }));
      const context = `${field.key}:${variant}`;
      assert.match(html, field.control === "textarea" ? /<textarea\b/ : /<input\b/, context);
      for (const attribute of ["minLength", "maxLength"]) {
        if (field[attribute] === undefined) {
          assert.doesNotMatch(html, new RegExp(`\\b${attribute}=`, "i"), context);
        } else {
          assert.match(html, new RegExp(`\\b${attribute}="${field[attribute]}"`, "i"), context);
        }
      }
      if (field.pattern && field.control !== "textarea") {
        assert.ok(html.includes(`pattern="${field.pattern}"`), context);
      } else {
        assert.doesNotMatch(html, /\bpattern=/, context);
      }
      if (variant === "secret") assert.match(html, /type="password"/, context);
      if (variant === "suggestions") assert.match(html, /role="combobox"/, context);
    }
  }
});

test("guided settings validation blocks schema string constraints before autosave", () => {
  const { guidedSchema } = parseModuleGuidedSchema("runescapedragonwilds");
  const workspaceSource = readSource("apps", "desktop", "src", "views", "settings", "ConfigurationWorkspace.tsx");

  const issues = validateGuidedSettingsObject(guidedSchema, {
    owner_id: "   ",
    server_name: "Dragonwilds",
    default_world_name: "ThisWorldNameIsTooLong"
  });

  assert.ok(
    issues.some((issue) => issue.fieldKey === "owner_id" && /pattern/i.test(issue.reason)),
    `expected owner_id pattern issue, got ${JSON.stringify(issues)}`
  );
  assert.ok(
    issues.some((issue) => issue.fieldKey === "default_world_name" && /maxLength/i.test(issue.reason)),
    `expected default_world_name maxLength issue, got ${JSON.stringify(issues)}`
  );
  assert.match(workspaceSource, /validateGuidedSettingsObject\(schema, settingsParseResult\.value/);
  assert.match(workspaceSource, /validationIssues\.length > 0/);
  assert.match(workspaceSource, /disabled:\s*saveBlocked/);

  const formSource = readSource("apps", "desktop", "src", "views", "settings", "GuidedSettingsForm.tsx");
  assert.match(formSource, /validationIssues\?: GuidedSettingsValidationIssue\[\]/);
  assert.match(formSource, /schemaValidationMessage/);
  assert.match(workspaceSource, /validationIssues=\{validationIssues\}/);
});

test("raw configuration fields cannot override explicitly managed native directives", () => {
  const schemaJson = JSON.stringify({
    type: "object",
    properties: {
      extra: {
        type: "string",
        format: "textarea",
        title: "Extra INI lines",
        "x-lsgm-disallowed-line-prefixes": ["MaxPlayers=", "[/Script/Engine.GameSession]"]
      }
    }
  });
  const schema = parseGuidedSettingsSchema(
    moduleDetailsFromSchema("synthetic", schemaJson),
    "en-US",
    (_key, _params, fallback) => fallback ?? ""
  );

  assert.deepEqual(schema.fields[0].disallowedLinePrefixes, [
    "MaxPlayers=",
    "[/Script/Engine.GameSession]"
  ]);
  assert.equal(
    validateGuidedSettingsObject(schema, { extra: "; MaxPlayers=comment only\nUnknown=True" }).length,
    0
  );
  assert.ok(
    validateGuidedSettingsObject(schema, { extra: "  MAXPLAYERS=64" })
      .some((issue) => issue.fieldKey === "extra" && issue.reason === "managedDirective")
  );
});

test("guided schema validation gates autosave across settings modals", () => {
  for (const fileName of settingsModalFiles) {
    const source = readSource("apps", "desktop", "src", "views", "settings", fileName);
    if (!source.includes("<GuidedSettingsForm") || !source.includes("useAutoSaveInstanceSettings")) {
      continue;
    }

    assert.match(
      source,
      /validateGuidedSettingsObject/,
      `${fileName} should validate guided schema constraints before autosave`
    );
    assert.match(
      source,
      /schemaIssues/,
      `${fileName} should retain guided schema validation issues for field-level rendering`
    );
    assert.match(
      source,
      /saveBlocked/,
      `${fileName} should share one saveBlocked gate for schema and modal-specific validation`
    );
    assert.match(
      source,
      /disabled:\s*saveBlocked/,
      `${fileName} autosave should be disabled while guided schema validation fails`
    );
    assert.match(
      source,
      /validationIssues=\{validationIssues\}/,
      `${fileName} should pass guided schema issues into GuidedSettingsForm`
    );
    if (source.includes("function submit(")) {
      assert.match(
        source,
        /if \(saveBlocked\)/,
        `${fileName} manual save should use the same saveBlocked gate`
      );
    }
  }
});

test("join path wizard is not embedded in basic instance configuration", () => {
  for (const fileName of settingsModalFiles) {
    const source = readSource("apps", "desktop", "src", "views", "settings", fileName);

    assert.doesNotMatch(source, /JoinPathPresets/, `${fileName} should keep basic configuration focused on direct fields`);
    assert.doesNotMatch(source, /join-path-presets/, `${fileName} should not render the join path wizard`);
  }

  assert.throws(
    () => readSource("apps", "desktop", "src", "views", "settings", "JoinPathPresets.tsx"),
    /ENOENT/,
    "JoinPathPresets should be removed rather than hidden"
  );
  assert.throws(
    () => readSource("apps", "desktop", "src", "views", "settings", "JoinPathPresets.css"),
    /ENOENT/,
    "JoinPathPresets styles should be removed with the component"
  );
});

test("instance settings keep backup policy out of configuration fields", () => {
  for (const fileName of settingsModalFiles) {
    const source = readSource("apps", "desktop", "src", "views", "settings", fileName);

    assert.doesNotMatch(source, /setBackupRetentionCount/, `${fileName} should not expose backup retention editing`);
    assert.doesNotMatch(source, /setAutoBackupOnStop/, `${fileName} should not expose stop-backup editing`);
    assert.doesNotMatch(source, /value=\{backupRetentionCount\}/, `${fileName} should not render backup retention inputs`);
    assert.doesNotMatch(source, /checked=\{autoBackupOnStop\}/, `${fileName} should not render stop-backup toggles`);
  }
});

test("Network settings own the persisted join address copy affordance", () => {
  const listenAddressSelect = readSource("apps", "desktop", "src", "views", "settings", "ListenAddressSelect.tsx");
  const joinAddressSelect = readSource("apps", "desktop", "src", "views", "settings", "PlayerJoinAddressSelect.tsx");
  const networkPanel = readSource("apps", "desktop", "src", "views", "settings", "InstanceConnectionSettingsPanel.tsx");

  assert.doesNotMatch(listenAddressSelect, /buildJoinEndpoint|navigator\.clipboard/);
  assert.match(joinAddressSelect, /buildShareEndpoints/);
  assert.match(joinAddressSelect, /navigator\.clipboard\.writeText\(selectedEndpoint\.endpoint\)/);
  assert.match(joinAddressSelect, /<select[\s\S]{0,300}?endpoints\.map/, "Detected LAN, overlay, and direct public endpoints should be selectable");
  assert.match(networkPanel, /<PlayerJoinAddressSelect[\s\S]{0,200}?details=\{props\.details\}/, "Network should own a separate player join-address selector");
  assert.doesNotMatch(networkPanel, /bindIpUnsupported|bindIpStrictHint/, "Network should provide controls instead of bind capability notices");

  for (const fileName of settingsModalFiles) {
    const source = readSource("apps", "desktop", "src", "views", "settings", fileName);
    assert.match(source, /<InstanceConnectionSettingsPanel/, `${fileName} should render the shared Network panel`);
    assert.doesNotMatch(source, /InstanceRuntimeSettingsPanel/, `${fileName} must leave autostart in Maintenance`);
    assert.doesNotMatch(source, /RoomSettingsPanel/, `${fileName} should not retain the mixed Room infrastructure panel`);
  }
});

test("Network settings expose editable player and management-service ports", () => {
  const portFields = readSource("apps", "desktop", "src", "views", "settings", "InstancePortFields.tsx");
  const networkPanel = readSource("apps", "desktop", "src", "views", "settings", "InstanceConnectionSettingsPanel.tsx");

  assert.match(networkPanel, /instance-connection-settings__listener-row/, "Listen address and player ports should share one network row when supported");
  assert.match(networkPanel, /instance-connection-settings__ports/, "Player ports should remain visible when strict binding is unavailable");
  assert.match(portFields, /instance-port-fields__input/, "Host Port should be editable as a number input");
  assert.match(portFields, /onPortChange/, "Host Port changes should flow through the settings save path");
  assert.match(networkPanel, /partitionInstancePorts/, "module port roles should drive player and service presentation");
  assert.match(networkPanel, /settings\.network\.servicePorts/, "management listeners should have a separate group");
  assert.match(portFields, /instancePortBindingKey/, "Each protocol-specific listener should keep its own editable draft");
  assert.doesNotMatch(networkPanel, /bind-address-quick-actions/, "Listen address should not render quick-pick button rows");
  assert.doesNotMatch(networkPanel, /bind-address-port-action/, "Host Port should not be hidden behind a register button");
  assert.match(networkPanel, /resolveVisibleInstancePorts\(props\.ports, props\.defaultPorts\)/);
  assert.match(networkPanel, /materializeInstancePortEdit/);

  for (const fileName of settingsModalFiles) {
    const source = readSource("apps", "desktop", "src", "views", "settings", fileName);
    assert.match(source, /defaultPorts=\{readOnly \? \[\] : defaultPorts\}/,
      `${fileName} should retain normal module defaults without projecting them into archives`);
    assert.match(source, /onPortsChange=\{\(value\) => \{ if \(!readOnly\) setPorts\(value\); \}\}/,
      `${fileName} should wire ordinary host port edits and hard reject archived draft changes`);
  }
});

test("Room settings no longer own bind, port, or autostart editors", () => {
  for (const fileName of settingsModalFiles) {
    const source = readSource("apps", "desktop", "src", "views", "settings", fileName);

    assert.doesNotMatch(source, /RoomSettingsPanel/, `${fileName} should not use the retired mixed panel`);
    assert.doesNotMatch(source, /BindAddressSelect/, `${fileName} should not own a bind editor`);
    assert.doesNotMatch(source, /editablePorts\.map\(\(port\)/, `${fileName} should not render a modal-local host port grid`);
    assert.match(source, /<InstanceConnectionSettingsPanel/);
    assert.doesNotMatch(source, /InstanceRuntimeSettingsPanel/);
  }
});

test("room and network render in Configuration while autostart belongs to Maintenance", () => {
  for (const fileName of settingsModalFiles) {
    const source = readSource("apps", "desktop", "src", "views", "settings", fileName);

    assert.match(source, /<GuidedSettingsForm[\s\S]{0,300}selectedSectionId=\{selectedSectionId\}/, `${fileName} should render schema-owned room fields`);
    assert.match(source, /activeNode\.builtInEditor === "instance-network"[\s\S]{0,200}<InstanceConnectionSettingsPanel/);
    assert.doesNotMatch(source, /instance-runtime|InstanceRuntimeSettingsPanel/);
  }
});

test("ARK and DST use the shared network category without duplicate service groups", () => {
  const arkAsa = readSource("apps", "desktop", "src", "views", "settings", "modules", "ark-asa.ts");
  const arkAse = readSource("apps", "desktop", "src", "views", "settings", "modules", "ark-ase.ts");
  const arkAsaFoundation = readSource(
    "apps", "desktop", "src", "views", "settings", "modules", "ark-asa-groups-foundation.ts"
  );
  const arkAseFoundation = readSource(
    "apps", "desktop", "src", "views", "settings", "modules", "ark-ase-groups-foundation.ts"
  );
  const dst = readSource("apps", "desktop", "src", "views", "settings", "modules", "dontstarve.ts");

  assert.doesNotMatch(arkAsa, /Network & Ports/);
  assert.doesNotMatch(arkAse, /Network & Ports/);
  assert.doesNotMatch(arkAsa, /id: "network"/);
  assert.doesNotMatch(arkAse, /id: "network"/);
  assert.doesNotMatch(arkAsa, /resolveFieldSectionId/);
  assert.doesNotMatch(arkAse, /resolveFieldSectionId/);
  for (const moduleId of ["arksurvivalascended", "arksurvivalevolved"]) {
    const properties = JSON.parse(readSource("modules", moduleId, "schema.json")).properties;
    for (const [key, property] of Object.entries(properties)) {
      if (key.startsWith("rcon_") || key === "enable_rcon") {
        assert.equal(property["x-lsgm-section"], "network", `${moduleId}.${key}`);
      }
    }
  }
  assert.doesNotMatch(arkAsaFoundation, /ark-operations-services/);
  assert.doesNotMatch(arkAseFoundation, /ark-operations-services/);
  assert.doesNotMatch(dst, /id: "network"[\s\S]{0,180}dst\.settings\.sections\.network/);
});

test("instance settings tabs open directly into configuration fields", () => {
  for (const fileName of settingsModalFiles) {
    const source = readSource("apps", "desktop", "src", "views", "settings", fileName);

    assert.doesNotMatch(source, /className="modal-header"/, `${fileName} should not render a redundant settings page header`);
    assert.doesNotMatch(source, /modal-header-copy/, `${fileName} should not repeat instance and module names above settings fields`);
    assert.doesNotMatch(source, /className="modal-body-head"/, `${fileName} should not repeat the active tab title above settings fields`);
    assert.doesNotMatch(source, /command-strip/, `${fileName} should not insert a summary strip before settings fields`);
    assert.doesNotMatch(source, /settings-workstrip|summary-chip/, `${fileName} should not insert summary chips before settings fields`);
    assert.doesNotMatch(source, /materializationPreview/, `${fileName} should not render materialization preview guide cards before settings fields`);
    assert.doesNotMatch(source, /buildSectionGuide\(/, `${fileName} should not build section guide cards before settings fields`);
    assert.doesNotMatch(source, /className="[^"]*(?:guide|preview|summary)-(?:fact|target|card|grid|section)[^"]*"/, `${fileName} should not render guide fact cards before settings fields`);
    assert.doesNotMatch(source, /\.facts\.map\(/, `${fileName} should not map guide facts into four-card field summaries`);
    assert.doesNotMatch(source, /settings-section-count/, `${fileName} should not show guided field count chips`);
    assert.doesNotMatch(source, /settings\.guided\.fieldCount/, `${fileName} should not render guided field count copy`);
  }

  const workspaceSource = readSource("apps", "desktop", "src", "views", "settings", "ConfigurationWorkspace.tsx");
  assert.doesNotMatch(workspaceSource, /sevendays-instance-name|sevendays-instance-module/, "Seven Days settings should not repeat instance and module names in the tab body");
});

test("schema-driven instance settings do not expose a config sources tab", () => {
  const source = readSource("apps", "desktop", "src", "views", "settings", "ConfigurationWorkspace.tsx");
  const englishMessages = readSource("apps", "desktop", "src", "i18n-messages.ts");
  const chineseMessages = readSource("apps", "desktop", "src", "i18n-messages-zh-settings.ts");

  assert.doesNotMatch(source, /GameConfigSourcePanel/);
  assert.doesNotMatch(source, /settings\.sections\.sources/);
  assert.doesNotMatch(source, /activeSectionId === "sources"/);
  assert.doesNotMatch(englishMessages, /settings\.sources\./);
  assert.doesNotMatch(chineseMessages, /settings\.sources\./);
  assert.doesNotMatch(englishMessages, /settings\.sections\.sources/);
  assert.doesNotMatch(chineseMessages, /settings\.sections\.sources/);
});

test("mod-like field names and sources remain in Configuration by default", () => {
  const schemaJson = JSON.stringify({
    title: "Synthetic server",
    type: "object",
    properties: {
      server_name: {
        type: "string",
        title: "Server name"
      },
      workshop_items: {
        type: "string",
        title: "WorkshopItems",
        "x-lsgm-section": "mods"
      },
      map_name: {
        type: "string",
        title: "Map load order",
        "x-lsgm-section": "mods"
      },
      master_modoverrides_lua: {
        type: "string",
        title: "Master modoverrides.lua",
        "x-lsgm-source": "master_modoverrides",
        "x-lsgm-source-key": "modoverrides.lua:raw_lua"
      }
    }
  });
  const guidedSchema = parseGuidedSettingsSchema(
    moduleDetailsWithSteamMods("synthetic", schemaJson),
    "en-US",
    (_key, _params, fallback) => fallback ?? ""
  );

  assert.ok(guidedSchema.fields.some((field) => field.key === "server_name"));
  assert.ok(guidedSchema.fields.some((field) => field.key === "workshop_items"));
  assert.ok(guidedSchema.fields.some((field) => field.key === "map_name"));
  assert.ok(guidedSchema.fields.some((field) => field.key === "master_modoverrides_lua"));
  assert.ok(guidedSchema.sections.some((section) => section.id === "mods"));
});

test("mods workbench ownership is never inferred from field names or sources", () => {
  const schemaJson = JSON.stringify({
    title: "Synthetic server",
    type: "object",
    properties: {
      server_name: {
        type: "string",
        title: "Server name"
      },
      workshop_items: {
        type: "string",
        title: "WorkshopItems",
        "x-lsgm-section": "mods"
      },
      master_modoverrides_lua: {
        type: "string",
        title: "Master modoverrides.lua",
        "x-lsgm-source": "master_modoverrides",
        "x-lsgm-source-key": "modoverrides.lua:raw_lua"
      }
    }
  });
  const guidedSchema = parseGuidedSettingsSchema(
    moduleDetailsWithSteamMods("dontstarve", schemaJson),
    "en-US",
    (_key, _params, fallback) => fallback ?? "",
    { surface: "mods" }
  );

  assert.equal(guidedSchema.fields.some((field) => field.key === "server_name"), false);
  assert.equal(guidedSchema.fields.some((field) => field.key === "workshop_items"), false);
  assert.equal(guidedSchema.fields.some((field) => field.key === "master_modoverrides_lua"), false);
  assert.deepEqual(
    guidedSchema.sections
      .filter((section) => ["room", "network", "access", "runtime"].includes(section.id))
      .map((section) => section.id),
    ["room", "network", "access", "runtime"]
  );
  assert.ok(guidedSchema.sections.every((section) => (
    ["room", "network", "access", "runtime"].includes(section.id) ||
    guidedSchema.fields.some((field) => field.sectionId === section.id)
  )));
});

test("known mod-capable games keep non-operational mod parameters in Configuration", () => {
  const exactModsOwnedFields = new Map([
    ["dontstarve", new Set([
      "shared_workshop_mod_ids", "shared_workshop_collection_ids",
      "master_enabled_workshop_mod_ids", "caves_enabled_workshop_mod_ids",
      "islands_enabled_workshop_mod_ids", "volcano_enabled_workshop_mod_ids",
      "master_mod_configuration_options", "caves_mod_configuration_options",
      "islands_mod_configuration_options", "volcano_mod_configuration_options"
    ])],
    ["projectzomboid", new Set(["workshop_items", "mods", "map_name"])],
    ["terraria", new Set(["tmodloader_workshop_item_ids"])]
  ]);
  for (const moduleId of ["dontstarve", "projectzomboid", "terraria"]) {
    const schemaJson = readSource("modules", moduleId, "schema.json");
    const properties = JSON.parse(schemaJson).properties ?? {};
    const guidedSchema = parseGuidedSettingsSchema(
      moduleDetailsWithSteamMods(moduleId, schemaJson),
      "en-US",
      (_key, _params, fallback) => fallback ?? ""
    );
    const delegated = exactModsOwnedFields.get(moduleId) ?? new Set();
    const expectedFieldKeys = Object.entries(properties)
      .filter(([fieldKey, property]) => property["x-lsgm-section"] === "mods" && !delegated.has(fieldKey))
      .map(([fieldKey]) => fieldKey)
      .sort();
    const actualFieldKeys = (guidedSchema.presentationFields ?? [])
      .filter((field) => (
        field.sectionId === "mods" &&
        field.presentation.owner === "configuration" &&
        ["editable", "specialized"].includes(field.presentation.state)
      ))
      .map((field) => field.key)
      .sort();

    assert.deepEqual(actualFieldKeys, expectedFieldKeys, `${moduleId} must delegate only exact operational fields`);
    if (moduleId === "dontstarve") {
      assert.deepEqual(expectedFieldKeys, [], "all ordinary DST Mod controls belong to the external workspace");
      for (const key of ["master_modoverrides_lua", "caves_modoverrides_lua", "islands_modoverrides_lua", "volcano_modoverrides_lua"]) {
        assert.ok(guidedSchema.fields.some((field) => field.key === key && field.sectionId === "advanced"),
          `${key} must remain available as an advanced native override`);
      }
      continue;
    }
    if (moduleId === "projectzomboid") {
      assert.deepEqual(expectedFieldKeys, [], "PZ Mod lists belong to the external workspace");
      assert.equal(guidedSchema.fields.some((field) => field.key === "map_name"), false,
        "native map order is edited only in the Mods workspace");
      continue;
    }
    assert.ok(expectedFieldKeys.length > 0, `${moduleId} should retain game-native mod parameters`);
    assert.ok(
      guidedSchema.sections.some((section) => section.id === "mods" || section.variant === "dst-workshop-hub"),
      `${moduleId} should retain its game-native Mods section inside Configuration`
    );
  }
});

test("player-access rosters have one owning surface", () => {
  const moduleDetails = moduleDetailsFromSchema("roster-owner", JSON.stringify({
    type: "object",
    properties: {
      server_name: { type: "string", title: "Server name" },
      blocked_players: {
        type: "string",
        format: "textarea",
        title: "Blocked players",
        "x-lsgm-player-access-kind": "block"
      }
    }
  }));

  const configuration = parseGuidedSettingsSchema(
    moduleDetails,
    "en-US",
    (_key, _params, fallback) => fallback ?? ""
  );
  const playerAccess = parseGuidedSettingsSchema(
    moduleDetails,
    "en-US",
    (_key, _params, fallback) => fallback ?? "",
    { surface: "player_access" }
  );

  assert.deepEqual(configuration.fields.map((field) => field.key), ["server_name"]);
  assert.deepEqual(playerAccess.fields.map((field) => field.key), ["blocked_players"]);
});

test("configuration settings do not mount dedicated mod import or status surfaces", () => {
  const guidedForm = readSource("apps", "desktop", "src", "views", "settings", "GuidedSettingsForm.tsx");
  const dstDefinition = readSource("apps", "desktop", "src", "views", "settings", "modules", "dontstarve.ts");
  const modWorkbench = ["ModWorkbench.tsx", "mod-workbench-model.ts", "mod-workbench-plans.ts"]
    .map((fileName) => readSource("apps", "desktop", "src", "views", "servers", fileName))
    .join("\n");

  assert.doesNotMatch(guidedForm, /WorkshopImportStudio/, "DST workshop import should live in the Mods workbench, not configuration");
  assert.doesNotMatch(guidedForm, /dst-workshop-hub/, "configuration sections should not render a special mod hub");
  assert.doesNotMatch(dstDefinition, /DstModStatusPanel/, "DST mod status should not be appended to the configuration page");
  assert.doesNotMatch(dstDefinition, /AddonPanel:\s*DstModStatusPanel/);
  assert.match(modWorkbench, /shared_workshop_mod_ids/);
  assert.match(modWorkbench, /workshop_items/);
  assert.match(modWorkbench, /tmodloader_workshop_item_ids/);
});

test("game settings do not append ops, invite, or preset addon surfaces", () => {
  for (const [moduleFile, panelFile, panelName] of [
    ["abioticfactor.ts", "AbioticFactorOpsPanel.tsx", "AbioticFactorOpsPanel"],
    ["palworld.ts", "PalworldOpsPanel.tsx", "PalworldOpsPanel"],
    ["projectzomboid.ts", "ProjectZomboidOpsPanel.tsx", "ProjectZomboidOpsPanel"],
    ["valheim.ts", "ValheimOpsPanel.tsx", "ValheimOpsPanel"]
  ]) {
    const source = readSource("apps", "desktop", "src", "views", "settings", "modules", moduleFile);
    assert.doesNotMatch(source, new RegExp(panelName), `${moduleFile} should not import an ops/invite addon panel`);
    assert.doesNotMatch(source, /AddonPanel:/, `${moduleFile} should not append an ops/invite addon panel`);
    assert.throws(
      () => readSource("apps", "desktop", "src", "views", "settings", panelFile),
      /ENOENT/,
      `${panelFile} should be deleted instead of left as dead UI`
    );
  }

  assert.throws(
    () => readSource("apps", "desktop", "src", "views", "settings", "TerrariaOpsPanel.tsx"),
    /ENOENT/,
    "unused TerrariaOpsPanel should be deleted with the other ops surfaces"
  );

  const zomboidDefinition = readSource("apps", "desktop", "src", "views", "settings", "modules", "projectzomboid.ts");
  assert.doesNotMatch(zomboidDefinition, /projectzomboid\.ops\./, "Zomboid settings should not keep the retired ops namespace alive");

  const terrariaDefinition = readSource("apps", "desktop", "src", "views", "settings", "modules", "terraria.ts");
  const terrariaHelpers = readSource("apps", "desktop", "src", "views", "settings", "modules", "terraria-helpers.ts");
  assert.doesNotMatch(terrariaDefinition, /terraria\.settings\.ops\./, "Terraria settings should not keep retired workbench copy");
  assert.doesNotMatch(terrariaHelpers, /terraria\.settings\.ops\./, "Terraria helpers should not keep retired workbench alerts");

  for (const [messageFile, retiredNamespace] of [
    ["abioticfactor.en.ts", /abiotic\.ops\./],
    ["abioticfactor.zh-cn.ts", /abiotic\.ops\./],
    ["necesse.en.ts", /servers\.necesse\.sectionJoin/],
    ["necesse.zh-cn.ts", /servers\.necesse\.sectionJoin/],
    ["palworld.en.ts", /palworld\.ops\./],
    ["palworld.zh-cn.ts", /palworld\.ops\./],
    ["projectzomboid.en.ts", /projectzomboid\.ops\./],
    ["projectzomboid.zh-cn.ts", /projectzomboid\.ops\./],
    ["terraria.en.ts", /terraria\.settings\.ops\./],
    ["terraria.zh-cn.ts", /terraria\.settings\.ops\./],
    ["valheim.en.ts", /valheim\.ops\./],
    ["valheim.zh-cn.ts", /valheim\.ops\./]
  ]) {
    const messages = readSource("apps", "desktop", "src", "i18n", "games", messageFile);
    assert.doesNotMatch(messages, retiredNamespace, `${messageFile} should not keep retired ops/invite copy`);
  }
});

test("selected server card uses a stable theme focus ring and visible keyboard focus", () => {
  const operationsCss = readWorkbenchOperationsCss();
  const runtimeConsoleCss = readSource("apps", "desktop", "src", "views", "servers", "workbench", "operations", "server-runtime-console.css");

  assert.match(operationsCss, /\.server-list-card\.is-active\s*\{[^}]*border-color:\s*var\(--shell-focus\)/);
  assert.match(operationsCss, /\.server-list-card-hitarea:focus-visible\s*\{[^}]*outline:\s*2px solid var\(--shell-focus\)/);
  assert.doesNotMatch(operationsCss, /server-list-card-active-flow|conic-gradient/);
  assert.doesNotMatch(runtimeConsoleCss, /\.server-list-card/, "Runtime console styles must not override instance cards in either theme");
  assert.match(operationsCss, /\.server-list-card-copy \.row-title\s*\{[^}]*color:\s*var\(--shell-text\)/);
  assert.match(operationsCss, /\.server-list-card-meta\s*\{[^}]*color:\s*var\(--shell-muted\)/);
});

test("maintenance tab opens stable instance folders", () => {
  const serversView = readSource("apps", "desktop", "src", "views", "ServersView.tsx");
  const maintenance = readSource("apps", "desktop", "src", "views", "servers", "ServerMaintenanceWorkspace.tsx");

  assert.match(serversView, /<ServerMaintenanceWorkspace[\s\S]*?details=\{props\.selectedDetails\}/);
  assert.match(maintenance, /function instanceRootFromConfigFilePath/);
  assert.match(maintenance, /selectedInstanceRoot = instanceRootFromConfigFilePath\(props\.details\.config_file_path\)/);
  assert.match(maintenance, /selectedBackupDirectory = selectedInstanceRoot \? normalizePath\(`\$\{selectedInstanceRoot\}\/backups`\) : ""/);
  assert.doesNotMatch(`${serversView}\n${maintenance}`, /props\.selectedBackups\.length > 0 \? dirname\(normalizePath\(props\.selectedBackups\[0\]\.backup_path\)\) : ""/);
});

test("files and backups share the maintenance tab without a diagnostic card", () => {
  const serversView = readSource("apps", "desktop", "src", "views", "ServersView.tsx");
  const maintenance = readSource("apps", "desktop", "src", "views", "servers", "ServerMaintenanceWorkspace.tsx");

  assert.doesNotMatch(serversView, /type ServerDetailTab = [^\n]*"backups"/);
  assert.doesNotMatch(serversView, /type ServerDetailTab = [^\n]*"files"/);
  assert.doesNotMatch(serversView, /type ServerDetailTab = [^\n]*"diagnostics"/);
  const tabSpecs = readSource("apps", "desktop", "src", "views", "servers", "server-detail-tab-specs.ts");
  assert.match(serversView, /const detailTabs = buildServerDetailTabSpecs\(/, "server navigation must use the shared top-level workspaces");
  assert.doesNotMatch(tabSpecs, /id: "(?:backups|files|diagnostics)"/);
  assert.doesNotMatch(serversView, /activeDetailTab === "backups"/);
  assert.match(serversView, /<ServerMaintenanceWorkspace active=\{activeDetailTab === "maintenance"\}/);
  assert.match(maintenance, /<MaintenanceWorkspace active=\{props\.active\}[\s\S]*server-backup-history-card/);
  assert.doesNotMatch(`${serversView}\n${maintenance}`, /server-maintenance-health|server-runtime-diagnostic-list|runtimeDiagnostics/);
  assert.doesNotMatch(`${serversView}\n${maintenance}`, /server-maintenance-launch|servers\.details\.launchDiagnosticsBody|onRefreshPreview/);
});

test("maintenance tab owns native save and managed backup policy editing", () => {
  const serversView = readSource("apps", "desktop", "src", "views", "ServersView.tsx");
  const maintenance = readSource("apps", "desktop", "src", "views", "servers", "ServerMaintenanceWorkspace.tsx");
  const policyEditor = readSource("apps", "desktop", "src", "views", "servers", "SavePolicyEditor.tsx");

  assert.match(serversView, /import \{ ServerMaintenanceWorkspace \}/);
  assert.match(maintenance, /import \{ SavePolicyEditor \}/);
  assert.match(policyEditor, /server-backup-policy-editor/);
  assert.match(policyEditor, /auto_backup_on_stop/);
  assert.match(policyEditor, /backup_retention_count/);
  assert.match(policyEditor, /onSaveSettings/);
  assert.match(policyEditor, /<NativeSavePolicyFields/);
  assert.match(policyEditor, /surface: "maintenance"/);
  assert.match(maintenance, /<SavePolicyEditor[\s\S]*details=\{props\.details\}/);
  assert.match(maintenance, /<SavePolicyEditor[\s\S]*readOnly=\{readOnly\} onSaveSettings=\{props\.onSaveSettings\}/);
});

test("maintenance keeps storage, policy and broadcast in its section navigation without nested tabs or disclosures", () => {
  const serversView = readSource("apps", "desktop", "src", "views", "ServersView.tsx");
  const maintenanceWorkspace = readSource("apps", "desktop", "src", "views", "servers", "MaintenanceWorkspace.tsx");
  const serverMaintenance = readSource("apps", "desktop", "src", "views", "servers", "ServerMaintenanceWorkspace.tsx");
  const backupTable = readSource("apps", "desktop", "src", "views", "servers", "BackupTable.tsx");
  const broadcastWorkbench = readSource("apps", "desktop", "src", "views", "servers", "AiBroadcastWorkbench.tsx");
  const operationsCss = readWorkbenchOperationsCss();
  assert.match(serversView, /<ServerMaintenanceWorkspace active=\{activeDetailTab === "maintenance"\}/,
    "the instance view must mount the shared maintenance owner");
  assert.match(maintenanceWorkspace, /ConfigurationSectionNavigation/);
  assert.match(maintenanceWorkspace, /maintenance-workspace/);
  assert.match(
    serverMaintenance,
    /id: "backups"[\s\S]*server-backup-history-card[\s\S]*DstWorldImportPanel[\s\S]*id: "save-policy"[\s\S]*server-backup-policy-card[\s\S]*id: "runtime"[\s\S]*InstanceAutostartEditor[\s\S]*RuntimeRecoveryEditor[\s\S]*id: "storage"[\s\S]*InstanceIsolationPanel/
  );
  assert.doesNotMatch(serverMaintenance, /<details|<summary|maintenance-disclosure/);
  assert.doesNotMatch(serverMaintenance, /role="tablist"|server-file-panel--paths|server-maintenance-backup-grid/);
  assert.doesNotMatch(maintenanceWorkspace, /role="tablist"|<details|<summary/);
  assert.match(maintenanceWorkspace, /id: "broadcast"[\s\S]*content: broadcast/);
  assert.doesNotMatch(serverMaintenance, /server-maintenance-launch|launchDiagnosticsBody|launchCommandFallback/);
  assert.match(serverMaintenance, /server-workbench-surface/);
  assert.match(broadcastWorkbench, /server-maintenance-card server-broadcast-panel/);
  assert.doesNotMatch(broadcastWorkbench, /<details|<summary/);
  assert.match(serverMaintenance, /backupPath=\{selectedBackupDirectory\}/);
  assert.equal((serverMaintenance.match(/<InstanceIsolationPanel/g) ?? []).length, 1);
  assert.match(serverMaintenance, /server-file-backup-summary/);
  assert.match(backupTable, /server-file-backup-table/);
  assert.doesNotMatch(serversView, /function HostShortcutCard/);
  assert.doesNotMatch(serversView, /function BackupSectionPanel/);
  assert.doesNotMatch(serversView, /server-local-files-grid/);
  assert.doesNotMatch(serversView, /server-backup-grid/);

  assert.match(operationsCss, /\.maintenance-workspace/);
  assert.match(operationsCss, /\.server-workbench-surface/);
  assert.match(operationsCss, /\.maintenance-workspace__section/);
  assert.match(operationsCss, /\.server-backup-policy-editor/);
  assert.match(serverMaintenance, /server-runtime-policy-card/);
  assert.match(operationsCss, /\.server-recovery-options/);
  assert.match(operationsCss, /\.server-file-backup-table/);
  assert.match(operationsCss, /\.server-native-save-policies/);
  assert.match(operationsCss, /\.server-backup-list-scroll/);
  assert.doesNotMatch(operationsCss, /\.server-diagnostics-summary|\.server-diagnostics-shell\[open\]/);
  assert.doesNotMatch(operationsCss, /\.server-backup-panel\b/);
});

test("manual backup creation reloads the backend-pruned backup list", () => {
  const actionsSource = readSource("apps", "desktop", "src", "hooks", "useDesktopActions.ts");
  const createBackupBody = actionsSource.match(/async function handleCreateBackup\(instanceId: string\) \{[\s\S]*?\n  async function handleRestoreBackup/);

  assert.ok(createBackupBody, "handleCreateBackup should be present");
  assert.match(createBackupBody[0], /const result = await createInstanceBackup\(instanceId\);/);
  assert.match(createBackupBody[0], /const backups = await listInstanceBackups\(instanceId\);/);
  assert.doesNotMatch(createBackupBody[0], /currentBackups/);
});

test("server list instance cards do not use per-game cover overrides", () => {
  const moduleArt = readSource("apps", "desktop", "src", "module-art.ts");

  assert.doesNotMatch(moduleArt, /dontstarve-server-list/);
  assert.doesNotMatch(moduleArt, /serverListSrc/);
});
