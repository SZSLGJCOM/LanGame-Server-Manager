const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
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

const { createAutosaveHarness, deferred, pendingDelays, runTimers } = require("./helpers/maintenance-autosave-harness.cjs");
function harness(save) {
  const calls = [];
  const props = { details: { summary: { id: "a", bind_ip: "127.0.0.1", status: "stopped" },
    settings_json: '{"server_name":"Preserve"}', ports: [{ name: "game", port: 7777, protocol: "udp" }],
    auto_backup_on_stop: true, backup_retention_count: 5 }, t: (key, params) => params?.message ? `${key}: ${params.message}` : key };
  props.onSaveSettings = (input, options) => {
    calls.push({ input, options });
    const persist = () => (props.details = { ...props.details, settings_json: input.settings_json });
    return save ? save(input, options).then(persist) : Promise.resolve(persist());
  };
  const state = createAutosaveHarness({ editorPath: "../../src/views/servers/RuntimePerformanceEditor.tsx",
    exportName: "RuntimePerformanceEditor", props });
  return { ...state, calls, limits: (index) => JSON.parse(calls[index].input.settings_json).runtime_performance.resource_limits };
}

test("resource editor auto-saves after typing pauses with CAS and preserves concurrent game settings", async () => {
  const state = harness();
  const initial = state.render();
  for (const field of ["cpu", "memory", "reserve"]) {
    const descriptionId = initial.input(field).props["aria-describedby"];
    assert.ok(descriptionId);
    assert.equal(initial.nodes.find((node) => node.props.id === descriptionId)?.props.children,
      "servers.resources.admission", "Each resource control keeps the admission help association");
  }
  assert.equal(initial.submit, undefined, "auto-save replaces the redundant save button");
  assert.equal(initial.state, "saved");
  state.edit("cpu", "25"); state.edit("memory", "8192");
  assert.deepEqual(pendingDelays(), [650], "typed limits wait for a pause and restart the debounce");
  state.props.details = { ...state.props.details, settings_json: '{"server_name":"Concurrent","mods":["123"]}' };
  const concurrent = state.props.details;
  state.render();
  await runTimers();
  assert.equal(state.calls.length, 1);
  const call = state.calls[0];
  assert.deepEqual(JSON.parse(call.input.settings_json), { server_name: "Concurrent", mods: ["123"],
    runtime_performance: { resource_limits: { cpu_percent: 25, memory_limit_mib: 8192, host_memory_reserve_mib: 2048 } } });
  assert.equal(call.options.expectedSettingsJson, concurrent.settings_json);
  assert.deepEqual(call.input.ports, concurrent.ports);
  const view = state.render();
  assert.equal(view.input("memory").props.value, "8192");
  assert.equal(view.state, "saved");
});

test("resource editor blocks invalid input and running-instance changes", async () => {
  const state = harness();
  state.edit("cpu", "101"); await runTimers();
  assert.equal(state.calls.length, 0);
  let view = state.render();
  assert.equal(view.input("cpu").props["aria-invalid"], true);
  assert.equal(view.state, "blocked");
  assert.match(view.html(), /servers.resources.savedValues/, "blocked edits show which limits remain saved");
  view.discard.props.onClick();
  state.props.details.summary.status = "running";
  view = state.render();
  assert.equal(view.input("cpu").props.disabled, true);
  view.input("cpu").props.onChange({ target: { value: "25" } });
  await runTimers();
  assert.equal(state.calls.length, 0);
});

test("resource edits keep the latest value during in-flight saves and never overwrite external limit changes", async () => {
  const held = [];
  const state = harness(() => { const gate = deferred(); held.push(gate); return gate.promise; });
  state.edit("memory", "4096"); await runTimers();
  assert.equal(state.calls.length, 1);
  assert.equal(state.render().state, "saving");
  state.edit("memory", "6144");
  held.shift().resolve(); await runTimers();
  assert.doesNotMatch(state.render().html(), /servers.resources.conflict/, "own earlier write is not a conflict");
  assert.equal(state.calls.length, 2);
  assert.equal(state.limits(1).memory_limit_mib, 6144);
  held.shift().resolve(); await runTimers();
  assert.equal(state.render().input("memory").props.value, "6144");
  assert.equal(state.render().state, "saved");

  state.edit("cpu", "40");
  state.props.details = { ...state.props.details, settings_json: JSON.stringify({ runtime_performance: { resource_limits:
    { cpu_percent: 10, memory_limit_mib: 6144, host_memory_reserve_mib: 2048 } } }) };
  let view = state.render();
  await runTimers();
  assert.equal(state.calls.length, 2, "limits changed elsewhere are not silently overwritten");
  assert.equal(view.state, "blocked");
  assert.match(view.html(), /servers.resources.conflict/);
  view.discard.props.onClick();
  view = state.render();
  assert.equal(view.input("cpu").props.value, "10");
  assert.equal(view.state, "saved");
});

test("resource save failures offer retry and unmount flushes the pending draft", async () => {
  let reject = true;
  const state = harness(async () => { if (reject) throw new Error("fixture write rejected"); });
  state.edit("memory", "8192"); await runTimers();
  let view = state.render();
  assert.equal(view.state, "failed");
  assert.match(view.html(), /fixture write rejected/);
  assert.equal(view.input("memory").props.value, "8192");
  reject = false; view.retry.props.onClick(); await runTimers();
  assert.equal(state.calls.length, 2);
  assert.equal(state.render().state, "saved");
  state.edit("reserve", "1024");
  state.unmount(); await runTimers();
  assert.equal(state.calls.length, 3);
  assert.equal(state.limits(2).host_memory_reserve_mib, 1024);
});

test("archived resource policy does not fabricate defaults and rejects direct edits", async () => {
  const state = harness();
  state.props.readOnly = true;
  for (const json of ["{}", "invalid-json"]) {
    state.props.details.settings_json = json;
    const view = state.render();
    assert.equal(view.footer, undefined);
    for (const name of ["cpu", "memory", "reserve"]) {
      assert.equal(view.input(name).props.value, "");
      assert.equal(view.input(name).props.disabled, true);
      assert.equal(view.input(name).props.placeholder, "servers.archives.configuration.notSaved");
    }
    view.input("reserve").props.onChange({ target: { value: "2048" } });
    await runTimers();
    assert.deepEqual(state.calls, []);
  }
  state.props.details.settings_json = JSON.stringify({ runtime_performance: { resource_limits: { cpu_percent: null, memory_limit_mib: 3072 } } });
  assert.equal(state.render().input("cpu").props.placeholder, "servers.resources.unlimited");
  assert.equal(state.render().input("memory").props.value, "3072");
  assert.equal(state.render().input("reserve").props.value, "");
  state.unmount(); await runTimers();
  assert.deepEqual(state.calls, []);
});
