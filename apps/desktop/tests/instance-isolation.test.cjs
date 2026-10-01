const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const helpHarness = require("./helpers/configuration-help-harness.cjs");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".tsx"] = require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const settle = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function report(id = "server-a", overrides = {}) {
  return {
    instance_id: id, mode: "private",
    runtime_path: "D:/instances/" + id + "/runtime",
    data_path: "D:/managed-data/" + id,
    config_path: "D:/instances/" + id + "/config",
    saves_path: "D:/instances/" + id + "/config/clusters/main",
    conflicts: [], issues: [], ...overrides
  };
}
function descendants(node) {
  if (!React.isValidElement(node)) return [];
  if (node.type === helpHarness.ConfigurationHelp) return descendants(node.type(node.props));
  return [node, ...React.Children.toArray([node.props.children, node.props.action]).flatMap(descendants)];
}
function harness(read = async (id) => report(id)) {
  const hooks = [], effects = [], calls = [], opened = [];
  let cursor = 0, writes = 0;
  const equal = (left, right) => left?.length === right?.length && left.every((value, index) => Object.is(value, right[index]));
  const react = {
    ...React,
    useId: () => "isolation-diagnostics",
    useState(initial) {
      const slot = cursor++;
      if (!(slot in hooks)) hooks[slot] = { value: typeof initial === "function" ? initial() : initial };
      return [hooks[slot].value, (next) => {
        const value = typeof next === "function" ? next(hooks[slot].value) : next;
        if (!Object.is(value, hooks[slot].value)) writes++;
        hooks[slot].value = value;
      }];
    },
    useRef(initial) {
      const slot = cursor++;
      if (!(slot in hooks)) hooks[slot] = { current: initial };
      return hooks[slot];
    },
    useEffect(create, deps) {
      const slot = cursor++;
      if (equal(hooks[slot]?.deps, deps)) return;
      const previous = hooks[slot];
      hooks[slot] = { deps };
      effects.push(() => { previous?.cleanup?.(); hooks[slot].cleanup = create(); });
    }
  };
  const filename = path.resolve(__dirname, "../src/views/servers/InstanceIsolationPanel.tsx");
  const loaded = new Module(filename, module);
  loaded.filename = filename;
  const requireFromFile = Module.createRequire(filename);
  const t = (key, params) => key + (params?.message ? ": " + params.message : params?.path ? ": " + params.path : "");
  loaded.require = (id) => {
    if (id === "react") return react;
    if (id === "../settings/ConfigurationFieldHelp") return helpHarness;
    if (id === "../../i18n") return { useI18n: () => ({ t }) };
    if (id === "../../app-state") return { describeError: (error) => error.message ?? String(error) };
    if (id === "../../components/ShellIcon") return { ShellIcon: () => React.createElement("svg") };
    if (id === "../../instance-panel-loader") return {
      instancePanelReader: { readIsolation(instanceId, signal) { calls.push({ instanceId, signal }); return read(instanceId, signal); } },
      formatInstancePanelError: (error) => error
    };
    if (id.endsWith(".css")) return {};
    return requireFromFile(id);
  };
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  const props = { instanceId: "server-a", onOpenLocalPath: (value) => opened.push(value) };
  function render() {
    let root;
    for (let attempt = 0; attempt < 10; attempt++) {
      cursor = 0;
      const before = writes;
      root = loaded.exports.InstanceIsolationPanel(props);
      while (effects.length) effects.shift()();
      if (before === writes) break;
      assert.ok(attempt < 9, "effects must settle");
    }
    const nodes = descendants(root);
    return {
      html: renderToStaticMarkup(root),
      nodes,
      refresh: nodes.find((node) => node.type === "button" && node.props.className.includes("instance-isolation-refresh")),
      open: nodes.filter((node) => node.type === "button" && node.props["aria-label"])
    };
  }
  return { props, calls, opened, render, get writes() { return writes; }, unmount() { for (const hook of hooks) hook?.cleanup?.(); } };
}

