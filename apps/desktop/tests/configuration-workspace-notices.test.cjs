const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const context = { locale: "en-US", t: (key, _params, fallback) => fallback ?? key };
for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    const load = module.require.bind(module);
    module.require = (id) => /\/i18n$/.test(id) ? { ...load(id), useI18n: () => context } : load(id);
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = (module) => module._compile("", module.filename);
require.extensions[".png"] = (module, filename) => module._compile(`module.exports = ${JSON.stringify(filename)};`, filename);

const { parseGuidedSettingsSchema } = require("../src/views/settings/guided-settings.ts");
const filename = path.resolve(__dirname, "../src/views/settings/ConfigurationWorkspace.tsx");
const loaded = new Module(filename, module);
loaded.filename = filename;
const load = Module.createRequire(filename);
loaded.require = (id) => id === "../../i18n" ? { ...load(id), useI18n: () => context }
  : id === "./useAutoSaveInstanceSettings" ? {
    useAutoSaveInstanceSettings: () => ({ status: { state: "saved" }, retry() {} })
  } : id === "./useModuleConfigurationIcons" ? {
    useModuleConfigurationIcons: () => ({ icons: {}, loading: false })
  } : id === "./useInstancePortRegistration" ? {
    useInstancePortRegistration: () => ({ ports: [], defaultPorts: [], setPorts() {} })
  } : load(id);
loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);

test("DST configuration omits migrated roster notices while Player Access retains all three lists", () => {
  const moduleDetails = {
    summary: { id: "dontstarve", name: "DST" },
    schema_json: fs.readFileSync(path.resolve(__dirname, "../../../modules/dontstarve/schema.json"), "utf8")
  };
  const html = renderToStaticMarkup(React.createElement(loaded.exports.ConfigurationWorkspace, {
    details: {
      summary: { id: "dst-notices", module_id: "dontstarve", name: "DST", bind_ip: "0.0.0.0" },
      settings_json: "{}", ports: [], backup_retention_count: 5
    },
    moduleDetails, bindAddressCandidates: [], runtime: null, launchPlan: null, launchPlanError: null,
    onSave() {}
  }));
  assert.equal(html.includes("configuration-workspace__evidence-item"), false, "migrated lists must not render notices");
  assert.doesNotMatch(html, /Configure this setting in Player Access/);
  assert.match(html, /configuration-dontstarve-cluster-name-input/);
  const playerAccess = parseGuidedSettingsSchema(moduleDetails, "en-US", context.t, { surface: "player_access" });
  for (const key of ["admin_list", "whitelist", "blocklist"]) {
    assert.ok(playerAccess.fields.some((field) => field.key === key), key);
  }
});

test("a room containing a module-addon field renders its real control once without an unavailable placeholder", () => {
  const moduleDetails = {
    summary: { id: "windrose", name: "Windrose" },
    schema_json: fs.readFileSync(path.resolve(__dirname, "../../../modules/windrose/schema.json"), "utf8")
  };
  const html = renderToStaticMarkup(React.createElement(loaded.exports.ConfigurationWorkspace, {
    details: {
      summary: { id: "windrose-room", module_id: "windrose", name: "Windrose", bind_ip: "0.0.0.0", status: "Stopped" },
      settings_json: JSON.stringify({ world_island_id: "fixture-world", world_name: "Fixture World" }),
      ports: [], backup_retention_count: 5
    },
    moduleDetails, bindAddressCandidates: [], runtime: null, launchPlan: null, launchPlanError: null,
    onSave() {}
  }));
  assert.equal((html.match(/data-field-key="world_name"/g) ?? []).length, 1);
  assert.doesNotMatch(html, /configuration-field-unavailable/);
});

test("SCUM General omits generated version notices while retaining controls and field help", () => {
  const selected = new Module(filename, module);
  selected.filename = filename;
  selected.require = (id) => {
    if (id !== "./configuration-workspace-model") return loaded.require(id);
    const model = load(id);
    return {
      ...model,
      resolveConfigurationSectionId: (workspace, requested) =>
        model.resolveConfigurationSectionId(workspace, requested ?? "general")
    };
  };
  selected._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  const originalTranslate = context.t;
  const fieldHelp = "Allows players to send messages to global chat.";
  context.t = (key, params, fallback) => key === "scum.settings.native.allow_global_chat.description"
    ? fieldHelp : originalTranslate(key, params, fallback);
  let html;
  try {
    html = renderToStaticMarkup(React.createElement(selected.exports.ConfigurationWorkspace, {
      details: {
        summary: { id: "scum-notices", module_id: "scum", name: "SCUM", bind_ip: "0.0.0.0", status: "Stopped" },
        settings_json: "{}", ports: [], backup_retention_count: 5
      },
      moduleDetails: {
        summary: { id: "scum", name: "SCUM" },
        schema_json: fs.readFileSync(path.resolve(__dirname, "../../../modules/scum/schema.json"), "utf8")
      },
      bindAddressCandidates: [], runtime: null, launchPlan: null, launchPlanError: null,
      onSave() {}
    }));
  } finally {
    context.t = originalTranslate;
  }
  assert.match(html, /data-scum-section="general"/);
  assert.equal(html.includes("configuration-workspace__evidence-item"), false);
  assert.doesNotMatch(html, /<strong>Server Settings Version<\/strong>/);
  const control = html.match(/<input\b[^>]*id="configuration-scum-server-general-allow-global-chat-input"[^>]*>/)?.[0];
  assert.ok(control, "the native chat setting remains editable");
  assert.match(control, /type="checkbox"/);
  assert.doesNotMatch(control, /\bdisabled=/);
  const descriptionId = control.match(/aria-describedby="([^"]+)"/)?.[1];
  assert.ok(descriptionId, "the field retains its accessible help association");
  const helpTag = html.match(new RegExp(`<span\\b[^>]*id="${descriptionId}"[^>]*>`))?.[0];
  assert.ok(helpTag);
  assert.match(helpTag, /role="tooltip"/);
  assert.ok(html.includes(fieldHelp));
});
