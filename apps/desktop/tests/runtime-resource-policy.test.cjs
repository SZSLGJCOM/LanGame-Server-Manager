const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const Module = require("node:module");
const path = require("node:path");
const React = require("react");
const helpHarness = require("./helpers/configuration-help-harness.cjs");
require.extensions[".css"] = () => {};
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".tsx"] = require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { readResourceLimits, resourceDraft, parseResourceDraft, mergeResourceLimits } = require("../src/runtime-resource-policy.ts");

test("limits are opt-in and retain explicit MiB units", () => {
  const defaults = { cpu_percent: null, memory_limit_mib: null, host_memory_reserve_mib: 2048 };
  assert.deepEqual(readResourceLimits({}), defaults);
  assert.deepEqual(parseResourceDraft(resourceDraft(defaults)), defaults);
  assert.deepEqual(parseResourceDraft({ cpu: "25", memory: "8192", reserve: "2048" }),
    { cpu_percent: 25, memory_limit_mib: 8192, host_memory_reserve_mib: 2048 });
});
test("invalid persisted limits and invalid drafts never become unlimited silently", () => {
  for (const value of [{ cpu_percent: 0 }, { cpu_percent: "20" }, { memory_limit_mib: 63 },
    { memory_limit_mib: 1.5 }, { host_memory_reserve_mib: null }, { unknown: 3 }, null]) {
    assert.throws(() => readResourceLimits({ runtime_performance: { resource_limits: value } }));
  }
  for (const cpu of ["0", "101", "1.5", "NaN"]) assert.throws(() => parseResourceDraft({ cpu, memory: "", reserve: "2048" }));
  for (const memory of ["1", "1048577", "Infinity"]) assert.throws(() => parseResourceDraft({ cpu: "", memory, reserve: "2048" }));
});
test("resource-only save preserves settings and detects same-field concurrency", () => {
  const baseline = { server_name: "Before", runtime_performance: { priority_class: "high" } };
  const latest = { ...baseline, server_name: "Concurrent", mods: ["123"] };
  const limits = { cpu_percent: 30, memory_limit_mib: 4096, host_memory_reserve_mib: 1024 };
  assert.deepEqual(mergeResourceLimits(latest, baseline, limits), { ...latest,
    runtime_performance: { priority_class: "high", resource_limits: limits } });
  const conflict = mergeResourceLimits(latest, baseline, { ...limits, cpu_percent: 40 });
  assert.throws(() => mergeResourceLimits(conflict, baseline, limits), /conflict/);
});