test("independent-instance diagnostics show actual paths without conversion controls", async () => {
  const state = harness();
  assert.match(state.render().html, /servers.isolation.loading/);
  assert.equal(state.render().refresh.props.disabled, true);
  await settle();
  const rendered = state.render();
  assert.equal(rendered.nodes.some((node) => node.type === "details" || node.type === "summary"), false,
    "diagnostics and directory controls must always be visible");
  assert.doesNotMatch(rendered.html, /servers.isolation.privateDescription|servers.isolation.noConflicts/);
  assert.match(rendered.html, /servers.isolation.mode.private/);
  assert.match(rendered.html, /D:\/instances\/server-a\/runtime/);
  assert.match(rendered.html, /D:\/instances\/server-a\/config\/clusters\/main/);
  assert.equal(rendered.nodes.filter((node) => node.type === "input").length, 0);
  assert.equal(rendered.nodes.filter((node) => node.type === "button").length, 5);
  rendered.open[0].props.onClick();
  assert.deepEqual(state.opened, ["D:/instances/server-a/runtime"]);
  rendered.open[1].props.onClick();
  assert.deepEqual(state.opened, ["D:/instances/server-a/runtime", "D:/managed-data/server-a"],
    "data location must use the reported path rather than derive a parent directory in the frontend");
  state.unmount();
});

test("read failure exposes the cause and retries without claiming private isolation", async () => {
  let fail = true;
  const state = harness(async (id) => {
    if (fail) throw new Error("Isolation database unavailable");
    return report(id);
  });
  state.render();
  await settle();
  assert.match(state.render().html, /role="alert"/);
  assert.match(state.render().html, /Isolation database unavailable/);
  assert.match(state.render().html, /shell-activity-notice is-error/);
  assert.match(state.render().html, /servers.isolation.readFailed/);
  assert.doesNotMatch(state.render().html, /instance-isolation-content/);
  assert.doesNotMatch(state.render().html, /servers.isolation.mode.private/);
  assert.equal(state.render().refresh.props.children, "common.refresh");
  const retry = state.render().nodes.find((node) => node.type === "button" && node.props.children === "common.retry");
  assert.ok(retry, "the failure retains its activity-bar retry action");
  fail = false;
  retry.props.onClick();
  await settle();
  assert.equal(state.calls.length, 2);
  assert.match(state.render().html, /servers.isolation.mode.private/);
  state.unmount();
});

test("a damaged instance can display conflicts and issues without InstanceDetails", async () => {
  const state = harness(async (id) => report(id, {
    mode: "damaged", issues: ["Instance runtime directory is missing"],
    conflicts: [{ instance_id: "server-b", instance_name: "Second DST server", kind: "saves",
      path: "D:/worlds/shared", other_path: "D:/worlds/shared/Master" }]
  }));
  state.render();
  await settle();
  const rendered = state.render();
  assert.match(rendered.html, /servers.isolation.mode.damaged/);
  assert.match(rendered.html, /Instance runtime directory is missing/);
  assert.match(rendered.html, /Second DST server/);
  assert.match(rendered.html, /D:\/worlds\/shared\/Master/);
  assert.doesNotMatch(rendered.html, /servers.isolation.mode.private/);
  state.unmount();
});

test("a failed refresh removes an earlier healthy report", async () => {
  let fail = false;
  const state = harness(async (id) => {
    if (fail) throw new Error("Directory probe failed");
    return report(id);
  });
  state.render();
  await settle();
  assert.match(state.render().html, /servers.isolation.mode.private/);
  fail = true;
  state.render().refresh.props.onClick();
  await settle();
  assert.match(state.render().html, /Directory probe failed/);
  assert.doesNotMatch(state.render().html, /servers.isolation.mode.private/);
  state.unmount();
});

