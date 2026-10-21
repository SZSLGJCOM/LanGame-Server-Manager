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
const { resolveWorldSaveAction, worldSaveDescription, worldSaveTitle } = require("../src/views/servers/world-save-action.ts");
const root = path.resolve(__dirname, "../../..");
const expected = {
  minecraft: { command_template: "save-all flush", transport: "stdin", process_key: "main" },
  projectzomboid: { command_template: "save", transport: "stdin", process_key: "main" },
  terraria: { command_template: "save", transport: "stdin", process_key: "main" },
  palworld: { command_template: "save", transport: "palworld_rest", port_name: "rest_api", password_setting_key: "admin_password", enabled_setting_key: "rest_api_enabled" },
  unturned: { command_template: "Save", transport: "stdin", process_key: "main" },
  arksurvivalevolved: { command_template: "saveworld", transport: "source_rcon", port_name: "rcon", password_setting_key: "admin_password", enabled_setting_key: "rcon_enabled" },
  arksurvivalascended: { command_template: "saveworld", transport: "source_rcon", port_name: "rcon", password_setting_key: "admin_password", enabled_setting_key: "rcon_enabled" },
  sevendaystodie: { command_template: "saveworld", transport: "telnet", port_name: "telnet", password_setting_key: "telnet_password", enabled_setting_key: "telnet_enabled" },
  necesse: { command_template: "save", transport: "stdin", process_key: "main" },
  rust: { command_template: "server.save", transport: "websocket_rcon", port_name: "rcon", password_setting_key: "rcon_password", enabled_setting_key: "rcon_web" },
  romestead: { command_template: "save", transport: "stdin", process_key: "main" },
  theforest: { command_template: "save", transport: "stdin", process_key: "main" },
  humanitz: { command_template: "save", transport: "source_rcon", port_name: "rcon", password_setting_key: "rcon_password", enabled_setting_key: "rcon_enabled" },
  satisfactory: { command_template: "save", transport: "satisfactory_https" },
  astroneer: { command_template: "save", transport: "astroneer_save", port_name: "console", password_setting_key: "console_password" },
  valheim: { command_template: "save", transport: "valheim_native" },
  runescapedragonwilds: { command_template: "save", transport: "dragonwilds_save" },
  corekeeper: { command_template: "save", transport: "corekeeper_save" },
  vrising: { command_template: "save", transport: "vrising_save" },
  sonsoftheforest: { command_template: "save", transport: "sonsoftheforest_save" },
  abioticfactor: { command_template: "save", transport: "abioticfactor_save" },
  returntomoria: { command_template: "save", transport: "returntomoria_save" },
  windrose: { command_template: "save", transport: "windrose_save" },
  enshrouded: { command_template: "save", transport: "enshrouded_save" },
  rimworld: { command_template: "save", transport: "rimworld_save" },
  barotrauma: { command_template: "save", transport: "barotrauma_save" },
  soulmask: { command_template: "SaveWorld 1", transport: "soulmask_save" },
  nightingale: { command_template: "save", transport: "nightingale_save" },
  conanexiles: { command_template: "save", transport: "conanexiles_save" },
  scum: { command_template: "save_persistent_state", transport: "scum_save" },
  dontstarve: { command_template: "c_save()", transport: "stdin", process_key: "master" }
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
    active_run: { run_id: 1, processes: [{ process_key: moduleId === "dontstarve" ? "master" : "main", status: "Running" }] },
    settings_json: JSON.stringify({ enable_rcon: true, rcon_password: "fixture-server-password", rest_api_enabled: true, admin_password: "fixture-admin-password",
      rcon_enabled: true, rcon_web: true, telnet_enabled: true, telnet_password: "fixture-server-password", console_password: "fixture-server-password" }),
    ports: [{ name: "rcon", port: 25575, protocol: "tcp" }, { name: "rest_api", port: 8212, protocol: "tcp" }, { name: "telnet", port: 8081, protocol: "tcp" }, { name: "console", port: 8778, protocol: "tcp" }] };
}
const settle = () => new Promise((resolve) => setImmediate(resolve));
function deferred() { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
function response(overrides = {}) { return { instance_id: "fixture", command: "save-all flush", process_key: "rcon", display_name: "RCON", pid: 1,
  response_text: "Saved the game", write_confirmation_pending: false, save_completion: "confirmed", submitted_at_unix_ms: 1, ...overrides }; }
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
  assert.equal(Object.keys(expected).length, 31);
  const actual = fs.readdirSync(path.join(root, "modules")).filter((id) => fs.existsSync(path.join(root, "modules", id, "module.toml"))
    && moduleDetails(id).runtime.player_actions.some((action) => action.id === "save_world"));
  assert.deepEqual(actual.sort(), Object.keys(expected).sort(), "Every module except the explicitly excluded Squad has its persistence entry");
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
  assert.match(maintenance, /id: "saves"[^]*?<ImmediateWorldSave details=\{props\.details\} moduleDetails=\{props\.moduleDetails\} readOnly=\{readOnly\}/);
});