function descendants(node) {
  if (React.isValidElement(node) && node.type === helpHarness.ConfigurationHelp) return descendants(node.type(node.props));
  return React.isValidElement(node) ? [node, ...React.Children.toArray(node.props.children).flatMap(descendants)] : [];
}
function harness(save = async () => undefined) {
  const hooks = []; const effects = []; const calls = []; let cursor = 0;
  const react = { ...React,
    useState(initial) { const i = cursor++; if (!(i in hooks)) hooks[i] = typeof initial === "function" ? initial() : initial;
      return [hooks[i], (next) => { hooks[i] = typeof next === "function" ? next(hooks[i]) : next; }]; },
    useRef(initial) { const i = cursor++; if (!(i in hooks)) hooks[i] = { current: initial }; return hooks[i]; },
    useId() { return "resource-validation"; },
    useEffect(create) { const i = cursor++; if (!(i in hooks)) { hooks[i] = true; effects.push(create); } }
  };
  const filename = path.resolve(__dirname, "../src/views/servers/RuntimePerformanceEditor.tsx");
  const loaded = new Module(filename, module); loaded.filename = filename;
  const localRequire = Module.createRequire(filename);
  loaded.require = (id) => id === "react" ? react
    : id === "../settings/ConfigurationFieldHelp" ? helpHarness : localRequire(id);
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  const props = { details: { summary: { id: "a", bind_ip: "127.0.0.1", status: "stopped" },
    settings_json: '{"server_name":"Preserve"}', ports: [{ name: "game", port: 7777, protocol: "udp" }],
    auto_backup_on_stop: true, backup_retention_count: 5 }, t: (key) => key,
    async onSaveSettings(input, options) { calls.push({ input, options }); return save(input, options); } };
  function render() {
    cursor = 0;
    const wrapper = loaded.exports.RuntimePerformanceEditor(props);
    const root = wrapper.type(wrapper.props);
    while (effects.length) effects.shift()();
    const nodes = descendants(root);
    return { nodes, input: (name) => nodes.find((node) => node.type === "input" && node.props.name === name),
      form: nodes.find((node) => node.type === "form") };
  }
  return { props, calls, render,
    edit(name, value) { render().input(name).props.onChange({ target: { value } }); },
    submit() { render().form.props.onSubmit({ preventDefault() {} }); } };
}
const settle = () => new Promise((resolve) => setImmediate(resolve));
test("resource editor saves with CAS and preserves concurrent game settings", async () => {
  const state = harness();
  const initial = state.render();
  for (const field of ["cpu", "memory", "reserve"]) {
    const descriptionId = initial.input(field).props["aria-describedby"];
    assert.ok(descriptionId);
    assert.equal(initial.nodes.find((node) => node.props.id === descriptionId)?.props.children,
      "servers.resources.admission", "Each resource control keeps the admission help association");
  }
  state.edit("cpu", "25"); state.edit("memory", "8192");
  state.props.details.settings_json = '{"server_name":"Concurrent","mods":["123"]}';
  state.submit(); await settle();
  assert.equal(state.calls.length, 1);
  const call = state.calls[0];
  assert.deepEqual(JSON.parse(call.input.settings_json), { server_name: "Concurrent", mods: ["123"],
    runtime_performance: { resource_limits: { cpu_percent: 25, memory_limit_mib: 8192, host_memory_reserve_mib: 2048 } } });
  assert.equal(call.options.expectedSettingsJson, state.props.details.settings_json);
  assert.deepEqual(call.input.ports, state.props.details.ports);
  assert.equal(state.render().input("memory").props.value, "8192");
});
test("resource editor refuses invalid input and running-instance changes", async () => {
  const state = harness(); state.edit("cpu", "101"); state.submit(); await settle();
  assert.equal(state.calls.length, 0); assert.equal(state.render().input("cpu").props["aria-invalid"], true);
  state.edit("cpu", "25"); state.props.details.summary.status = "running";
  assert.equal(state.render().input("cpu").props.disabled, true);
  state.submit(); await settle(); assert.equal(state.calls.length, 0);
});
test("resource save remains single-flight while the backend is pending", async () => {
  let resolve;
  const state = harness(() => new Promise((done) => { resolve = done; }));
  state.edit("memory", "4096"); state.submit(); state.submit();
  assert.equal(state.calls.length, 1); assert.equal(state.render().input("memory").props.disabled, true);
  resolve(); await settle(); assert.equal(state.render().input("memory").props.value, "4096");
});

test("archived resource policy does not fabricate defaults and rejects direct edit or submit", async () => {
  const state = harness();
  state.props.readOnly = true;
  for (const json of ["{}", "invalid-json"]) {
    state.props.details.settings_json = json;
    const view = state.render();
    for (const name of ["cpu", "memory", "reserve"]) {
      assert.equal(view.input(name).props.value, "");
      assert.equal(view.input(name).props.disabled, true);
      assert.equal(view.input(name).props.placeholder, "servers.archives.configuration.notSaved");
    }
    state.edit("reserve", "2048");
    state.submit();
    await settle();
    assert.deepEqual(state.calls, []);
  }
  state.props.details.settings_json = JSON.stringify({ runtime_performance: { resource_limits: { cpu_percent: null, memory_limit_mib: 3072 } } });
  assert.equal(state.render().input("cpu").props.placeholder, "servers.resources.unlimited");
  assert.equal(state.render().input("memory").props.value, "3072");
  assert.equal(state.render().input("reserve").props.value, "");
});