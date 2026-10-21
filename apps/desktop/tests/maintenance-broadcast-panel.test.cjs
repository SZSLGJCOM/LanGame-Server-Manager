const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
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
const settle = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function descendants(node) {
  if (!React.isValidElement(node)) return [];
  return [node, ...React.Children.toArray(node.props.children).flatMap(descendants)];
}
function policy(instanceId) {
  return { instance_id: instanceId, enabled: true, updated_at_unix_ms: 1, rules: {
    startup: { enabled: false, prompt: null }, shutdown: { enabled: true, prompt: "Keep shutdown" },
    runtime_health: { enabled: true, prompt: "Keep health" },
    periodic: { enabled: true, interval_minutes: 40, prompt: "Keep periodic" }, tone: "short", cooldown_minutes: 10
  } };
}
function dstModuleDetails() {
  const result = spawnSync("python", ["-B", "-c", [
    "import json, pathlib, tomllib",
    "manifest = tomllib.loads(pathlib.Path('modules/dontstarve/module.toml').read_text(encoding='utf-8-sig'))",
    "print(json.dumps({'summary': {'id': manifest['id']}, 'runtime': manifest['runtime']}))"
  ].join("; ")], { cwd: path.resolve(__dirname, "../../.."), encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  return JSON.parse(result.stdout);
}

test("broadcast history shows pending confirmation without claiming success", async () => {
  const state = harness({ listInstanceBroadcastEvents: async () => [{
    event_id: "pending-fixture", instance_id: "fixture", source: "manual",
    message: "One submission", status: "pending", created_at_unix_ms: 1
  }] });
  state.render(); await settle();
  const view = state.render();
  assert.match(view.html, /server-broadcast-event-status is-warning/);
  assert.match(view.html, /servers.broadcast.status.pending/);
  assert.match(view.html, /servers.broadcast.pendingNotice/);
  assert.doesNotMatch(view.html, /server-broadcast-event-status is-success/);
  assert.doesNotMatch(view.html, /servers.broadcast.status.sent/);
  assert.equal(state.calls.filter((call) => call.name === "sendInstanceBroadcast").length, 0);
  state.unmount();
});
let sequence = 0;
function harness(overrides = {}) {
  const hooks = [], effects = [], calls = [];
  const timers = new Map();
  let timerId = 0;
  let cursor = 0, writes = 0;
  const same = (left, right) => left?.length === right?.length && left.every((value, index) => Object.is(value, right[index]));
  const react = {
    ...React,
    useState(initial) {
      const slot = cursor++;
      if (!(slot in hooks)) hooks[slot] = { value: typeof initial === "function" ? initial() : initial };
      return [hooks[slot].value, (next) => {
        const value = typeof next === "function" ? next(hooks[slot].value) : next;
        if (!Object.is(value, hooks[slot].value)) writes++;
        hooks[slot].value = value;
      }];
    },
    useMemo(create, deps) {
      const slot = cursor++;
      if (!same(hooks[slot]?.deps, deps)) hooks[slot] = { deps, value: create() };
      return hooks[slot].value;
    },
    useCallback(callback, deps) { return react.useMemo(() => callback, deps); },
    useRef(value) {
      const slot = cursor++;
      if (!(slot in hooks)) hooks[slot] = { current: value };
      return hooks[slot];
    },
    useEffect(create, deps) {
      const slot = cursor++;
      if (same(hooks[slot]?.deps, deps)) return;
      const previous = hooks[slot];
      hooks[slot] = { deps };
      effects.push(() => { previous?.cleanup?.(); hooks[slot].cleanup = create(); });
    }
  };
  const actions = {
    readInstanceBroadcastPolicy: async (id) => policy(id),
    listInstanceBroadcastEvents: async () => [],
    updateInstanceBroadcastPolicy: async (input) => ({ ...input, updated_at_unix_ms: 2 }),
    generateInstanceBroadcast: async () => ({ message: "Generated message", provider: "local", model: "fixture-model" }),
    sendInstanceBroadcast: async () => ({}),
    ...overrides
  };
  const api = Object.fromEntries(Object.entries(actions).map(([name, action]) => [name, (...args) => {
    calls.push({ name, args });
    return action(...args);
  }]));
  const filename = path.resolve(__dirname, "../src/views/servers/AiBroadcastWorkbench.tsx");
  const loaded = new Module(filename, module);
  loaded.filename = filename;
  loaded.broadcastTimers = {
    setTimeout(callback) { timers.set(++timerId, callback); return timerId; },
    clearTimeout(id) { timers.delete(id); }
  };
  const requireFromFile = Module.createRequire(filename);
  loaded.require = (name) => {
    if (name === "react") return react;
    if (name === "../../api") return api;
    if (name === "../../i18n") return { useI18n: () => ({ locale: "en-US", t: (key) => key }) };
    if (name === "../../app-state") return { describeError: (error) => error.message ?? String(error) };
    if (name === "../../view-models") return { formatTime: () => "10:00" };
    if (name === "../../components/ShellIcon") return { ShellIcon: () => React.createElement("svg") };
    return requireFromFile(name);
  };
  loaded._compile("const { setTimeout, clearTimeout } = module.broadcastTimers;\n"
    + transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  const props = { active: true, assistantCanRun: false, aiSettings: { provider: "local", model: "fixture-model", baseUrl: "", apiKey: "" },
    details: { summary: { id: `broadcast-fixture-${++sequence}`, module_id: "dontstarve", status: "Running", active_process_count: 1 }, active_run: null },
    moduleDetails: { summary: { id: "dontstarve" }, runtime: { player_actions: [{ kind: "broadcast", command_template: "private-template" }] } }, runtime: null };
  function render() {
    let root;
    for (let attempt = 0; attempt < 10; attempt++) {
      cursor = 0;
      const previous = writes;
      root = loaded.exports.AiBroadcastWorkbench(props);
      while (effects.length) effects.shift()();
      if (writes === previous) break;
      assert.ok(attempt < 9, "broadcast effects must settle");
    }
    const nodes = descendants(root);
    return { root, nodes, html: renderToStaticMarkup(root),
      control: (name) => nodes.find((node) => node.props.name === name),
      button: (label) => nodes.find((node) => node.type === "button" && React.Children.toArray(node.props.children).includes(label)) };
  }
  return { props, calls, render,
    poll() {
      const timer = timers.entries().next().value;
      assert.ok(timer, "pending history schedules a refresh");
      timers.delete(timer[0]);
      timer[1]();
    },
    change(name, value) {
      const control = render().control(name);
      assert.ok(control, `Missing control ${name}`);
      control.props.onChange({ target: { value, checked: value } });
    },
    unmount() { for (const hook of hooks) hook?.cleanup?.(); }
  };
}

test("a late same-instance poll cannot erase a newer pending broadcast", async () => {
  const oldRead = deferred();
  const first = { event_id: "first", source: "manual", message: "First", status: "pending", created_at_unix_ms: 1 };
  const second = { ...first, event_id: "second", message: "Second pending", created_at_unix_ms: 2 };
  let reads = 0;
  const state = harness({ listInstanceBroadcastEvents: async () => {
    reads++;
    if (reads === 1) return [first];
    if (reads === 2) return oldRead.promise;
    return [second, { ...first, status: "sent" }];
  } });
  state.render(); await settle(); state.render();
  state.poll(); await settle();
  state.change("broadcast-message", "Second pending");
  state.render().button("servers.broadcast.send").props.onClick(); await settle();
  assert.match(state.render().html, /Second pending/);
  oldRead.resolve([{ ...first, status: "sent" }]); await settle();
  const view = state.render();
  assert.match(view.html, /Second pending/);
  assert.match(view.html, /servers.broadcast.status.pending/);
  state.poll(); await settle();
  assert.equal(reads, 4, "new pending entry keeps polling after the stale response");
  assert.equal(state.calls.filter((call) => call.name === "sendInstanceBroadcast").length, 1);
  state.unmount();
});

test("broadcast panel keeps all sections visible and sends handwritten messages without AI", async () => {
  const state = harness();
  state.render(); await settle();
  let view = state.render();
  assert.match(view.root.props.className, /server-maintenance-card server-broadcast-panel/);
  assert.equal(view.nodes.some((node) => ["details", "summary"].includes(node.type)), false);
  assert.equal(view.nodes.filter((node) => node.type === "textarea").length, 4);
  assert.equal(view.control("broadcast-tone").type, "select");
  assert.match(view.html, /servers.broadcast.historyTitle/);
  assert.doesNotMatch(view.html, /private-template/);
  assert.equal(view.button("servers.broadcast.generate").props.disabled, true);
  state.change("broadcast-message", "Handwritten maintenance reminder");
  view = state.render();
  assert.equal(view.button("servers.broadcast.send").props.disabled, false);
  view.button("servers.broadcast.send").props.onClick(); await settle();
  const sent = state.calls.find((call) => call.name === "sendInstanceBroadcast").args[0];
  assert.equal(sent.instanceId, state.props.details.summary.id);
  assert.equal(sent.message, "Handwritten maintenance reminder");
  assert.equal(sent.aiModel, null);
  assert.equal(state.render().control("broadcast-message").props.value, "");
  state.unmount();
});

test("unsupported or unknown broadcast capabilities show an accurate notice without editors or reads", async () => {
  for (const [moduleDetails, key] of [
    [{ summary: { id: "dontstarve" }, runtime: { player_actions: [] } }, "servers.broadcast.unsupported"],
    [null, "servers.broadcast.capabilityUnavailable"],
    [{ summary: { id: "other-game" }, runtime: { player_actions: [{ kind: "broadcast" }] } }, "servers.broadcast.capabilityUnavailable"]
  ]) {
    const state = harness(); state.props.moduleDetails = moduleDetails;
    const view = state.render(); await settle();
    assert.match(view.root.props.className, /server-broadcast-panel--unavailable/);
    assert.ok(view.html.includes(key));
    assert.equal(view.nodes.some((node) => ["input", "select", "textarea", "button"].includes(node.type)), false);
    assert.equal(state.calls.length, 0);
    state.unmount();
  }
});

test("compact automatic rules preserve full policy values and zero cooldown", async () => {
  const state = harness({ readInstanceBroadcastPolicy: async (id) => {
    const current = policy(id);
    return { ...current, rules: { ...current.rules,
      startup: { enabled: false, prompt: "Existing welcome\nExisting instructions" },
      shutdown: { enabled: true, prompt: "Keep shutdown\nKeep second line" } } };
  } });
  state.render(); await settle();
  const loaded = state.render();
  for (const name of ["broadcast-startup-prompt", "broadcast-shutdown-prompt"]) {
    assert.equal(loaded.control(name).type, "textarea", "rule prompts must support persisted multiline values");
    assert.equal(loaded.control(name).props.rows, 1);
  }
  assert.equal(loaded.control("broadcast-startup-prompt").props.value, "Existing welcome\nExisting instructions");
  assert.equal(loaded.control("broadcast-shutdown-prompt").props.value, "Keep shutdown\nKeep second line");
  state.change("broadcast-tone", "friendly"); await settle();
  state.change("broadcast-cooldown", "0"); await settle();
  state.change("broadcast-startup-enabled", true); await settle();
  state.change("broadcast-startup-prompt", "Welcome players\nPlease read the server rules"); await settle();
  const updates = state.calls.filter((call) => call.name === "updateInstanceBroadcastPolicy");
  const saved = updates.at(-1).args[0];
  assert.equal(saved.rules.tone, "friendly");
  assert.equal(saved.rules.cooldown_minutes, 0);
  assert.deepEqual(saved.rules.startup, { enabled: true, prompt: "Welcome players\nPlease read the server rules" });
  assert.deepEqual(saved.rules.shutdown, { enabled: true, prompt: "Keep shutdown\nKeep second line" });
  assert.deepEqual(saved.rules.runtime_health, { enabled: true, prompt: "Keep health" });
  assert.deepEqual(saved.rules.periodic, { enabled: true, interval_minutes: 40, prompt: "Keep periodic" });
  state.unmount();
});

test("failed sends retain the message and visible error until a successful retry", async () => {
  let fail = true;
  const state = harness({ sendInstanceBroadcast: async () => { if (fail) throw new Error("Connection rejected"); } });
  state.render(); await settle();
  state.change("broadcast-message", "Keep this message");
  state.render().button("servers.broadcast.send").props.onClick(); await settle();
  const errorNotice = state.render().nodes.find((node) => node.props.tone === "error");
  assert.equal(errorNotice?.props.children, "Connection rejected");
  const errorMarkup = renderToStaticMarkup(errorNotice);
  assert.match(errorMarkup, /role="alert"/);
  assert.equal(errorMarkup.replace(/<[^>]*>/g, ""), "Connection rejected");
  assert.equal(state.render().control("broadcast-message").props.value, "Keep this message");
  fail = false;
  state.render().button("servers.broadcast.send").props.onClick(); await settle();
  assert.doesNotMatch(state.render().html, /Connection rejected/);
  assert.equal(state.render().control("broadcast-message").props.value, "");
  state.unmount();
});

test("AI generation reaches the message field and never overwrites another instance", async () => {
  const pending = deferred(); let wait = false;
  const state = harness({ generateInstanceBroadcast: async () => wait ? pending.promise
    : { message: "Generated announcement", provider: "local", model: "fixture-model" } });
  state.props.assistantCanRun = true;
  state.render(); await settle();
  state.change("broadcast-intent", "Explain the restart");
  state.render().button("servers.broadcast.generate").props.onClick(); await settle();
  assert.equal(state.render().control("broadcast-message").props.value, "Generated announcement");
  wait = true;
  state.render().button("servers.broadcast.generate").props.onClick(); await settle();
  state.props.details = { ...state.props.details, summary: { ...state.props.details.summary, id: "other-instance" } };
  state.render(); await settle();
  pending.resolve({ message: "Late old announcement", provider: "local", model: "fixture-model" }); await settle();
  assert.equal(state.render().control("broadcast-message").props.value, "");
  state.unmount();
});

test("history keeps actual event results and causes visible while stopped servers cannot send", async () => {
  const state = harness({ listInstanceBroadcastEvents: async () => [{ event_id: "failed-1", status: "Failed", source: "manual",
    initiator: "manual", message: "Restart in ten minutes", ai_model: "fixture-model", created_at_unix_ms: 1,
    error_message: "Game process is offline" }] });
  state.props.details.summary.status = "Stopped"; state.props.details.summary.active_process_count = 0;
  state.render(); await settle(); state.change("broadcast-message", "Do not send");
  const view = state.render();
  assert.match(view.html, /Restart in ten minutes/);
  assert.match(view.html, /Game process is offline/);
  assert.equal(view.nodes.filter((node) => node.type === "article").length, 1);
  assert.equal(view.button("servers.broadcast.send").props.disabled, true);
  state.unmount();
});

test("bundled DST broadcast uses the shared panel and preserves text until dispatch completes", async () => {
  const pending = deferred();
  let fail = false;
  const state = harness({ sendInstanceBroadcast: async () => {
    if (fail) throw new Error("Master command channel is unavailable");
    return pending.promise;
  } });
  state.props.moduleDetails = dstModuleDetails();
  const action = state.props.moduleDetails.runtime.player_actions.find((entry) => entry.kind === "broadcast");
  assert.ok(action, "the bundled DST manifest must declare its broadcast capability");
  assert.equal(action.transport, "stdin");
  assert.equal(action.process_key, "master");
  assert.equal(action.command_template, "c_announce({{target}})");
  assert.equal(action.target_encoding, "quoted_string", "Lua text must use the shared quoted-string encoder");
  state.props.details.active_run = { processes: [{ process_key: "master", status: "Running" }] };
  state.render(); await settle();
  const message = '集合到 "东门" \\ 营地';
  state.change("broadcast-message", message);
  let view = state.render();
  assert.equal(view.button("servers.broadcast.send").props.disabled, false);
  view.button("servers.broadcast.send").props.onClick();
  view = state.render();
  assert.equal(view.button("common.sending").props.disabled, true);
  assert.equal(view.control("broadcast-message").props.value, message);
  assert.equal(state.calls.filter((call) => call.name === "listInstanceBroadcastEvents").length, 1,
    "the panel must wait for dispatch completion before refreshing history");
  const sent = state.calls.find((call) => call.name === "sendInstanceBroadcast").args[0];
  assert.equal(sent.message, message, "the UI must send message data without constructing Lua");
  assert.equal(sent.source, "manual");
  assert.equal(sent.aiModel, null);
  pending.resolve({}); await settle();
  assert.equal(state.render().control("broadcast-message").props.value, "");
  assert.equal(state.calls.filter((call) => call.name === "listInstanceBroadcastEvents").length, 2);

  fail = true;
  state.change("broadcast-message", message);
  state.render().button("servers.broadcast.send").props.onClick(); await settle();
  assert.equal(state.render().control("broadcast-message").props.value, message);
  assert.equal(state.render().nodes.find((node) => node.props.tone === "error")?.props.children,
    "Master command channel is unavailable");
  state.unmount();
});

test("bundled DST broadcast requires Master even when other shards remain running", async () => {
  for (const masterStatus of [null, "Stopped", "Starting", "Failed"]) {
    const state = harness();
    state.props.moduleDetails = dstModuleDetails();
    state.props.details.active_run = { processes: [
      ...(masterStatus ? [{ process_key: "master", status: masterStatus }] : []),
      ...["caves", "islands", "volcano"].map((process_key) => ({ process_key, status: "Running" }))
    ] };
    state.render(); await settle();
    state.change("broadcast-message", "Wait for Master");
    const button = state.render().button("servers.broadcast.send");
    assert.equal(button.props.disabled, true, `Master status ${masterStatus} must block dispatch`);
    button.props.onClick(); await settle();
    assert.equal(state.calls.some((call) => call.name === "sendInstanceBroadcast"), false);
    state.unmount();
  }
});
