const ts = require("@typescript/typescript6");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { parseSource, sourceText, transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
// Browser coverage verifies styles; these Node tests exercise policy state and persistence.
require.extensions[".css"] = () => {};
require.extensions[".tsx"] = require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const { readRecoveryPolicy, recoveryDraft, parseRecoveryDraft, mergeRecoveryPolicy } = require("../src/views/servers/runtime-recovery-model.ts");
const defaults = { enabled: false, max_restarts: 3, backoff_ms: 5000, only_nonzero_exit: true };

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

const { createAutosaveHarness, deferred, pendingDelays, runTimers } = require("./helpers/maintenance-autosave-harness.cjs");

function details(id = "server-a", settings = { server_name: "Keep" }) {
  return { summary: { id, bind_ip: "127.0.0.1" }, auto_backup_on_stop: true, backup_retention_count: 7,
    settings_json: JSON.stringify(settings), ports: [{ name: "game", port: 7777, protocol: "udp" }] };
}
/** Saves resolve with the refreshed record, which the parent then passes back like the real workbench does. */
function harness(save) {
  const calls = [];
  const props = { details: details(), t: (key, params) => params?.message ? `${key}: ${params.message}` : key };
  props.onSaveSettings = (input, options) => {
    calls.push({ input, options });
    const persist = () => {
      const saved = { ...props.details, summary: { ...props.details.summary, id: input.id }, settings_json: input.settings_json };
      if (props.details.summary.id === input.id) props.details = saved;
      return saved;
    };
    return save ? save(input, options).then(persist) : Promise.resolve(persist());
  };
  const state = createAutosaveHarness({ editorPath: "../../src/views/servers/RuntimeRecoveryEditor.tsx",
    exportName: "RuntimeRecoveryEditor", props });
  return { ...state, calls, policy: (index) => JSON.parse(calls[index].input.settings_json).runtime_restart };
}

test("recovery editor has no save button and auto-saves toggles immediately with CAS while keeping concurrent edits", async () => {
  const state = harness();
  const initial = state.render();
  const exitControl = initial.input("onlyNonzeroExit");
  const exitDescription = initial.nodes.find((node) => node.props.id === exitControl.props["aria-describedby"]);
  assert.equal(exitControl.props.title, undefined);
  assert.equal(exitDescription?.props.children, "servers.recovery.exitBehavior", "Recovery help remains associated with its checkbox");
  assert.equal(initial.submit, undefined, "auto-save replaces the redundant save button");
  assert.equal(initial.state, "saved");
  state.edit("enabled", true);
  assert.deepEqual(pendingDelays(), [0], "toggles save without a typing delay");
  state.props.details = details("server-a", { server_name: "Concurrent", mods: ["123"] });
  const concurrent = state.props.details;
  state.render();
  await runTimers();
  assert.equal(state.calls.length, 1);
  const { input, options } = state.calls[0];
  assert.deepEqual(JSON.parse(input.settings_json), { server_name: "Concurrent", mods: ["123"], runtime_restart: { ...defaults, enabled: true } });
  assert.equal(input.auto_backup_on_stop, true); assert.equal(input.backup_retention_count, 7);
  assert.deepEqual(input.ports, concurrent.ports);
  assert.equal(options.expectedSettingsJson, concurrent.settings_json);
  const saved = state.render();
  assert.equal(saved.input("enabled").props.checked, true, "the saved policy survives the refresh");
  assert.equal(saved.state, "saved");
  assert.match(saved.html(), /servers.backups.policySaved/);
});

test("recovery number fields debounce typing and the latest edit wins over in-flight saves", async () => {
  const held = [];
  const state = harness(() => { const gate = deferred(); held.push(gate); return gate.promise; });
  state.edit("maxRestarts", "4");
  assert.deepEqual(pendingDelays(), [650], "typed numbers wait for a pause");
  await runTimers();
  assert.equal(state.calls.length, 1);
  assert.equal(state.render().state, "saving");
  state.edit("maxRestarts", "5"); state.edit("maxRestarts", "6");
  held.shift().resolve(); await runTimers();
  assert.equal(state.render().input("maxRestarts").props.value, "6", "a refresh echoing the earlier write keeps the newer draft");
  assert.doesNotMatch(state.render().html(), /servers.recovery.conflict/, "own earlier write is not a conflict");
  await runTimers();
  assert.equal(state.calls.length, 2, "intermediate values are coalesced");
  assert.equal(state.policy(1).max_restarts, 6);
  assert.equal(state.calls[1].options.expectedSettingsJson, state.calls[0].input.settings_json);
  held.shift().resolve(); await runTimers();
  assert.equal(state.render().state, "saved");
  assert.equal(state.render().input("maxRestarts").props.value, "6");
});

test("recovery validation blocks writes and same-field external changes are never overwritten", async () => {
  const state = harness();
  state.edit("enabled", true); state.edit("maxRestarts", "0");
  await runTimers();
  assert.equal(state.calls.length, 0, "invalid drafts are not written");
  let view = state.render();
  assert.equal(view.input("maxRestarts").props["aria-invalid"], true);
  assert.equal(view.state, "blocked");
  await assert.rejects(state.coordinator.flush("server-a"), /invalid/i, "start barriers refuse an invalid pending draft");
  state.edit("maxRestarts", "4");
  state.props.details = details("server-a", { runtime_restart: { ...defaults, max_restarts: 5 } });
  view = state.render();
  await runTimers();
  assert.equal(state.calls.length, 0, "a policy edited elsewhere is not silently overwritten");
  assert.equal(view.state, "blocked");
  assert.match(view.html(), /servers.recovery.conflict/);
  view.discard.props.onClick();
  view = state.render();
  assert.equal(view.input("maxRestarts").props.value, "5", "discard adopts the external policy");
  assert.equal(view.input("enabled").props.checked, false);
  assert.equal(view.state, "saved");
  await runTimers();
  assert.equal(state.calls.length, 0);
});

test("recovery save failures keep the draft with retry and discard actions", async () => {
  let reject = true;
  const state = harness(async () => { if (reject) throw new Error("Permission denied"); });
  state.edit("enabled", true); state.edit("maxRestarts", "4"); state.edit("waitSeconds", "0.125");
  await runTimers();
  let view = state.render();
  assert.equal(view.state, "failed");
  assert.match(view.html(), /Permission denied/);
  assert.equal(view.input("maxRestarts").props.value, "4");
  assert.equal(view.input("waitSeconds").props.value, "0.125");
  assert.ok(view.retry && view.discard);
  reject = false; view.retry.props.onClick(); await runTimers();
  assert.equal(state.calls.length, 2);
  assert.deepEqual(state.policy(1), { enabled: true, max_restarts: 4, backoff_ms: 125, only_nonzero_exit: true });
  view = state.render();
  assert.equal(view.state, "saved");
  assert.equal(view.retry, undefined);
  reject = true; state.edit("maxRestarts", "5"); await runTimers();
  state.render().discard.props.onClick();
  view = state.render();
  assert.equal(view.input("maxRestarts").props.value, "4", "discard returns to the persisted policy");
  assert.equal(view.state, "saved");
});

test("recovery flushes pending edits on instance switch and unmount without leaking into the next instance", async () => {
  const state = harness();
  state.edit("maxRestarts", "8");
  state.props.details = details("server-b");
  let view = state.render();
  await runTimers();
  assert.equal(state.calls.length, 1, "switching instances writes the pending draft instead of dropping it");
  assert.equal(state.calls[0].options.expectedSettingsJson, details().settings_json);
  assert.equal(state.policy(0).max_restarts, 8);
  view = state.render();
  assert.equal(view.input("maxRestarts").props.value, "3", "the next instance shows its own policy");
  state.edit("waitSeconds", "2");
  state.unmount();
  await runTimers();
  assert.equal(state.calls.length, 2, "unmount flushes the debounced draft");
  assert.equal(state.policy(1).backoff_ms, 2000);
  assert.match(state.calls[1].input.settings_json, /runtime_restart/);
});

test("recovery batches edits to different controls and keeps limits editable while disabled", async () => {
  const state = harness();
  assert.equal(state.render().input("maxRestarts").props.disabled, false);
  assert.equal(state.render().input("waitSeconds").props.disabled, false);
  const frame = state.render();
  frame.input("enabled").props.onChange({ target: { checked: true } });
  frame.input("maxRestarts").props.onChange({ target: { value: "6" } });
  frame.input("waitSeconds").props.onChange({ target: { value: "2.5" } });
  frame.input("onlyNonzeroExit").props.onChange({ target: { checked: false } });
  state.render();
  await runTimers();
  assert.equal(state.calls.length, 1);
  assert.deepEqual(state.policy(0), { enabled: true, max_restarts: 6, backoff_ms: 2500, only_nonzero_exit: false });
});

test("turning recovery off after invalid edits keeps correction and saving reachable", async () => {
  const state = harness();
  state.edit("enabled", true);
  state.edit("maxRestarts", "");
  state.edit("enabled", false);
  await runTimers();
  assert.equal(state.calls.length, 0);
  assert.equal(state.render().input("maxRestarts").props.disabled, false);
  state.edit("maxRestarts", "4");
  await runTimers();
  assert.equal(state.calls.length, 1);
  assert.deepEqual(state.policy(0), { enabled: false, max_restarts: 4, backoff_ms: 5000, only_nonzero_exit: true });
});

test("archived recovery shows missing values instead of live defaults and forbids writes", async () => {
  const state = harness();
  state.props.readOnly = true;
  let view = state.render();
  assert.equal(view.footer, undefined, "archives have no save status");
  assert.equal(view.input("enabled").props.hidden, true);
  assert.equal(view.input("onlyNonzeroExit").props.hidden, true);
  assert.equal(view.input("maxRestarts").props.value, "");
  assert.equal(view.input("waitSeconds").props.value, "");
  assert.match(view.html(), /servers.archives.configuration.notSaved/);
  state.edit("enabled", true);
  state.edit("maxRestarts", "8");
  await runTimers();
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
  state.unmount(); await runTimers();
  assert.deepEqual(state.calls, []);
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
  assert.equal((view.html().match(/>null</g) ?? []).length, 2, "raw null stays visible in each boolean label");
  assert.doesNotMatch(view.html(), />servers.archives.configuration.notSaved</);
  await runTimers();
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
  assert.match(view.html(), /&quot;&quot;/);
  assert.match(view.html(), />unknown</);
  assert.equal(view.input("maxRestarts").props.value, "0");
  assert.equal(view.input("waitSeconds").props.value, "0");
  state.props.savedCrashRestartLimit = null;
  state.props.details.settings_json = "{}";
  view = state.render();
  assert.equal(view.input("maxRestarts").props.value, "null", "saved metadata null is distinct from absent metadata");
  assert.equal(view.input("enabled").props.hidden, true);
  assert.match(view.html(), />servers.archives.configuration.notSaved</);
  state.unmount();
});
