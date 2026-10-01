const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".tsx"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};

const src = path.resolve(__dirname, "../src");
const read = (file) => fs.readFileSync(path.join(src, file), "utf8");
const settle = () => new Promise((resolve) => setImmediate(resolve));
const t = (key, params, fallback) => (fallback ?? key).replace(/\{(\w+)\}/g, (_, name) => String(params?.[name] ?? ""));

function descendants(node) {
  if (!React.isValidElement(node)) return [];
  return [node, ...React.Children.toArray(node.props.children).flatMap(descendants)];
}

function harness(importWorld = async () => ({ imported_master: true, imported_caves: false,
  copied_file_count: 2, copied_total_bytes: 2048, safeguard_path: "backups/before-import" })) {
  const hooks = [];
  let cursor = 0;
  const react = {
    ...React,
    useId: () => "save-import",
    useState(initial) {
      const index = cursor++;
      if (!(index in hooks)) hooks[index] = initial;
      return [hooks[index], (next) => { hooks[index] = typeof next === "function" ? next(hooks[index]) : next; }];
    },
    useRef(initial) {
      const index = cursor++;
      if (!(index in hooks)) hooks[index] = { current: initial };
      return hooks[index];
    }
  };
  const filename = path.join(src, "views/servers/DstWorldImportPanel.tsx");
  const loaded = new Module(filename, module);
  loaded.filename = filename;
  const requireFromFile = Module.createRequire(filename);
  loaded.require = (id) => id === "../settings/ConfigurationFieldHelp" ? require("./helpers/configuration-help-harness.cjs")
    : id === "react" ? react
    : id === "../../i18n" ? { useI18n: () => ({ locale: "en-US", t }) }
      : id === "../../app-state" ? { describeError: (error) => error.message }
        : id === "../../components/ShellIcon" ? { ShellIcon: () => null } : requireFromFile(id);
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  const calls = [];
  const props = {
    details: { summary: { id: "dst-a", module_id: "dontstarve", status: "Stopped" }, active_run: null,
      config_file_path: "D:/instances/dst/config/server.cfg", settings_json: '{"cluster_name":"Keep me"}' },
    onPickDirectory: async () => "D:/existing-cluster",
    async onImportWorldData(...args) { calls.push(args); return importWorld(...args); }
  };
  function render() {
    cursor = 0;
    const root = loaded.exports.DstWorldImportPanel(props);
    const buttons = descendants(root).filter((node) => node.type === "button");
    const nodes = descendants(root);
    return { root, nodes, html: renderToStaticMarkup(root), choose: buttons[0], import: buttons[1] };
  }
  return { props, calls, render,
    async choose() { render().choose.props.onClick(); await settle(); }
  };
}

test("DST import belongs only to Maintenance and configuration retains world-generation guidance", () => {
  const source = read("views/ServersView.tsx");
  assert.match(source, /<ServerMaintenanceWorkspace active=\{activeDetailTab === "maintenance"\}/);
  const maintenance = read("views/servers/ServerMaintenanceWorkspace.tsx");
  assert.match(maintenance, /<MaintenanceWorkspace active=\{props\.active\}/, "the shared Maintenance workspace must own the import panel");
  assert.match(maintenance, /summary\.module_id === "dontstarve"[\s\S]*<DstWorldImportPanel/);
  assert.match(maintenance, /onImportWorldData=\{props\.onImportDontStarveWorldData\}/);
  assert.equal((maintenance.match(/<DstWorldImportPanel\b/g) ?? []).length, 1);
  assert.doesNotMatch(source, /<DstWorldImportPanel\b/, "the normal instance view must not duplicate the shared import panel");
  assert.match(maintenance, /!readOnly && props\.details\.summary\.module_id === "dontstarve"/);
  assert.doesNotMatch(read("views/settings/modules/dontstarve.ts"), /dst-world-bootstrap|DstWorldBootstrap/);
  assert.doesNotMatch(read("views/settings/ConfigurationWorkspace.tsx"), /ConfigurationRendererOperations|onPickDirectory|onImportDontStarveWorldData/);
  assert.match(read("views/settings/modules/dontstarve-preset-notice.tsx"), /dst\.settings\.generation\.appliesToNew/);
  assert.equal(fs.existsSync(path.join(src, "views/settings/ConfigurationRendererOperations.tsx")), false);
  assert.equal(fs.existsSync(path.join(src, "views/settings/DstWorldBootstrapPanel.tsx")), false);
});

test("save import keeps its source and actions visible without disclosure controls", async () => {
  const state = harness();
  assert.equal(state.render().nodes.some((node) => node.type === "details" || node.type === "summary"), false);
  assert.match(state.render().html, /Import save/);
  assert.doesNotMatch(state.render().root.props.className, /server-maintenance-card/);
  const initial = state.render();
  assert.equal(initial.choose.props.title, undefined, "Import help must not duplicate the shared tooltip with a native title");
  const descriptionId = initial.choose.props["aria-describedby"];
  assert.ok(descriptionId);
  const description = initial.nodes.find((node) => node.props.id === descriptionId);
  assert.match(description.props.children, /overworld \(Master\).*Caves.*Islands.*Volcano/);
  await state.choose();
  assert.match(state.render().html, /D:\/existing-cluster/);
  assert.equal(state.render().import.props.disabled, false);
});