test("selection changes and unmount suppress stale isolation results", async () => {
  const oldRead = deferred();
  const state = harness((id) => id === "server-a" ? oldRead.promise : Promise.resolve(report(id)));
  state.render();
  state.props.instanceId = "server-b";
  state.render();
  assert.equal(state.calls[0].signal.aborted, true);
  await settle();
  oldRead.resolve(report("server-a", { mode: "damaged", issues: ["stale issue"] }));
  await settle();
  assert.match(state.render().html, /D:\/instances\/server-b\/runtime/);
  assert.doesNotMatch(state.render().html, /stale issue/);
  assert.doesNotMatch(state.render().html, /servers.isolation.mode.damaged/);
  state.unmount();
  assert.equal(state.calls[1].signal.aborted, true);

  const pending = deferred();
  const unmounted = harness(() => pending.promise);
  unmounted.render();
  unmounted.unmount();
  const writes = unmounted.writes;
  pending.reject(new Error("late error"));
  await settle();
  assert.equal(unmounted.writes, writes);
});

test("a healthy runtime with conflicts or issues still exposes diagnostics", async () => {
  for (const overrides of [
    { conflicts: [{ instance_id: "server-b", instance_name: "Other server", kind: "saves",
      path: "D:/worlds/shared", other_path: "D:/worlds/shared/Master" }] },
    { issues: ["Configuration directory is unavailable"] }
  ]) {
    const state = harness(async (id) => report(id, overrides));
    state.render();
    await settle();
    const rendered = state.render();
    assert.match(rendered.html, /servers.isolation.mode.warning/);
    assert.doesNotMatch(rendered.html, /servers.isolation.mode.private/);
    state.unmount();
  }
});

test("all directory shortcuts stay visible with full paths and retain the diagnostic lifecycle", async () => {
  const state = harness();
  state.props.backupPath = "D:/backups/server-a";
  state.render();
  await settle();
  const rendered = state.render();
  assert.equal(rendered.open.length, 5);
  for (const button of rendered.open) {
    assert.equal(button.props.disabled, false);
    assert.equal(button.props.title, undefined, "Directory help must use the shared tooltip only");
    const descriptionId = button.props["aria-describedby"];
    assert.ok(descriptionId);
    const description = rendered.nodes.find((node) => node.props.id === descriptionId);
    assert.match(description.props.children, /^D:\//);
    assert.ok(button.props["aria-label"].includes(description.props.children));
  }
  rendered.open[4].props.onClick();
  assert.deepEqual(state.opened, ["D:/backups/server-a"]);
  const refreshedView = state.render();
  assert.equal(state.calls.length, 1, "rendering directory shortcuts must not restart the diagnostic request");
  assert.equal(state.calls[0].signal.aborted, false);
  assert.match(refreshedView.html, /D:\/instances\/server-a\/runtime/);
  assert.match(refreshedView.html, /D:\/backups\/server-a/);
  state.unmount();
});

const { buildMockInstanceIsolation } = require("../src/api-mock/instance-isolation.ts");
function details(id, saves) {
  return {
    summary: { id, name: id, module_id: "dontstarve", status: "Stopped", active_process_count: 0 },
    config_file_path: "D:/instances/" + id + "/config/instance.json",
    saves_path: saves ?? "D:/instances/" + id + "/config/clusters/main"
  };
}
test("same-game mock instances always have separate runtimes and the current report contract", () => {
  const a = details("a"), b = details("b");
  const first = buildMockInstanceIsolation(a, [a, b]);
  const second = buildMockInstanceIsolation(b, [a, b]);
  assert.equal(first.mode, "private");
  assert.equal(first.runtime_path, "D:/instances/a/runtime");
  assert.equal(first.data_path, "D:/instances/a");
  assert.equal(second.runtime_path, "D:/instances/b/runtime");
  assert.deepEqual(first.conflicts, []);
  assert.deepEqual(Object.keys(first).sort(), [
    "instance_id", "mode", "runtime_path", "data_path", "config_path", "saves_path", "conflicts", "issues"
  ].sort());
});
test("independent mock runtimes retain real save-overlap diagnostics", () => {
  const a = details("a", "D:/shared-world");
  const b = details("b", "D:/shared-world/Master");
  const current = buildMockInstanceIsolation(a, [a, b]);
  assert.equal(current.runtime_path, "D:/instances/a/runtime");
  assert.deepEqual(current.conflicts, [{
    instance_id: "b", instance_name: "b", kind: "saves",
    path: "D:/shared-world", other_path: "D:/shared-world/Master"
  }]);
});
