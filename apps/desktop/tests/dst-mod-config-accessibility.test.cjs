const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const panelPath = path.resolve(__dirname, "../src/views/servers/DstModConfigPanel.tsx");
const translate = (key, params, fallback) => (fallback ?? key)
  .replace(/\{(\w+)\}/g, (match, name) => params?.[name] ?? match);

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    if (filename === panelPath) {
      const load = module.require.bind(module);
      module.require = (request) => request === "../../i18n"
        ? { useI18n: () => ({ t: translate }) }
        : load(request);
    }
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = (module) => module._compile("", module.filename);

const { DstModConfigPanel } = require(panelPath);

test("matching overworld and caves Mod options keep unique control and help targets", () => {
  const props = {
    settings: { master_enabled_workshop_mod_ids: "1234567890" },
    selectedModId: "1234567890",
    configurationSpecs: [{
      mod_id: "1234567890",
      status: "ready",
      options: [{
        name: "enabled",
        label: "Enabled",
        hover: "Enable this option for the selected world.",
        default_value: { kind: "boolean", value: true },
        options: []
      }]
    }],
    loadingSpecs: false,
    onSettingsChange() {}
  };
  const html = renderToStaticMarkup(React.createElement(React.Fragment, null,
    React.createElement(DstModConfigPanel, props),
    React.createElement(DstModConfigPanel, props)
  ));
  const inputIds = [...html.matchAll(/<select[^>]*\bid="([^"]+)"/g)].map((match) => match[1]);
  const labelTargets = [...html.matchAll(/<label[^>]*\bfor="([^"]+)"/g)].map((match) => match[1]);
  const helpIds = [...html.matchAll(/\bid="([^"]+-description)"/g)].map((match) => match[1]);
  assert.equal(inputIds.length, 2);
  assert.equal(new Set(inputIds).size, 2);
  assert.deepEqual(labelTargets, inputIds);
  assert.equal(helpIds.length, 2);
  assert.equal(new Set(helpIds).size, 2);
});

test("inactive Mod controls keep their configuration without asking the user to clear a raw override", () => {
  const settings = {
    master_enabled_workshop_mod_ids: "1234567890",
    master_mod_configuration_options: { "1234567890": { enabled: false } },
    master_modoverrides_lua: "return make_mods()"
  };
  const snapshot = structuredClone(settings);
  const props = {
    settings, selectedModId: "1234567890", disabled: true, loadingSpecs: false,
    configurationSpecs: [{
      mod_id: "1234567890", status: "ready", options: [{
        name: "enabled", label: "Enabled", default_value: { kind: "boolean", value: true }, options: []
      }]
    }],
    onSettingsChange() { assert.fail("rendering inactive settings must not rewrite them"); }
  };
  const html = renderToStaticMarkup(React.createElement(DstModConfigPanel, props));
  assert.match(html, /<select[^>]*disabled=""/);
  assert.doesNotMatch(html, /Clear it before changing these options/);
  assert.deepEqual(settings, snapshot);
  const activeHtml = renderToStaticMarkup(React.createElement(DstModConfigPanel, { ...props, disabled: false }));
  assert.match(activeHtml, /<select[^>]*disabled=""/, "raw scripts must not accept ineffective structured edits");
  assert.match(activeHtml, /Clear it before changing these options/,
    "the active Master raw-Mod workflow retains its existing warning");
});

test("loading, unread, failed and explicitly empty configuration remain distinct", () => {
  const props = {
    settings: { master_enabled_workshop_mod_ids: "123456" },
    selectedModId: "123456", configurationSpecs: [], loadingSpecs: false,
    onRetryConfiguration() {}, onSettingsChange() {}
  };
  const render = (overrides) => renderToStaticMarkup(React.createElement(DstModConfigPanel, { ...props, ...overrides }));
  const unread = render({});
  assert.match(unread, /configuration has not been read yet/);
  assert.match(unread, /Read configuration again/);
  assert.doesNotMatch(unread, /does not expose configurable options|does not declare any configurable/);

  const loading = render({ loadingSpecs: true });
  assert.match(loading, /aria-busy="true"/);
  assert.match(loading, /role="status"[\s\S]*>Reading Mod options/);
  assert.doesNotMatch(loading, /configuration has not been read yet|does not expose configurable/);

  const failed = render({ configurationError: "read failed" });
  assert.match(failed, /role="alert"/);
  assert.match(failed, /read failed/);
  assert.match(failed, /Read configuration again/);
  assert.doesNotMatch(failed, /does not expose configurable options/);

  const empty = render({ configurationSpecs: [{ mod_id: "123456", status: "no_options", options: [] }] });
  assert.match(empty, /No editable options were read from this downloaded version/);
  assert.match(empty, /Check each required Mod separately/);
  assert.match(empty, /Read configuration again/);
  assert.doesNotMatch(empty, /configuration has not been read yet|does not declare any configurable options/);

  const missingInfo = render({ configurationSpecs: [{ mod_id: "123456", status: "missing_modinfo", options: [] }] });
  assert.match(missingInfo, /missing modinfo.lua/);
  assert.doesNotMatch(missingInfo, /No editable options were read|does not offer readable options/);
});