test("unavailable saves explain stopped process, unloaded capabilities and missing remote configuration", () => {
  const details = instance();
  const module = moduleDetails("minecraft");
  assert.match(resolveWorldSaveAction(details, null, "en-US").unavailable, /Save capability is unavailable/);
  assert.match(resolveWorldSaveAction({ ...details, summary: { ...details.summary, status: "Stopped", active_process_count: 0 }, active_run: null }, module, "en-US").unavailable, /Start the server/);
  const remote = instance("rust"), remoteModule = moduleDetails("rust");
  for (const [settings, expectedError] of [[{ rcon_web: false }, /Enable WebSocket RCON/], [{ rcon_web: true, rcon_password: "" }, /administrator password/]]) {
    assert.match(resolveWorldSaveAction({ ...remote, settings_json: JSON.stringify(settings) }, remoteModule, "en-US").unavailable, expectedError);
  }
  assert.match(resolveWorldSaveAction({ ...remote, ports: [] }, remoteModule, "en-US").unavailable, /TCP port/);
  assert.match(resolveWorldSaveAction({ ...instance("humanitz"), ports: [], settings_json: "{}" }, moduleDetails("humanitz"), "en-US").unavailable, /Enable RCON/);
  assert.equal(resolveWorldSaveAction({ ...details, ports: [], settings_json: JSON.stringify({enable_rcon:false, rcon_password:""}) }, module, "en-US").unavailable, null,
    "Minecraft's native stdin command does not require a remote console");
  assert.match(resolveWorldSaveAction(instance("squad"), moduleDetails("squad"), "en-US").unavailable, /no recoverable persistent world save/);
  assert.match(resolveWorldSaveAction({ ...instance("theforest"), active_run: { run_id: 1, processes: [{ process_key: "other", status: "Running" }] } },
    moduleDetails("theforest"), "en-US").unavailable, /Start the server/);
});

test("save capability comes from module metadata without a game allowlist", () => {
  const module = moduleDetails("terraria");
  module.summary.id = "custom-game";
  assert.equal(resolveWorldSaveAction(instance("custom-game"), module, "en-US").unavailable, null);
  for (const invalid of [{ target_required: true }, { destructive: true }, { command_template: "" }]) {
    const unsafe = { ...module, runtime: { player_actions: [{ ...module.runtime.player_actions.find((entry) => entry.id === "save_world"), ...invalid }] } };
    assert.equal(resolveWorldSaveAction(instance("custom-game"), unsafe, "en-US").action, null);
  }
});

test("save actions need no separate preparation control", () => {
  for (const id of ["corekeeper", "vrising", "sonsoftheforest", "abioticfactor", "returntomoria", "windrose", "rimworld", "barotrauma", "nightingale", "conanexiles", "scum", "unturned", "minecraft", "romestead", "necesse", "projectzomboid", "terraria", "sevendaystodie", "arksurvivalevolved"]) {
    const state = harness();
    state.props.details = instance(id); state.props.moduleDetails = moduleDetails(id);
    const running = state.render();
    assert.doesNotMatch(running.html, /Prepare save extension/i);
    assert.equal(moduleDetails(id).runtime.player_actions.some((action) => action.id === "prepare_world_save"), false, id);
    assert.doesNotMatch(running.html, /before first use|manually install|loader manually/i);
    assert.equal(running.button.props.disabled, false, `${id}: saving must not require a separate preparation step`);
    assert.deepEqual(state.calls, [], `${id}: showing setup help must not send a save command`);
    state.unmount();
  }
});

test("WebSocket RCON saves reject disabled, uncredentialed or invalid TCP configuration", () => {
  for (const [id, enabledKey, passwordKey, protocol] of [["rust", "rcon_web", "rcon_password", "WebSocket RCON"]]) {
    const details = instance(id), module = moduleDetails(id), settings = JSON.parse(details.settings_json);
    assert.match(resolveWorldSaveAction({ ...details, settings_json: JSON.stringify({ ...settings, [enabledKey]: false }) }, module, "en-US").unavailable, new RegExp(`Enable ${protocol}`));
    assert.match(resolveWorldSaveAction({ ...details, settings_json: JSON.stringify({ ...settings, [passwordKey]: "" }) }, module, "en-US").unavailable, /administrator password/);
    assert.match(resolveWorldSaveAction({ ...details, ports: details.ports.map((port) => ({ ...port, protocol: "udp" })) }, module, "en-US").unavailable, /TCP port/);
  }
});

test("7 Days native Telnet saving preserves EAC and validates its own connection", () => {
  const details = instance("sevendaystodie"), module = moduleDetails("sevendaystodie");
  const settings = { eac_enabled: true, telnet_enabled: true, telnet_password: "fixture-server-password" };
  assert.equal(resolveWorldSaveAction({ ...details, settings_json: JSON.stringify(settings) }, module, "en-US").unavailable, null);
  assert.match(resolveWorldSaveAction({ ...details, settings_json: JSON.stringify({ ...settings, telnet_enabled: false }) }, module, "en-US").unavailable, /Enable Telnet/);
  assert.match(resolveWorldSaveAction({ ...details, settings_json: JSON.stringify({ ...settings, telnet_password: "" }) }, module, "en-US").unavailable, /administrator password/);
  assert.match(resolveWorldSaveAction({ ...details, ports: [], settings_json: JSON.stringify(settings) }, module, "en-US").unavailable, /TCP port/);
});

