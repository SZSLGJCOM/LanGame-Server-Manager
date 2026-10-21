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
const settle = () => new Promise((resolve) => setImmediate(resolve));
const deferred = () => { let resolve; const promise = new Promise((yes) => { resolve = yes; }); return { promise, resolve }; };
const descendants = (node) => React.isValidElement(node) ? [node, ...React.Children.toArray(node.props.children).flatMap(descendants)] : [];

function moduleDetails(id) {
  const manifest = fs.readFileSync(path.resolve(__dirname, "../../../modules", id, "module.toml"), "utf8");
  const block = manifest.split("[[runtime.player_actions]]").find((entry) => /^id = "save_world"$/m.test(entry));
  const action = block ? Object.fromEntries([...block.split(/\r?\n\[/)[0].matchAll(/^([a-z_]+) = "([^"]*)"$/gm)].map((match) => [match[1], match[2]])) : null;
  return { summary: { id }, runtime: { player_actions: action ? [action] : [] } };
}

function harness(overrides = {}, moduleId = "dontstarve") {
  const hooks = [], effects = [], calls = [], notices = [];
  let cursor = 0;
  const react = { ...React,
    useRef(initial) { const i = cursor++; hooks[i] ??= { current: initial }; return hooks[i]; },
    useState(initial) { const i = cursor++; hooks[i] ??= { value: initial }; return [hooks[i].value, (value) => { hooks[i].value = value; }]; },
    useEffect(create) { const i = cursor++; if (!hooks[i]) { hooks[i] = {}; effects.push(() => { hooks[i].cleanup = create(); }); } }
  };
  const props = { instance: { id: "dst-card", name: "DST", module_id: moduleId, status: "Running", active_process_count: 4 }, supported: true, onActivity: (value) => notices.push(value) };
  const details = { summary: props.instance,
    settings_json: JSON.stringify({ shard_layout: "island_adventures", enable_rcon: true, rcon_enabled: true, rcon_web: true,
      rcon_password: "fixture-server-password", telnet_enabled: true, telnet_password: "fixture-server-password", rest_api_enabled: true, admin_password: "fixture-admin-password", console_password: "fixture-server-password" }),
    ports: [{ name: "rcon", port: 25575, protocol: "tcp" }, { name: "telnet", port: 8081, protocol: "tcp" }, { name: "rest_api", port: 8212, protocol: "tcp" }, { name: "console", port: 8778, protocol: "tcp" }],
    active_run: { run_id: 1, processes: (moduleId === "dontstarve" ? ["master", "caves", "islands", "volcano"] : ["main"]).map((process_key) => ({ process_key, status: "Running" })) } };
  const api = {
    readInstanceDetails: async () => details,
    readModuleDetails: async () => moduleDetails(moduleId),
    sendInstanceRuntimeCommand: async (...args) => { calls.push(args); return { response_text: "All enabled shards confirmed", write_confirmation_pending: false, save_completion: "confirmed" }; },
    ...overrides
  };
  const filename = path.resolve(__dirname, "../src/views/servers/ServerCardWorldSave.tsx");
  const loaded = new Module(filename, module); loaded.filename = filename;
  const fromFile = Module.createRequire(filename);
  loaded.require = (name) => {
    if (name === "react") return react;
    if (name === "../../api") return api;
    if (name === "../../i18n") return { useI18n: () => ({ locale: "en-US" }), selectLocaleText: (_, _zh, en) => en };
    if (name === "../../app-state") return { describeError: (error) => error.message };
    if (name === "../../components/ShellIcon") return { ShellIcon: () => React.createElement("svg") };
    return fromFile(name);
  };
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  return { props, details, calls, notices,
    render() { cursor = 0; const root = loaded.exports.ServerCardWorldSave(props); while (effects.length) effects.shift()();
      return { html: renderToStaticMarkup(root), button: descendants(root).find((node) => node.type === "button") }; },
    unmount() { for (const hook of hooks) hook?.cleanup?.(); }
  };
}

test("card save is a dedicated icon and uses the same declared save action", async () => {
  const state = harness(); const view = state.render();
  assert.match(view.button.props.className, /server-list-card-save-action/);
  assert.doesNotMatch(view.button.props.className, /server-list-card-copy-action/);
  view.button.props.onClick(); await settle();
  assert.deepEqual(state.calls, [["dst-card", "c_save()", "master", { runtimeActionId: "save_world" }]]);
  assert.match(state.notices[0].params.message, /All enabled shards confirmed/);
  state.unmount();
});

test("card save hides unsupported games, blocks stopped or busy instances, and validates all DST shards", async () => {
  const state = harness();
  state.props.supported = false; assert.equal(state.render().button, undefined);
  state.props.supported = true;
  state.props.instance = { ...state.details.summary, status: "Stopped", active_process_count: 0 }; assert.equal(state.render().button.props.disabled, true);
  state.props.instance = state.details.summary; state.props.disabled = true; assert.equal(state.render().button.props.disabled, true);
  state.props.disabled = false; state.details.active_run.processes.pop();
  state.render().button.props.onClick(); await settle();
  assert.equal(state.calls.length, 0); assert.match(state.notices[0].params.message, /volcano/);
  state.unmount();
});

test("every connected game card sends its own declared save command", async () => {
  const expected = { minecraft: "save-all flush", projectzomboid: "save", terraria: "save", palworld: "save", unturned: "Save",
    arksurvivalevolved: "saveworld", arksurvivalascended: "saveworld", sevendaystodie: "saveworld", necesse: "save",
    rust: "server.save", romestead: "save", theforest: "save", humanitz: "save", satisfactory: "save", astroneer: "save", valheim: "save", runescapedragonwilds: "save", corekeeper: "save", vrising: "save", sonsoftheforest: "save", abioticfactor: "save", returntomoria: "save", windrose: "save",
    enshrouded: "save", rimworld: "save", barotrauma: "save", soulmask: "SaveWorld 1", nightingale: "save", conanexiles: "save", scum: "save_persistent_state" };
  for (const [id, command] of Object.entries(expected)) {
    const state = harness({}, id);
    const button = state.render().button;
    assert.equal(button.props.disabled, false, id);
    button.props.onClick(); await settle();
    assert.equal(state.calls.length, 1, id);
    assert.equal(state.calls[0][1], command, id);
    assert.deepEqual(state.calls[0][3], { runtimeActionId: "save_world" });
    assert.equal(state.notices[0]?.tone, undefined, id);
    state.unmount();
  }
});

test("scoped card actions expose their persistence boundaries in their accessible descriptions", () => {
  for (const [id, scope] of [["rimworld", /active clients.*Offline clients cannot be saved/],
    ["barotrauma", /lobby between rounds.*does not end a round/],
    ["scum", /global state.*transform changes.*registered persistent entities/]]) {
    const state = harness({}, id); const button = state.render().button;
    assert.match(button.props.title, scope);
    if (id === "scum") assert.match(button.props["aria-label"], /^Save persistent state:/);
    state.unmount();
  }
});

test("card visibility never replaces current capability validation before dispatch", async () => {
  const state = harness({ readModuleDetails: async () => moduleDetails("squad") });
  state.render().button.props.onClick(); await settle();
  assert.equal(state.calls.length, 0);
  assert.equal(state.notices[0].tone, "error");
  state.unmount();
});

test("card distinguishes requested saving, native completion and unknown results", async () => {
  for (const completion of [undefined, "unconfirmed", "requested", "confirmed"]) {
    const state = harness({ sendInstanceRuntimeCommand: async () => ({ write_confirmation_pending: false, save_completion: completion }) });
    state.render().button.props.onClick(); await settle();
    const accepted = completion === "confirmed" || completion === "requested";
    assert.match(state.notices[0].params.message, completion === "confirmed" ? /Save completed/
      : completion === "requested" ? /Save request sent\. The game will perform the save\./ : /Save outcome unknown/);
    assert.equal(state.notices[0].tone, accepted ? undefined : "error");
    if (completion === "requested") assert.doesNotMatch(state.render().html, /is-error|role="alert"|Save completed/);
    state.unmount();
  }
});

test("card save neither duplicates an in-flight command nor displays a late unmounted result", async () => {
  const wait = deferred(); let sends = 0;
  const state = harness({ sendInstanceRuntimeCommand: async () => { sends++; return wait.promise; } });
  const button = state.render().button; button.props.onClick(); button.props.onClick(); await settle();
  assert.equal(sends, 1); assert.equal(state.render().button.props.disabled, true);
  state.unmount(); wait.resolve({ response_text: "Late save", write_confirmation_pending: false }); await settle();
  assert.deepEqual(state.notices, []);
});
