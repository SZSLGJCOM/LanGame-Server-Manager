const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".tsx"] = require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const settle = () => new Promise((resolve) => setImmediate(resolve));
const domainFile = path.resolve(__dirname, "../src/maintenance-schedule.ts");
const domain = new Module(domainFile, module);
domain.filename = domainFile;
domain.require = (id) => {
  assert.equal(id, "./api-transport");
  return { invokeOrMock: () => { throw new Error("Unexpected native invocation in pure policy test"); } };
};
domain._compile(transpileTypeScript(fs.readFileSync(domainFile, "utf8"), domainFile), domainFile);
function deferred() { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
function policy(id = "server-a", action = "backup", overrides = {}) {
  return { instance_id: id, action, enabled: false, time_local: "04:00", revision: 0, last_run: null, ...overrides };
}
function descendants(node) {
  return React.isValidElement(node) ? [node, ...React.Children.toArray([node.props.children, node.props.action]).flatMap(descendants)] : [];
}
function harness(overrides = {}) {
  const hooks = [], effects = [], calls = [], timers = new Map();
  let cursor = 0, writes = 0, timerId = 0;
  const equal = (a, b) => a?.length === b?.length && a.every((v, i) => Object.is(v, b[i]));
  const react = { ...React,
    useState(initial) { const slot = cursor++; if (!(slot in hooks)) hooks[slot] = { value: typeof initial === "function" ? initial() : initial };
      return [hooks[slot].value, (next) => { hooks[slot].value = typeof next === "function" ? next(hooks[slot].value) : next; writes++; }]; },
    useRef(initial) { const slot = cursor++; return hooks[slot] ??= { current: initial }; },
    useMemo(create, deps) { const slot = cursor++; if (!equal(hooks[slot]?.deps, deps)) hooks[slot] = { value: create(), deps }; return hooks[slot].value; },
    useEffect(create, deps) { const slot = cursor++; if (equal(hooks[slot]?.deps, deps)) return; const old = hooks[slot]; hooks[slot] = { deps };
      effects.push(() => { old?.cleanup?.(); hooks[slot].cleanup = create(); }); }
  };
  const api = {
    validDailyTime: domain.exports.validDailyTime,
    readDailyMaintenanceSchedule: async (id, action) => policy(id, action),
    updateDailyMaintenanceSchedule: async (input) => policy(input.instanceId, input.action, {
      enabled: input.enabled, time_local: input.timeLocal, revision: input.expectedRevision + 1 }), ...overrides
  };
  const filename = path.resolve(__dirname, "../src/views/servers/DailyMaintenanceEditor.tsx");
  const loaded = new Module(filename, module); loaded.filename = filename;
  const nativeRequire = Module.createRequire(filename);
  loaded.require = (id) => id === "react" ? react : id === "../../maintenance-schedule" ? new Proxy(api, {
    get(target, name) { return name === "validDailyTime" ? target[name] : (...args) => { calls.push({ name, args }); return target[name](...args); }; }
  }) : nativeRequire(id);
  loaded.fixtureSetInterval = (fn) => { const id = ++timerId; timers.set(id, fn); return id; };
  loaded.fixtureClearInterval = (id) => timers.delete(id);
  loaded._compile("const setInterval = module.fixtureSetInterval; const clearInterval = module.fixtureClearInterval;\n"
    + transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  const props = { instanceId: "server-a", action: "backup", t: (key, _args, fallback) => fallback ?? key };
  function render() {
    let root;
    for (let i = 0; i < 10; i++) { cursor = 0; const before = writes; root = loaded.exports.DailyMaintenanceEditor(props);
      while (effects.length) effects.shift()(); if (writes === before) break; }
    const nodes = descendants(root);
    return { nodes, html: renderToStaticMarkup(root), control: (name) => nodes.find((n) => n.props.name === name) };
  }
  return { props, calls, render, poll: () => [...timers.values()].forEach((fn) => fn()),
    unmount: () => hooks.forEach((hook) => hook?.cleanup?.()) };
}

test("daily maintenance is disabled by default and saves only its own policy with a revision", async () => {
  const state = harness();
  assert.equal(state.render().control("daily-backup-enabled").props.disabled, true);
  await settle();
  const view = state.render();
  assert.equal(view.control("daily-backup-enabled").props.checked, false);
  assert.equal(view.control("daily-backup-time").props.value, "04:00");
  view.control("daily-backup-enabled").props.onChange({ target: { checked: true } });
  await settle();
  assert.deepEqual(state.calls.find((c) => c.name === "updateDailyMaintenanceSchedule").args[0], {
    instanceId: "server-a", action: "backup", enabled: true, timeLocal: "04:00", expectedRevision: 0
  });
  assert.equal(state.render().control("daily-backup-enabled").props.checked, true);
  state.unmount();
});

test("invalid time never submits and stopped/other-game policies do not need a running-game shortcut", async () => {
  const state = harness(); state.props.action = "restart";
  state.render(); await settle();
  state.render().control("daily-restart-time").props.onChange({ target: { value: "24:00" } });
  const invalid = state.render(); invalid.control("daily-restart-time").props.onBlur(); await settle();
  assert.match(state.render().html, /servers.dailyMaintenance.invalidTime/);
  assert.equal(state.calls.some((call) => call.name === "updateDailyMaintenanceSchedule"), false);
  state.unmount();
});

test("late same-instance read and old-instance saves cannot replace current controls", async () => {
  const read = deferred(), write = deferred(); let reads = 0;
  const state = harness({ readDailyMaintenanceSchedule: async (id, action) => ++reads === 2 ? read.promise : policy(id, action),
    updateDailyMaintenanceSchedule: async () => write.promise });
  state.render(); await settle(); state.render(); state.poll();
  state.render().control("daily-backup-enabled").props.onChange({ target: { checked: true } });
  write.resolve(policy("server-a", "backup", { enabled: true, revision: 1 })); await settle();
  read.resolve(policy()); await settle();
  assert.equal(state.render().control("daily-backup-enabled").props.checked, true);
  const staleToggle = state.render().control("daily-backup-enabled");
  state.props.instanceId = "server-b"; state.render(); await settle();
  staleToggle.props.onChange({ target: { checked: false } }); await settle();
  assert.equal(state.calls.filter((c) => c.name === "updateDailyMaintenanceSchedule").length, 1);
  assert.equal(state.render().control("daily-backup-enabled").props.checked, false);
  state.unmount();
});

test("polling retains the revision a dirty edit began from so concurrent writes conflict", async () => {
  let reads = 0;
  const state = harness({ readDailyMaintenanceSchedule: async () => policy("server-a", "backup", {
    revision: reads++ === 0 ? 0 : 9, time_local: "04:00" }) });
  state.render(); await settle();
  state.render().control("daily-backup-time").props.onChange({ target: { value: "05:30" } });
  state.render(); state.poll(); await settle();
  const draft = state.render(); assert.equal(draft.control("daily-backup-time").props.value, "05:30");
  draft.control("daily-backup-time").props.onBlur(); await settle();
  assert.equal(state.calls.find((c) => c.name === "updateDailyMaintenanceSchedule").args[0].expectedRevision, 0);
  state.unmount();
});

test("archived maintenance cannot read or write active schedules", async () => {
  const state = harness(); state.props.readOnly = true;
  const view = state.render();
  assert.equal(view.control("daily-backup-enabled").props.disabled, true);
  view.control("daily-backup-enabled").props.onChange({ target: { checked: true } });
  await settle(); assert.deepEqual(state.calls, []);
  state.unmount();
});

test("an old live control cannot save after the same instance becomes archived", async () => {
  const state = harness(); state.render(); await settle();
  const old = state.render().control("daily-backup-enabled");
  state.props.readOnly = true; state.render();
  old.props.onChange({ target: { checked: true } }); await settle();
  assert.equal(state.calls.some((call) => call.name === "updateDailyMaintenanceSchedule"), false);
  state.unmount();
});