test("Palworld native REST saving requires its declared credentials and TCP port", () => {
  const details = { ...instance("palworld"), ports: [],
    settings_json: JSON.stringify({ rest_api_enabled: false, admin_password: "" }) };
  const module = moduleDetails("palworld");
  assert.match(resolveWorldSaveAction(details, module, "en-US").unavailable, /Enable REST API/);
  assert.equal(resolveWorldSaveAction(instance("palworld"), module, "en-US").unavailable, null);
});

test("DST immediate saving requires every enabled shard and excludes disabled shards", () => {
  const module = moduleDetails("dontstarve");
  const details = instance("dontstarve");
  details.settings_json = JSON.stringify({ shard_layout: "island_adventures" });
  details.active_run.processes = ["master", "caves", "islands", "volcano"].map((process_key) => ({ process_key, status: "Running" }));
  assert.equal(resolveWorldSaveAction(details, module, "en-US").unavailable, null);
  for (const shard of ["master", "caves", "islands", "volcano"]) {
    const partial = { ...details, active_run: { ...details.active_run, processes: details.active_run.processes.filter((process) => process.process_key !== shard) } };
    assert.ok(resolveWorldSaveAction(partial, module, "en-US").unavailable, shard);
  }
  details.settings_json = JSON.stringify({ enable_caves: false });
  details.active_run.processes = [{ process_key: "master", status: "Running" }];
  assert.equal(resolveWorldSaveAction(details, module, "en-US").unavailable, null);
});

test("ASTRONEER native saving requires its TCP console password and port", () => {
  const details = { ...instance("astroneer"), ports: [], settings_json: JSON.stringify({ console_password: "" }) };
  const module = moduleDetails("astroneer");
  assert.match(resolveWorldSaveAction(details, module, "en-US").unavailable, /console administrator password/);
  assert.match(resolveWorldSaveAction({ ...instance("astroneer"), ports: [] }, module, "en-US").unavailable, /console TCP port/);
  assert.equal(resolveWorldSaveAction(instance("astroneer"), module, "en-US").unavailable, null);
});

test("scoped native persistence is visible and never promises a complete world snapshot", () => {
  for (const [id, description] of [["rimworld", /active clients.*Offline clients cannot be saved/],
    ["barotrauma", /lobby between rounds.*Ongoing rounds cannot be saved manually/],
    ["scum", /global state.*transform changes.*registered persistent entities/]]) {
    const state = harness(); state.props.details = instance(id); state.props.moduleDetails = moduleDetails(id);
    assert.equal(resolveWorldSaveAction(state.props.details, state.props.moduleDetails, "en-US").unavailable, null);
    assert.match(worldSaveDescription(id, "en-US"), description);
    assert.match(state.render().html, description, `${id} scope must be visible without opening help`);
    state.unmount();
  }
  assert.equal(worldSaveTitle("scum", "en-US"), "Save persistent state");
  assert.equal(worldSaveTitle("scum", "zh-CN"), "保存服务器状态");
  assert.equal(resolveWorldSaveAction(instance("squad"), moduleDetails("squad"), "en-US").action, null);
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
    assert.match(state.render().html, /Save completed/);
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

test("world-save missing completion and errors fail verification without retry", async () => {
  const pending = harness(async () => response({ write_confirmation_pending: true, save_completion: "unconfirmed", response_text: "native writer pending" }));
  pending.render().button.props.onClick(); await settle();
  assert.match(pending.render().html, /role="alert"[^]*Save outcome unknown/);
  assert.doesNotMatch(pending.render().html, /command was accepted/);
  assert.equal(pending.calls.length, 1); pending.unmount();
  const failed = harness(async () => { throw new Error("native save connection lost"); });
  failed.render().button.props.onClick(); await settle();
  assert.match(failed.render().html, /role="alert"[^]*native save connection lost/);
  assert.equal(failed.calls.length, 1); failed.unmount();
});

test("requested saving succeeds without claiming native completion", async () => {
  const state = harness(async () => response({ save_completion: "requested", response_text: "Native command processed" }));
  state.render().button.props.onClick(); await settle();
  const view = state.render();
  assert.match(view.html, /Save request sent\. The game will perform the save\./);
  assert.match(view.html, /Native command processed/);
  assert.doesNotMatch(view.html, /role="alert"|Save completed|Save outcome unknown/);
  assert.equal(view.button.props.disabled, false);
  assert.equal(state.calls.length, 1); state.unmount();
});

test("confirmed transport delivery and missing save status cannot claim native save completion", async () => {
  for (const completion of [undefined, null, "unconfirmed"]) {
    const state = harness(async () => response({ write_confirmation_pending: false, save_completion: completion, response_text: "Command sent" }));
    state.render().button.props.onClick(); await settle();
    assert.match(state.render().html, /role="alert"[^]*Save outcome unknown/);
    assert.doesNotMatch(state.render().html, /Save completed/);
    assert.equal(state.calls.length, 1); state.unmount();
  }
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
