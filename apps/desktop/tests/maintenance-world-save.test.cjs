const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
for (const extension of [".ts", ".tsx"]) require.extensions[extension] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
require.extensions[".css"] = (module) => module._compile("", module.filename);
const { resolveWorldSaveAction } = require("../src/views/servers/world-save-action.ts");
const root = path.resolve(__dirname, "../../..");
const expected = {
  minecraft: { command_template: "save-all flush", transport: "source_rcon", port_name: "rcon", password_setting_key: "rcon_password", enabled_setting_key: "enable_rcon" },
  projectzomboid: { command_template: "save", transport: "source_rcon", port_name: "rcon", password_setting_key: "rcon_password" },
  terraria: { command_template: "save", transport: "stdin", process_key: "main" },
  palworld: { command_template: "save", transport: "palworld_rest", port_name: "rest_api", password_setting_key: "admin_password", enabled_setting_key: "rest_api_enabled" },
  unturned: { command_template: "Save", transport: "stdin", process_key: "main" }
};
function moduleDetails(moduleId) {
  const source = fs.readFileSync(path.join(root, "modules", moduleId, "module.toml"), "utf8");
  const actions = source.split("[[runtime.player_actions]]").slice(1).map((block) => {
    const fields = [...block.split(/\r?\n\[/)[0].matchAll(/^([a-z_]+) = ("([^"]*)"|true|false)$/gm)];
    return Object.fromEntries(fields.map((field) => [field[1], field[3] ?? (field[2] === "true")]));
  });
  return { summary: { id: moduleId }, runtime: { player_actions: actions } };
}
function instance(moduleId = "minecraft", id = `${moduleId}-fixture`) {
  return { summary: { id, module_id: moduleId, status: "Running", active_process_count: 1 },
    active_run: { run_id: 1, processes: [{ process_key: "main", status: "Running" }] },
    settings_json: JSON.stringify({ enable_rcon: true, rcon_password: "fixture-server-password", rest_api_enabled: true, admin_password: "fixture-admin-password" }),
    ports: [{ name: "rcon", port: 25575, protocol: "tcp" }, { name: "rest_api", port: 8212, protocol: "tcp" }] };
}
const settle = () => new Promise((resolve) => setImmediate(resolve));
function deferred() { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
function response(overrides = {}) { return { instance_id: "fixture", command: "save-all flush", process_key: "rcon", display_name: "RCON", pid: 1,
  response_text: "Saved the game", write_confirmation_pending: false, submitted_at_unix_ms: 1, ...overrides }; }
function descendants(node) { return React.isValidElement(node) ? [node, ...React.Children.toArray(node.props.children).flatMap(descendants)] : []; }

function harness(send = async () => response()) {
  const hooks = [], effects = [], calls = [];
  let cursor = 0, writes = 0;
  const same = (a, b) => a?.length === b?.length && a.every((value, index) => Object.is(value, b[index]));
  const react = { ...React,
    useState(initial) {
      const slot = cursor++;
      if (!(slot in hooks)) hooks[slot] = { value: typeof initial === "function" ? initial() : initial };
      return [hooks[slot].value, (next) => { hooks[slot].value = typeof next === "function" ? next(hooks[slot].value) : next; writes++; }];
    },
    useMemo(create, deps) { const slot = cursor++; if (!same(hooks[slot]?.deps, deps)) hooks[slot] = { deps, value: create() }; return hooks[slot].value; },
    useRef(value) { const slot = cursor++; if (!(slot in hooks)) hooks[slot] = { current: value }; return hooks[slot]; },
    useEffect(create, deps) { const slot = cursor++; if (same(hooks[slot]?.deps, deps)) return; const previous = hooks[slot];
      hooks[slot] = { deps }; effects.push(() => { previous?.cleanup?.(); hooks[slot].cleanup = create(); }); }
  };
  const filename = path.resolve(__dirname, "../src/views/servers/ImmediateWorldSave.tsx");
  const loaded = new Module(filename, module); loaded.filename = filename;
  const requireFromFile = Module.createRequire(filename);
  loaded.require = (name) => {
    if (name === "react") return react;
    if (name === "../../api") return { sendInstanceRuntimeCommand: (...args) => { calls.push(args); return send(...args); } };
    if (name === "../../i18n") return { useI18n: () => ({ locale: "en-US", t: (key) => key }), selectLocaleText: (_, _zh, en) => en };
    if (name === "../../app-state") return { describeError: (error) => error.message ?? String(error) };
    if (name === "../../components/ShellIcon") return { ShellIcon: () => React.createElement("svg") };
    return requireFromFile(name);
  };
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  const props = { details: instance(), moduleDetails: moduleDetails("minecraft") };
  function render() {
    let view;
    for (let attempt = 0; attempt < 10; attempt++) {
      cursor = 0; const before = writes; view = loaded.exports.ImmediateWorldSave(props);
      while (effects.length) effects.shift()();
      if (before === writes) break;
      assert.ok(attempt < 9, "Save effects must settle");
    }
    const nodes = descendants(view);
    return { root: view, html: renderToStaticMarkup(view), button: nodes.find((node) => node.type === "button") };
  }
  return { props, calls, render, unmount() { for (const hook of hooks) hook?.cleanup?.(); } };
}

test("immediate saves have exact declared native commands and are reachable from Maintenance", () => {
  for (const [id, contract] of Object.entries(expected)) {
    const module = moduleDetails(id);
    const saves = module.runtime.player_actions.filter((action) => action.id === "save_world");
    assert.equal(saves.length, 1, `${id} has exactly one save declaration`);
    for (const [key, value] of Object.entries(contract)) assert.equal(saves[0][key], value, `${id}.${key}`);
    assert.equal(saves[0].target_required, undefined);
    assert.equal(saves[0].destructive, undefined);
    assert.equal(resolveWorldSaveAction(instance(id), module, "en-US").unavailable, null, id);
  }
  assert.equal(moduleDetails("projectzomboid").runtime.player_actions.find((action) => action.id === "save_world").enabled_setting_key, undefined,
    "Project Zomboid must not depend on a nonexistent RCON toggle");
  const view = fs.readFileSync(path.join(root, "apps/desktop/src/views/ServersView.tsx"), "utf8");
  const maintenance = fs.readFileSync(path.join(root, "apps/desktop/src/views/servers/ServerMaintenanceWorkspace.tsx"), "utf8");
  assert.match(view, /<ServerMaintenanceWorkspace active=\{activeDetailTab === "maintenance"\}/);
  assert.match(maintenance, /id: "backups"[^]*?<ImmediateWorldSave details=\{props\.details\} moduleDetails=\{props\.moduleDetails\} readOnly=\{readOnly\}/);
});

test("unavailable saves explain stopped process, unloaded capabilities and missing remote configuration", () => {
  const details = instance();
  const module = moduleDetails("minecraft");
  assert.match(resolveWorldSaveAction(details, null, "en-US").unavailable, /Save capability is unavailable/);
  assert.match(resolveWorldSaveAction({ ...details, summary: { ...details.summary, status: "Stopped", active_process_count: 0 }, active_run: null }, module, "en-US").unavailable, /Start the server/);
  for (const [settings, expectedError] of [[{ enable_rcon: false }, /Enable RCON/], [{ enable_rcon: true, rcon_password: "" }, /administrator password/]]) {
    assert.match(resolveWorldSaveAction({ ...details, settings_json: JSON.stringify(settings) }, module, "en-US").unavailable, expectedError);
  }
  assert.match(resolveWorldSaveAction({ ...details, ports: [] }, module, "en-US").unavailable, /TCP port/);
  assert.equal(resolveWorldSaveAction(instance("valheim"), moduleDetails("valheim"), "en-US"), null);
  assert.match(resolveWorldSaveAction({ ...instance("terraria"), active_run: { run_id: 1, processes: [{ process_key: "other", status: "Running" }] } },
    moduleDetails("terraria"), "en-US").unavailable, /Start the server/);
});

test("maintenance sends declared save action independently of AI and preserves native response", async () => {
  for (const id of Object.keys(expected)) {
    const state = harness(async () => response({ response_text: "Native save response" }));
    state.props.details = instance(id); state.props.moduleDetails = moduleDetails(id);
    state.render().button.props.onClick(); await settle();
    assert.equal(state.calls.length, 1, id);
    assert.equal(state.calls[0][0], `${id}-fixture`);
    assert.equal(state.calls[0][1], expected[id].command_template);
    assert.deepEqual(state.calls[0][3], { runtimeActionId: "save_world" });
    assert.match(state.render().html, /Native save response/);
    assert.match(state.render().html, /Save request sent/);
    state.unmount();
  }
});

test("world-save settings refresh cannot duplicate an in-flight operation", async () => {
  const waiting = deferred(); const state = harness(() => waiting.promise);
  const oldButton = state.render().button; oldButton.props.onClick(); oldButton.props.onClick();
  state.props.details = { ...state.props.details, settings_json: JSON.stringify({ enable_rcon: true, rcon_password: "fixture-pz-rcon-safe" }) };
  assert.equal(state.render().button.props.disabled, true);
  state.render().button.props.onClick(); assert.equal(state.calls.length, 1);
  waiting.resolve(response()); await settle();
  assert.equal(state.render().button.props.disabled, false); state.unmount();
});

test("late save results do not cross instance boundaries", async () => {
  const waiting = deferred(); const state = harness(() => waiting.promise);
  state.render().button.props.onClick();
  state.props.details = instance("minecraft", "other-instance"); state.render();
  waiting.resolve(response({ response_text: "old instance response" })); await settle();
  assert.doesNotMatch(state.render().html, /old instance response/);
  assert.equal(state.render().button.props.disabled, false); state.unmount();
});

test("world-save pending and error responses remain unconfirmed and do not retry", async () => {
  const pending = harness(async () => response({ write_confirmation_pending: true, response_text: "native writer pending" }));
  pending.render().button.props.onClick(); await settle();
  assert.match(pending.render().html, /awaiting write confirmation/); assert.match(pending.render().html, /native writer pending/);
  assert.equal(pending.calls.length, 1); pending.unmount();
  const failed = harness(async () => { throw new Error("native save connection lost"); });
  failed.render().button.props.onClick(); await settle();
  assert.match(failed.render().html, /role="alert"[^]*native save connection lost/);
  assert.equal(failed.calls.length, 1); failed.unmount();
});

test("archived immediate save keeps the shared card and restore hint without sending a command", async () => {
  const state = harness();
  state.props.readOnly = true;
  const view = state.render();
  assert.equal(view.button.props.disabled, true);
  assert.match(view.html, /servers.archives.workspace.restoreForMaintenance/);
  view.button.props.onClick();
  await settle();
  assert.deepEqual(state.calls, []);
  state.unmount();
});