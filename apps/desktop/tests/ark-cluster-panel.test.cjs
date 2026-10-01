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
function deferred() {
  let resolve;
  const promise = new Promise((done) => { resolve = done; });
  return { promise, resolve };
}
function cluster(instanceId = "island", overrides = {}) {
  return {
    instance_id: instanceId,
    identity: { module_id: "arksurvivalevolved", cluster_id: "friends", directory_key: "d:/clusters/friends", member_ids: [instanceId] },
    cluster_directory: "D:/clusters/friends",
    members: [{ summary: { id: instanceId, name: instanceId, status: "Stopped" }, map_name: "TheIsland", ports: [{ name: "game", port: 7777, protocol: "udp" }] }],
    related_instances: [], issues: [], start_blocked: false, ...overrides
  };
}
function descendants(node) {
  return React.isValidElement(node) ? [node, ...React.Children.toArray(node.props.children).flatMap(descendants)] : [];
}
function harness(read = async (id) => cluster(id), operation = async () => ({ action: "start", members: [] })) {
  const hooks = [], effects = [], calls = [];
  let cursor = 0, writes = 0, changed = 0;
  const react = {
    ...React, useId: () => "cluster",
    useRef(initial) { const slot = cursor++; return hooks[slot] ??= { current: initial }; },
    useState(initial) {
      const slot = cursor++;
      hooks[slot] ??= { value: initial };
      return [hooks[slot].value, (next) => { if (!Object.is(next, hooks[slot].value)) writes++; hooks[slot].value = next; }];
    },
    useEffect(create, dependencies) {
      const slot = cursor++;
      if (hooks[slot]?.dependencies?.every((value, index) => Object.is(value, dependencies[index]))) return;
      const old = hooks[slot]; hooks[slot] = { dependencies };
      effects.push(() => { old?.cleanup?.(); hooks[slot].cleanup = create(); });
    }
  };
  const filename = path.resolve(__dirname, "../src/views/servers/ArkClusterPanel.tsx");
  const loaded = new Module(filename, module);
  loaded.filename = filename;
  const originalRequire = Module.createRequire(filename);
  loaded.require = (id) => {
    if (id === "react") return react;
    if (id === "../../i18n") return { useI18n: () => ({ locale: "en-US", t: (key, params, fallback) => fallback ?? key }), selectLocaleText: (locale, zh, en) => en };
    if (id === "../../app-state") return { describeError: (error) => error.message ?? String(error) };
    if (id.endsWith(".css")) return {};
    return originalRequire(id);
  };
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  const props = { instanceId: "island", readReport: read, operate: (input) => { calls.push(input); return operation(input); }, onChanged: () => { changed++; } };
  function render() {
    let root;
    for (let attempt = 0; attempt < 10; attempt++) {
      cursor = 0;
      const before = writes;
      root = loaded.exports.ArkClusterPanel(props);
      while (effects.length) effects.shift()();
      if (before === writes) break;
      assert.ok(attempt < 9, "effects settle");
    }
    return { html: renderToStaticMarkup(root), buttons: descendants(root).filter((node) => node.type === "button") };
  }
  return { props, calls, render, get writes() { return writes; }, get changed() { return changed; }, unmount() { hooks.forEach((hook) => hook?.cleanup?.()); } };
}

test("cluster inspection renders real member map and ports, and forwards the inspected identity", async () => {
  const operation = deferred();
  const state = harness(undefined, () => operation.promise);
  assert.match(state.render().html, /Inspecting cluster members/);
  await settle();
  const view = state.render();
  assert.match(view.html, /TheIsland/);
  assert.match(view.html, /7777\/udp/);
  view.buttons.find((button) => button.props.children === "Start cluster").props.onClick();
  assert.deepEqual(state.calls[0], { instance_id: "island", expected_identity: cluster().identity, action: "start" });
  assert.ok(state.render().buttons.every((button) => button.props.disabled));
  operation.resolve({ action: "start", members: [
    { instance_id: "island", instance_name: "Island", outcome: "succeeded", message: "started" },
    { instance_id: "center", instance_name: "Center", outcome: "failed", message: "Port is occupied" }
  ] });
  await settle();
  assert.match(state.render().html, /1 completed, 0 skipped, 1 failed/);
  assert.match(state.render().html, /Port is occupied/);
  assert.equal(state.changed, 1);
  state.unmount();
});

test("cluster conflicts block start while keeping stop available", async () => {
  const state = harness(async () => cluster("island", {
    start_blocked: true,
    issues: [{ instance_id: "center", instance_name: "Center", code: "id_directory_mismatch", severity: "error", message: "different", path: "D:/private" }]
  }));
  state.render(); await settle();
  const view = state.render();
  assert.equal(view.buttons.find((button) => button.props.children === "Start cluster").props.disabled, true);
  assert.equal(view.buttons.find((button) => button.props.children === "Stop cluster").props.disabled, false);
  assert.match(view.html, /transfer data is not shared/);
  state.unmount();
});

test("late inspection and operation results cannot overwrite another selected instance or an unmounted panel", async () => {
  const initial = deferred();
  const state = harness((id) => id === "island" ? initial.promise : Promise.resolve(cluster(id)));
  state.render(); state.props.instanceId = "center"; state.render(); await settle();
  initial.resolve(cluster("island")); await settle();
  assert.match(state.render().html, /center/);
  assert.doesNotMatch(state.render().html, />island</);
  state.unmount();
  const pending = deferred();
  const other = harness(undefined, () => pending.promise);
  other.render(); await settle();
  other.render().buttons.find((button) => button.props.children === "Start cluster").props.onClick();
  other.unmount(); const before = other.writes;
  pending.resolve({ action: "start", members: [] }); await settle();
  assert.equal(other.writes, before);
});
