const ts = require("@typescript/typescript6");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const React = require("react");
const helpHarness = require("./helpers/configuration-help-harness.cjs");
const { renderToStaticMarkup } = require("react-dom/server");
const { parseSource, sourceText, transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
// Browser coverage verifies styles; these Node tests exercise policy state and persistence.
require.extensions[".css"] = () => {};
require.extensions[".tsx"] = require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const { readRecoveryPolicy, recoveryDraft, parseRecoveryDraft, mergeRecoveryPolicy } = require("../src/views/servers/runtime-recovery-model.ts");
const defaults = { enabled: false, max_restarts: 3, backoff_ms: 5000, only_nonzero_exit: true };
const settle = () => new Promise((resolve) => setImmediate(resolve));

test("recovery policy reads the current supervisor defaults and bounded policy", () => {
  assert.deepEqual(readRecoveryPolicy({}), defaults);
  assert.deepEqual(readRecoveryPolicy({ runtime_restart: { enabled: true, max_restarts: 25, backoff_ms: 900000, only_nonzero_exit: false } }),
    { enabled: true, max_restarts: 10, backoff_ms: 300000, only_nonzero_exit: false });
});
test("recovery policy ignores obsolete JSON keys and requires current scalar types", () => {
  const obsolete = { restart_policy: { enabled: true, max_restarts: 9, backoff_ms: 20000, only_nonzero_exit: false },
    auto_restart_enabled: true, crash_restart_limit: 8, auto_restart_backoff_ms: 25000, auto_restart_only_nonzero_exit: false };
  assert.deepEqual(readRecoveryPolicy(obsolete), defaults);
  assert.deepEqual(readRecoveryPolicy({ ...obsolete, runtime_restart: { enabled: false, backoff_ms: 0 } }),
    { ...defaults, backoff_ms: 0 });
  assert.deepEqual(readRecoveryPolicy({ runtime_restart: { enabled: "yes", max_restarts: "8", backoff_ms: "20000", only_nonzero_exit: "off" } }), defaults);
});
test("mock runtime stability uses current recovery defaults and preserves a zero backoff", () => {
  const filename = path.join(__dirname, "../src/api-mock.ts");
  const source = fs.readFileSync(filename, "utf8");
  const declaration = parseSource(source, filename).statements.find(
    (node) => ts.isFunctionDeclaration(node) && node.name.text === "buildMockStability"
  );
  assert.ok(declaration);
  const stability = vm.runInNewContext(
    transpileTypeScript(sourceText(source, declaration), filename) + "\nbuildMockStability;",
    { readRecoveryPolicy, parseSettingsJson: JSON.parse }, { filename }
  );
  const obsolete = stability({ summary: { status: "Stopped" }, settings_json: JSON.stringify({
    auto_restart_enabled: true, crash_restart_limit: 9, auto_restart_backoff_ms: 20000
  }) });
  assert.equal(obsolete.restart_policy_enabled, false);
  assert.equal(obsolete.restart_limit, 3);
  assert.equal(obsolete.restart_backoff_ms, 5000);
  const current = stability({ summary: { status: "Stopped" }, settings_json: JSON.stringify({
    runtime_restart: { enabled: true, max_restarts: 25, backoff_ms: 0 }
  }) });
  assert.equal(current.restart_policy_enabled, true);
  assert.equal(current.restart_limit, 10);
  assert.equal(current.restart_backoff_ms, 0);
});
test("recovery draft validates finite whole retry counts and millisecond-accurate waits", () => {
  const draft = recoveryDraft(defaults);
  assert.deepEqual(parseRecoveryDraft(draft), { value: defaults, error: null });
  for (const maxRestarts of ["", "0", "11", "1.5", "NaN"]) assert.equal(parseRecoveryDraft({ ...draft, maxRestarts }).error, "count");
  for (const waitSeconds of ["", "-1", "301", "Infinity", "0.0001"]) assert.equal(parseRecoveryDraft({ ...draft, waitSeconds }).error, "wait");
  assert.equal(parseRecoveryDraft({ ...draft, waitSeconds: "0.001" }).value.backoff_ms, 1);
});
test("recovery patch preserves unrelated settings and rejects conflicting owned settings", () => {
  const baseline = { server_name: "Before", runtime_restart: defaults };
  const latest = { server_name: "Concurrent name", mods: ["123"], runtime_restart: defaults };
  const policy = { ...defaults, enabled: true };
  assert.deepEqual(mergeRecoveryPolicy(latest, baseline, policy), { ...latest, runtime_restart: policy });
  assert.throws(() => mergeRecoveryPolicy({ ...latest, runtime_restart: { ...defaults, max_restarts: 4 } }, baseline, policy), /conflict/);
  const extra = { ...latest, runtime_restart: { ...defaults, future_option: "keep" } };
  assert.deepEqual(mergeRecoveryPolicy(extra, baseline, policy).runtime_restart, { ...policy, future_option: "keep" });
  assert.throws(() => mergeRecoveryPolicy({ runtime_restart: { enabled: true } }, {}, { ...defaults, max_restarts: 5 }), /conflict/);
});

function descendants(node) {
  return React.isValidElement(node) ? [node, ...React.Children.toArray(node.props.children).flatMap(descendants)] : [];
}
function details(id = "server-a", settings = { server_name: "Keep" }) {
  return { summary: { id, bind_ip: "127.0.0.1" }, auto_backup_on_stop: true, backup_retention_count: 7,
    settings_json: JSON.stringify(settings), ports: [{ name: "game", port: 7777, protocol: "udp" }] };
}
function harness(save = async () => {}) {
  const hooks = [], effects = [], calls = [];
  let cursor = 0;
  const equal = (a, b) => a?.length === b?.length && a.every((value, index) => Object.is(value, b[index]));
  const react = { ...React, useId: () => "recovery-validation",
    useState(initial) { const i = cursor++; if (!(i in hooks)) hooks[i] = { value: typeof initial === "function" ? initial() : initial };
      return [hooks[i].value, (next) => { hooks[i].value = typeof next === "function" ? next(hooks[i].value) : next; }]; },
    useMemo(create, deps) { const i = cursor++; if (!equal(hooks[i]?.deps, deps)) hooks[i] = { value: create(), deps }; return hooks[i].value; },
    useRef(initial) { const i = cursor++; if (!(i in hooks)) hooks[i] = { current: initial }; return hooks[i]; },
    useEffect(create, deps) { const i = cursor++; if (equal(hooks[i]?.deps, deps)) return;
      const previous = hooks[i]; hooks[i] = { deps }; effects.push(() => { previous?.cleanup?.(); hooks[i].cleanup = create(); }); }
  };
  const filename = path.resolve(__dirname, "../src/views/servers/RuntimeRecoveryEditor.tsx");
  const loaded = new Module(filename, module); loaded.filename = filename;
  const localRequire = Module.createRequire(filename);
  loaded.require = (id) => id === "react" ? react
    : id === "../settings/ConfigurationFieldHelp" ? helpHarness : localRequire(id);
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  const props = { details: details(), t: (key) => key, async onSaveSettings(input, options) { calls.push({ input, options }); await save(input, options); } };
  function render() { cursor = 0; const root = loaded.exports.RuntimeRecoveryEditor(props); while (effects.length) effects.shift()();
    const nodes = descendants(root); return { root, html: renderToStaticMarkup(root),
      input: (name) => nodes.find((node) => node.type === "input" && node.props.name === name),
      submit: nodes.find((node) => node.type === "button" && node.props.type === "submit") }; }
  return { props, calls, render, edit(name, value) { render().input(name).props.onChange({ target: { value, checked: value } }); },
    submit() { render().root.props.onSubmit({ preventDefault() {} }); },
    unmount() { for (const hook of hooks) hook?.cleanup?.(); } };
}
test("recovery editor saves canonical policy with CAS while retaining backups, ports and concurrent game edits", async () => {
  const state = harness();
  const initial = state.render();
  const exitControl = initial.input("onlyNonzeroExit");
  const exitDescription = descendants(initial.root).find((node) => node.props.id === exitControl.props["aria-describedby"]);
  assert.equal(exitControl.props.title, undefined);
  assert.equal(exitDescription?.props.children, "servers.recovery.exitBehavior", "Recovery help remains associated with its checkbox");
  assert.equal(state.render().submit.props.disabled, true);
  state.edit("enabled", true);
  state.props.details = details("server-a", { server_name: "Concurrent", mods: ["123"] });
  state.submit(); await settle();
  const { input, options } = state.calls[0];
  assert.deepEqual(JSON.parse(input.settings_json), { server_name: "Concurrent", mods: ["123"], runtime_restart: { ...defaults, enabled: true } });
  assert.equal(input.auto_backup_on_stop, true); assert.equal(input.backup_retention_count, 7);
  assert.deepEqual(input.ports, state.props.details.ports);
  assert.equal(options.expectedSettingsJson, state.props.details.settings_json);
  assert.equal(state.render().input("enabled").props.checked, true, "successful policy must survive delayed refresh");
  assert.equal(state.render().submit.props.disabled, true);
});
test("recovery editor blocks invalid values and owned conflicts without saving", async () => {
  const state = harness(); state.edit("enabled", true); state.edit("maxRestarts", "0"); state.submit(); await settle();
  assert.equal(state.calls.length, 0); assert.equal(state.render().input("maxRestarts").props["aria-invalid"], true);
  state.edit("maxRestarts", "4");
  state.props.details = details("server-a", { runtime_restart: { ...defaults, max_restarts: 5 } });
  state.submit(); await settle();
  assert.equal(state.calls.length, 0); assert.match(state.render().html, /servers.recovery.conflict/);
});
test("recovery saves are single-flight and instance switches isolate late completion", async () => {
  let resolve; const state = harness(() => new Promise((done) => { resolve = done; }));
  state.edit("enabled", true); state.submit(); state.submit();
  assert.equal(state.calls.length, 1);
  state.props.details = details("server-b");
  assert.equal(state.render().input("enabled").props.checked, false);
  resolve(); await settle();
  assert.equal(state.render().input("enabled").props.checked, false);
  assert.doesNotMatch(state.render().html, /servers.backups.policySaved/);
  state.unmount();
});

test("recovery save errors retain the draft for retry and reload adopts the latest policy", async () => {
  let reject = true;
  const state = harness(async () => { if (reject) throw new Error("Permission denied"); });
  state.edit("enabled", true); state.edit("maxRestarts", "4"); state.edit("waitSeconds", "0.125");
  state.submit(); await settle();
  assert.match(state.render().html, /Permission denied/);
  assert.equal(state.render().input("maxRestarts").props.value, "4");
  assert.equal(state.render().input("waitSeconds").props.value, "0.125");
  assert.equal(state.render().submit.props.disabled, false);
  reject = false; state.submit(); await settle();
  assert.equal(state.calls.length, 2);
  assert.equal(JSON.parse(state.calls[1].input.settings_json).runtime_restart.backoff_ms, 125);
  assert.equal(state.render().submit.props.disabled, true);
  state.edit("maxRestarts", "5");
  state.props.details = details("server-a", { runtime_restart: { ...defaults, max_restarts: 7 } });
  const reload = descendants(state.render().root).find((node) => node.type === "button" && node.props.type === "button");
  reload.props.onClick();
  assert.equal(state.render().input("enabled").props.checked, false);
  assert.equal(state.render().submit.props.disabled, true);
  state.edit("enabled", true);
  assert.equal(state.render().input("maxRestarts").props.value, "7");
});

test("recovery success retains a refreshed full settings object for subsequent CAS saves", async () => {
  let resolve;
  const state = harness(() => new Promise((done) => { resolve = done; }));
  state.edit("enabled", true); state.submit();
  const refreshed = { server_name: "Fresh name", runtime_restart: { ...defaults, enabled: true } };
  state.props.details = details("server-a", refreshed);
  state.render(); resolve(); await settle();
  state.edit("maxRestarts", "4"); state.submit();
  assert.equal(state.calls.length, 2);
  assert.equal(JSON.parse(state.calls[1].input.settings_json).server_name, "Fresh name");
  assert.equal(state.calls[1].options.expectedSettingsJson, state.props.details.settings_json);
  resolve(); await settle();
});

test("recovery late failures after an instance switch or unmount do not change the new editor", async () => {
  let reject;
  const state = harness(() => new Promise((_, fail) => { reject = fail; }));
  state.edit("enabled", true); state.submit();
  state.props.details = details("server-b"); state.render();
  reject(new Error("Old instance failure")); await settle();
  assert.doesNotMatch(state.render().html, /Old instance failure/);
  assert.equal(state.render().input("enabled").props.checked, false);
  state.edit("enabled", true); state.submit(); state.unmount();
  reject(new Error("Unmounted failure")); await settle();
});

test("recovery batches changes to different controls without losing earlier draft values", async () => {
  const state = harness();
  state.edit("enabled", true);
  const frame = state.render();
  frame.input("enabled").props.onChange({ target: { checked: true } });
  frame.input("maxRestarts").props.onChange({ target: { value: "6" } });
  frame.input("waitSeconds").props.onChange({ target: { value: "2.5" } });
  frame.input("onlyNonzeroExit").props.onChange({ target: { checked: false } });
  state.submit(); await settle();
  assert.deepEqual(JSON.parse(state.calls[0].input.settings_json).runtime_restart,
    { enabled: true, max_restarts: 6, backoff_ms: 2500, only_nonzero_exit: false });
});


test("recovery limits stay visible and editable before enabling the policy", async () => {
  const state = harness();
  assert.equal(state.render().input("maxRestarts").props.disabled, false);
  assert.equal(state.render().input("waitSeconds").props.disabled, false);
  assert.equal(state.render().submit.props.disabled, true);
  state.edit("enabled", true);
  state.edit("maxRestarts", "6");
  state.edit("waitSeconds", "2.5");
  state.edit("enabled", false);
  assert.equal(state.render().input("maxRestarts").props.disabled, false);
  state.edit("enabled", true);
  assert.equal(state.render().input("maxRestarts").props.value, "6");
  assert.equal(state.render().input("waitSeconds").props.value, "2.5");
  state.submit(); await settle();
  assert.deepEqual(JSON.parse(state.calls[0].input.settings_json).runtime_restart,
    { enabled: true, max_restarts: 6, backoff_ms: 2500, only_nonzero_exit: true });
});


test("turning recovery off after invalid edits keeps correction and saving reachable", async () => {
  const state = harness();
  state.edit("enabled", true);
  state.edit("maxRestarts", "");
  state.edit("enabled", false);
  assert.equal(state.render().submit.props.disabled, true);
  assert.equal(state.render().input("maxRestarts").props.disabled, false);
  state.edit("maxRestarts", "4");
  assert.equal(state.render().submit.props.disabled, false);
  state.submit(); await settle();
  assert.equal(state.calls.length, 1);
  assert.deepEqual(JSON.parse(state.calls[0].input.settings_json).runtime_restart,
    { enabled: false, max_restarts: 4, backoff_ms: 5000, only_nonzero_exit: true });
});

test("archived recovery shows missing values instead of live defaults and forbids writes", async () => {
  const state = harness();
  state.props.readOnly = true;
  let view = state.render();
  assert.equal(view.input("enabled").props.hidden, true);
  assert.equal(view.input("onlyNonzeroExit").props.hidden, true);
  assert.equal(view.input("maxRestarts").props.value, "");
  assert.equal(view.input("waitSeconds").props.value, "");
  assert.match(view.html, /servers.archives.configuration.notSaved/);
  state.edit("enabled", true);
  state.edit("maxRestarts", "8");
  state.submit();
  await settle();
  assert.deepEqual(state.calls, []);
  state.props.details.settings_json = JSON.stringify({ runtime_restart: { enabled: false, only_nonzero_exit: false, backoff_ms: 2700 } });
  state.props.savedCrashRestartLimit = 6;
  view = state.render();
  assert.equal(view.input("enabled").props.hidden, false);
  assert.equal(view.input("enabled").props.checked, false);
  assert.equal(view.input("onlyNonzeroExit").props.hidden, false);
  assert.equal(view.input("onlyNonzeroExit").props.checked, false);
  assert.equal(view.input("maxRestarts").props.value, "6");
  assert.equal(view.input("waitSeconds").props.value, "2.7");
  state.unmount();
});

test("archived recovery preserves null sources without alias or metadata fallback", async () => {
  const state = harness();
  state.props.readOnly = true;
  state.props.savedCrashRestartLimit = 6;
  state.props.details.settings_json = JSON.stringify({
    runtime_restart: { enabled: null, max_restarts: null, backoff_ms: null, only_nonzero_exit: null },
    restart_policy: { enabled: true, max_restarts: 4, backoff_ms: 1200, only_nonzero_exit: true },
    auto_restart_enabled: true, crash_restart_limit: 5
  });
  const view = state.render();
  for (const name of ["enabled", "onlyNonzeroExit"]) assert.equal(view.input(name).props.hidden, true);
  for (const name of ["maxRestarts", "waitSeconds"]) {
    assert.equal(view.input(name).props.type, "text");
    assert.equal(view.input(name).props.readOnly, true);
    assert.equal(view.input(name).props.value, "null");
  }
  assert.equal((view.html.match(/>null</g) ?? []).length, 2, "raw null stays visible in each boolean label");
  assert.doesNotMatch(view.html, />servers.archives.configuration.notSaved</);
  state.submit(); await settle();
  assert.deepEqual(state.calls, []);
  state.unmount();
});

test("archived recovery ignores obsolete JSON keys while retaining current maintenance metadata", () => {
  const state = harness();
  state.props.readOnly = true;
  state.props.savedCrashRestartLimit = 6;
  state.props.details.settings_json = JSON.stringify({
    restart_policy: { enabled: true, max_restarts: 4, backoff_ms: 1200, only_nonzero_exit: true },
    auto_restart_enabled: true, crash_restart_limit: 5, auto_restart_backoff_ms: 2000,
    auto_restart_only_nonzero_exit: false
  });
  const view = state.render();
  assert.equal(view.input("enabled").props.hidden, true);
  assert.equal(view.input("onlyNonzeroExit").props.hidden, true);
  assert.equal(view.input("maxRestarts").props.value, "6");
  assert.equal(view.input("waitSeconds").props.value, "");
  state.unmount();
});

test("archived recovery distinguishes valid booleans, empty saved text and missing fields", () => {
  const state = harness();
  state.props.readOnly = true;
  state.props.savedCrashRestartLimit = 7;
  for (const [enabled, onlyNonzeroExit, checked, exitChecked] of [
    [true, false, true, false], [" yes ", " OFF ", true, false], [" 0 ", " On ", false, true]
  ]) {
    state.props.details.settings_json = JSON.stringify({ runtime_restart: { enabled, only_nonzero_exit: onlyNonzeroExit } });
    const view = state.render();
    assert.equal(view.input("enabled").props.hidden, false);
    assert.equal(view.input("enabled").props.checked, checked);
    assert.equal(view.input("onlyNonzeroExit").props.hidden, false);
    assert.equal(view.input("onlyNonzeroExit").props.checked, exitChecked);
    assert.equal(view.input("maxRestarts").props.value, "7", "metadata supplies the count only when all saved count keys are missing");
  }
  state.props.details.settings_json = JSON.stringify({ runtime_restart: { enabled: "", only_nonzero_exit: "unknown", max_restarts: 0, backoff_ms: 0 } });
  let view = state.render();
  assert.equal(view.input("enabled").props.hidden, true);
  assert.equal(view.input("onlyNonzeroExit").props.hidden, true);
  assert.match(view.html, /&quot;&quot;/);
  assert.match(view.html, />unknown</);
  assert.equal(view.input("maxRestarts").props.value, "0");
  assert.equal(view.input("waitSeconds").props.value, "0");
  state.props.savedCrashRestartLimit = null;
  state.props.details.settings_json = "{}";
  view = state.render();
  assert.equal(view.input("maxRestarts").props.value, "null", "saved metadata null is distinct from absent metadata");
  assert.equal(view.input("enabled").props.hidden, true);
  assert.match(view.html, />servers.archives.configuration.notSaved</);
  state.unmount();
});