test("import submits only the selected instance and source, preserving configuration and feedback", async () => {
  const state = harness();
  const before = structuredClone(state.props.details);
  assert.equal(state.render().import.props.disabled, true);
  await state.choose();
  assert.equal(state.render().import.props.disabled, false);
  state.render().import.props.onClick();
  await settle();
  assert.deepEqual(state.calls, [["dst-a", "D:/existing-cluster"]]);
  assert.deepEqual(state.props.details, before);
  assert.match(state.render().html, /Replaces the current save after creating a backup/);
  assert.match(state.render().html, /Restores the source world&#x27;s Mod enablement and options/);
  assert.match(state.render().html, /downloads missing Mod files before starting/);
  assert.match(state.render().html, /Overworld imported/);
  assert.match(state.render().html, /backups\/before-import/);
});

test("an active DST run disables import and cannot submit through its click handler", async () => {
  const state = harness();
  await state.choose();
  state.props.details = { ...state.props.details, active_run: { id: "run-a" } };
  assert.equal(state.render().import.props.disabled, true);
  assert.match(state.render().html, /Stop the server first/);
  state.render().import.props.onClick();
  await settle();
  assert.equal(state.calls.length, 0);
});

test("failed imports retain the cause and permit retry after the pending operation", async () => {
  let reject;
  const pending = new Promise((_, fail) => { reject = fail; });
  const state = harness(() => pending);
  await state.choose();
  state.render().import.props.onClick();
  assert.equal(state.render().import.props.disabled, true);
  assert.equal(state.render().choose.props.disabled, true);
  reject(new Error("Invalid snapshot pair"));
  await settle();
  assert.match(state.render().html, /World import failed: Invalid snapshot pair/);
  assert.equal(state.render().import.props.disabled, false);
  assert.doesNotMatch(state.render().html, /Overworld imported/);
});

test("an in-flight import cannot submit twice and keeps progress and results visible", async () => {
  let resolve;
  const state = harness(() => new Promise((done) => { resolve = done; }));
  await state.choose();
  const submit = state.render().import.props.onClick;
  submit();
  submit();
  assert.equal(state.calls.length, 1);
  assert.equal(state.render().choose.props.disabled, true);
  assert.match(state.render().html, /role="status">Importing/);
  resolve({ imported_master: true, imported_caves: true, copied_file_count: 3,
    copied_total_bytes: 4096, safeguard_path: "backups/safeguard" });
  await settle();
  const view = state.render();
  assert.match(view.html, /Caves imported/);
  assert.match(view.html, /backups\/safeguard/);
  assert.equal(view.nodes.some((node) => node.props.hidden), false);
});

test("directory selection is serialized and a failure preserves the previous source", async () => {
  const state = harness();
  await state.choose();
  let reject;
  let pickCalls = 0;
  state.props.onPickDirectory = () => {
    pickCalls += 1;
    return new Promise((_, fail) => { reject = fail; });
  };
  const view = state.render();
  view.choose.props.onClick();
  view.choose.props.onClick();
  view.import.props.onClick();
  assert.equal(pickCalls, 1);
  assert.equal(state.calls.length, 0);
  assert.equal(state.render().import.props.disabled, true);
  reject(new Error("Directory picker unavailable"));
  await settle();
  assert.match(state.render().html, /Could not choose a save folder: Directory picker unavailable/);
  assert.match(state.render().html, /D:\/existing-cluster/);
  assert.equal(state.render().import.props.disabled, false);
  assert.equal(state.render().nodes.some((node) => node.props.hidden), false);
});

test("canceling directory selection preserves the selected source", async () => {
  const state = harness();
  await state.choose();
  state.props.onPickDirectory = async () => null;
  await state.choose();
  assert.match(state.render().html, /D:\/existing-cluster/);
  assert.equal(state.render().import.props.disabled, false);
});

test("four-shard import reports every restored world and its Workshop Mod count", async () => {
  const state = harness(async () => ({ imported_master: true, imported_caves: true,
    imported_shards: ["Master", "Caves", "Islands", "Volcano"], imported_workshop_mod_ids: ["1467214795", "3435352667"],
    copied_file_count: 8, copied_total_bytes: 8192, safeguard_path: "backups/four-shards" }));
  await state.choose();
  state.render().import.props.onClick();
  await settle();
  const html = state.render().html;
  assert.match(html, /Overworld imported.*Caves imported.*Islands imported.*Volcano imported/);
  assert.match(html, /Restored 2 Workshop Mods/);
});