test("text and numeric defaults without declared choices remain editable inputs", () => {
  const props = {
    settings: {
      master_enabled_workshop_mod_ids: "123456",
      master_mod_configuration_options: { "123456": { count: 8, title: "Custom" } }
    },
    selectedModId: "123456", loadingSpecs: false, onSettingsChange() {},
    configurationSpecs: [{ mod_id: "123456", status: "loaded", options: [
      { name: "count", label: "Count", default_value: { kind: "number", value: 5 }, options: [] },
      { name: "title", label: "Title", default_value: { kind: "string", value: "Default" }, options: [] }
    ] }]
  };
  const html = renderToStaticMarkup(React.createElement(DstModConfigPanel, props));
  assert.match(html, /<input[^>]*type="number"[^>]*placeholder="5"[^>]*value="8"/);
  assert.match(html, /<input[^>]*type="text"[^>]*placeholder="Default"[^>]*value="Custom"/);
  assert.doesNotMatch(html, /<select/);
  const refreshing = renderToStaticMarkup(React.createElement(DstModConfigPanel, { ...props, loadingSpecs: true }));
  assert.doesNotMatch(refreshing, /<input|<select/);
});

test("downloaded client-only Mods expose their options read-only with the game-client explanation", () => {
  const props = {
    settings: { master_enabled_workshop_mod_ids: "3734727477" },
    selectedModId: "3734727477", loadingSpecs: false, onSettingsChange() {},
    configurationSpecs: [{ mod_id: "3734727477", client_only: true, status: "loaded", options: [
      { name: "keybind", label: "Key", default_value: { kind: "string", value: "K" }, options: [] }
    ] }]
  };
  const html = renderToStaticMarkup(React.createElement(DstModConfigPanel, props));
  assert.match(html, /only runs in the game client/);
  assert.match(html, /<input[^>]*disabled=""/);
  assert.doesNotMatch(html, /Download and read configuration|Download this Mod/);
});

test("both-shard editing warns when only one shard overrides the effective Mod default", () => {
  const id = "123456";
  const props = {
    selectedModId: id, loadingSpecs: false, onSettingsChange() {},
    configurationSpecs: [{ mod_id: id, status: "loaded", options: [
      { name: "difficulty", label: "Difficulty", default_value: { kind: "number", value: 1 }, options: [] }
    ] }]
  };
  for (const shard of ["master", "caves"]) {
    const render = (value) => renderToStaticMarkup(React.createElement(DstModConfigPanel, {
      ...props, settings: { master_enabled_workshop_mod_ids: id, caves_enabled_workshop_mod_ids: id,
        [`${shard}_mod_configuration_options`]: { [id]: { difficulty: value } } }
    }));
    assert.match(render(10), /Existing shard values differ/,
      `${shard} explicit value differs from the other shard's implicit default`);
    assert.doesNotMatch(render(1), /Existing shard values differ/,
      "an explicit default is equivalent to the omitted default");
  }
});

test("both-shard controls show the overworld default while keeping cave-only overrides resettable", () => {
  const id = "123456";
  const props = {
    settings: { master_enabled_workshop_mod_ids: id, caves_enabled_workshop_mod_ids: id,
      caves_mod_configuration_options: { [id]: { difficulty: 10 } } },
    selectedModId: id, loadingSpecs: false, onSettingsChange() {},
    configurationSpecs: [{ mod_id: id, status: "loaded", options: [{
      name: "difficulty", label: "Difficulty", default_value: { kind: "number", value: 1 },
      options: [1, 10].map((value) => ({ label: String(value), value: { kind: "number", value } }))
    }] }]
  };
  const render = (overrides) => renderToStaticMarkup(React.createElement(DstModConfigPanel, { ...props, ...overrides }));
  const html = render({});
  assert.match(html, /<option value="number:1" selected="">/,
    "the displayed value must match the warned overworld baseline, including its implicit default");
  assert.doesNotMatch(html, /<option value="number:10" selected="">/);
  assert.match(html, /<button type="button" class="ghost-button">Restore all defaults<\/button>/,
    "restoring defaults must still clear an override present only in caves");
  assert.match(render({ disabled: true }), /<button type="button" class="ghost-button" disabled="">Restore all defaults/,
    "reset keeps the host's editing lock");
});

test("archived DST uses the same options layout and preserves saved false, zero and empty strings without definitions", () => {
  const props = { settings: { master_enabled_workshop_mod_ids: "1234567890",
    master_mod_configuration_options: { "1234567890": { toggle: false, count: 0, text: "" } } },
    selectedModId: "1234567890", configurationSpecs: [], loadingSpecs: false, readOnly: true,
    onSettingsChange() { assert.fail("archived options must not mutate"); } };
  const html = renderToStaticMarkup(React.createElement(DstModConfigPanel, props));
  assert.match(html, /dst-mod-spec-list/);
  assert.match(html, /readOnly=""[^>]*value="false"/i);
  assert.match(html, /readOnly=""[^>]*value="0"/i);
  assert.match(html, /readOnly=""[^>]*value=""/i);
  assert.doesNotMatch(html, /Read configuration again|Restore all defaults/);
  const ids = [...html.matchAll(/\bid="([^"]+)"/g)].map((match) => match[1]);
  const labels = [...html.matchAll(/\bfor="([^"]+)"/g)].map((match) => match[1]);
  assert.equal(new Set(ids).size, ids.length);
  assert.deepEqual(labels, ids);
});