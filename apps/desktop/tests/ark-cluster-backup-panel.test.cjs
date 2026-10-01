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
function deferred() { let resolve; const promise = new Promise((done) => { resolve = done; }); return { promise, resolve }; }
function report() {
  return { instance_id: "island", identity: { module_id: "arksurvivalevolved", cluster_id: "friends", directory_key: "d:/friends", member_ids: ["island"] }, cluster_directory: "D:/friends", members: [{ summary: { id: "island", status: "Stopped", active_process_count: 0 } }], issues: [] };
}
const snapshot = { backup_id: "cluster-1", created_at_unix_ms: 1700000000000, backup_kind: "manual", identity: report().identity, backup_path: "D:/backups/1", file_count: 2, total_bytes: 2048, members: [{ instance_id: "island", instance_name: "Island", map_name: "TheIsland" }] };
function descendants(node) { return React.isValidElement(node) ? [node, ...React.Children.toArray(node.props.children).flatMap(descendants)] : []; }
function harness(overrides = {}) {
  const hooks = [], effects = [], calls = [];
  let cursor = 0, writes = 0;
  const react = { ...React, useId: () => "backup",
    useRef(initial) { const index = cursor++; return hooks[index] ??= { current: initial }; },
    useState(initial) { const index = cursor++; hooks[index] ??= { value: initial }; return [hooks[index].value, (value) => { if (!Object.is(value, hooks[index].value)) writes++; hooks[index].value = value; }]; },
    useEffect(create, dependencies) { const index = cursor++; if (hooks[index]?.dependencies?.every((value, offset) => Object.is(value, dependencies[offset]))) return; const old = hooks[index]; hooks[index] = { dependencies }; effects.push(() => { old?.cleanup?.(); hooks[index].cleanup = create(); }); }
  };
  const filename = path.resolve(__dirname, "../src/views/servers/ArkClusterBackupPanel.tsx");
  const loaded = new Module(filename, module); loaded.filename = filename;
  const originalRequire = Module.createRequire(filename);
  loaded.require = (id) => {
    if (id === "react") return react;
    if (id === "../../i18n") return { useI18n: () => ({ locale: "en-US", t: (key) => key }), selectLocaleText: (locale, zh, en) => en };
    if (id === "../../app-state") return { describeError: (error) => error.message ?? String(error) };
    if (id.endsWith(".css")) return {};
    return originalRequire(id);
  };
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  const props = { report: report(), busy: false, listBackups: async () => [snapshot], createBackup: async (input) => { calls.push({ create: input }); return snapshot; }, restoreBackup: async (input) => { calls.push({ restore: input }); return { backup: snapshot, safeguard_backup: { ...snapshot, backup_id: "guard-1" }, restored_at_unix_ms: 1700000000100 }; }, onChanged: () => {}, ...overrides };
  function render() {
    let root;
    for (let attempt = 0; attempt < 10; attempt++) {
      cursor = 0; const before = writes; root = loaded.exports.ArkClusterBackupPanel(props);
      while (effects.length) effects.shift()();
      if (writes === before) break;
      assert.ok(attempt < 9);
    }
    const nodes = descendants(root);
    return { html: renderToStaticMarkup(root), check: nodes.find((node) => node.type === "input"), button: (label) => nodes.find((node) => node.type === "button" && node.props.children === label) };
  }
  return { props, calls, render, get writes() { return writes; }, unmount() { hooks.forEach((hook) => hook?.cleanup?.()); } };
}

test("cluster snapshot requires an explicit exclusive-directory acknowledgement and stopped members", async () => {
  const state = harness(); state.render(); await settle();
  let view = state.render();
  assert.equal(view.check.props.checked, false);
  assert.equal(view.button("Create cluster snapshot").props.disabled, true);
  view.check.props.onChange({ target: { checked: true } }); view = state.render();
  assert.equal(view.button("Create cluster snapshot").props.disabled, false);
  state.props.report.members[0].summary.status = "Running"; view = state.render();
  assert.equal(view.button("Create cluster snapshot").props.disabled, true);
  assert.match(view.html, /Stop every cluster member first/);
  state.props.report.members[0].summary.status = "Stopped";
  state.render().button("Create cluster snapshot").props.onClick(); await settle();
  assert.equal(state.calls.length, 1);
  assert.deepEqual(state.calls[0].create, { instance_id: "island", expected_identity: report().identity, exclusive_root_confirmed: true });
  state.unmount();
});

test("restore presents member and replacement scope before sending its snapshot identity", async () => {
  const state = harness(); state.render(); await settle();
  state.render().check.props.onChange({ target: { checked: true } });
  state.render().button("Restore…").props.onClick();
  const confirmation = state.render();
  assert.match(confirmation.html, /Replace all member worlds, configurations, and the shared directory/);
  assert.match(confirmation.html, /Island \(TheIsland\)/);
  assert.equal(state.calls.length, 0);
  confirmation.button("Restore entire cluster").props.onClick(); await settle();
  assert.equal(state.calls[0].restore.backup_id, "cluster-1");
  assert.equal(state.calls[0].restore.exclusive_root_confirmed, true);
  assert.match(state.render().html, /Pre-restore safeguard: guard-1/);
  state.unmount();
});

test("incomplete peer inspection blocks backup and changing group clears the directory acknowledgement", async () => {
  const state = harness(); state.render(); await settle();
  state.render().check.props.onChange({ target: { checked: true } });
  state.props.report.issues = [{ code: "peer_inspection_incomplete", severity: "warning" }];
  assert.equal(state.render().button("Create cluster snapshot").props.disabled, true);
  state.props.report = { ...report(), identity: { ...report().identity, cluster_id: "other" } };
  state.render(); await settle();
  assert.equal(state.render().check.props.checked, false);
  state.unmount();
});

test("backup completion after unmount performs no stale component state writes", async () => {
  const pending = deferred();
  const state = harness({ createBackup: () => pending.promise }); state.render(); await settle();
  state.render().check.props.onChange({ target: { checked: true } });
  state.render().button("Create cluster snapshot").props.onClick(); state.unmount();
  const before = state.writes; pending.resolve(snapshot); await settle();
  assert.equal(state.writes, before);
});

test("a same-frame repeated backup action sends only one mutation", async () => {
  const pending = deferred(); let calls = 0;
  const state = harness({ createBackup: () => { calls++; return pending.promise; } });
  state.render(); await settle(); state.render().check.props.onChange({ target: { checked: true } });
  const action = state.render().button("Create cluster snapshot").props.onClick;
  action(); action(); assert.equal(calls, 1);
  pending.resolve(snapshot); await settle(); state.unmount();
});

test("restore cleanup warnings survive the successful backup list refresh", async () => {
  const state = harness({ restoreBackup: async () => ({ backup: snapshot, safeguard_backup: snapshot,
    restored_at_unix_ms: 1, cleanup_warnings: ["Safeguard cleanup requires attention"] }) });
  state.render(); await settle(); state.render().check.props.onChange({ target: { checked: true } });
  state.render().button("Restore…").props.onClick();
  state.render().button("Restore entire cluster").props.onClick(); await settle();
  assert.match(state.render().html, /Safeguard cleanup requires attention/);
  state.unmount();
});
