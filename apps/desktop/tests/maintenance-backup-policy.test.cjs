const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
// Browser coverage verifies styles; these Node tests exercise policy state and persistence.
require.extensions[".css"] = () => {};
require.extensions[".tsx"] = require.extensions[".ts"];
const { InstanceSettingsSaveCoordinator } = require("../src/views/settings/instance-settings-save-coordinator.ts");
const settle = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function details(id = "server-a", overrides = {}) {
  return {
    summary: { id, module_id: "barotrauma", bind_ip: "127.0.0.1", autostart: true },
    auto_backup_on_stop: false, backup_retention_count: 3,
    settings_json: '{"server_name":"Preserve this configuration"}',
    ports: [{ name: "game", port: 27015, protocol: "udp" }],
    ...overrides
  };
}
function descendants(node) {
  if (!React.isValidElement(node)) return [];
  return [node, ...React.Children.toArray(node.props.children).flatMap(descendants)];
}

function harness(save = async () => {}) {
  const coordinator = new InstanceSettingsSaveCoordinator();
  const hooks = [];
  const effects = [];
  const calls = [];
  let cursor = 0;
  let writes = 0;
  const equal = (left, right) => left?.length === right?.length && left.every((value, index) => Object.is(value, right[index]));
  const react = {
    ...React,
    useId: () => "policy-validation",
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
      if (!equal(hooks[slot]?.deps, deps)) hooks[slot] = { value: create(), deps };
      return hooks[slot].value;
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
      hooks[slot] = { deps, create };
      effects.push(() => { previous?.cleanup?.(); hooks[slot].cleanup = create(); });
    }
  };
  const filename = path.resolve(__dirname, "../src/views/servers/SavePolicyEditor.tsx");
  const loaded = new Module(filename, module);
  loaded.filename = filename;
  const requireFromFile = Module.createRequire(filename);
  loaded.require = (id) => id === "react" ? react
    : id === "../settings/InstanceSettingsSaveContext" ? { useInstanceSettingsSaveCoordinator: () => coordinator }
      : requireFromFile(id);
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  const props = {
    details: details(), moduleDetails: null, locale: "en-US", t: (key, _params, fallback) => fallback ?? key,
    onSaveSettings(input, options) { calls.push({ input, options }); return save(input, options); }
  };
  function render() {
    let root;
    for (let attempt = 0; attempt < 10; attempt++) {
      cursor = 0;
      const before = writes;
      root = loaded.exports.SavePolicyEditor(props);
      while (effects.length) effects.shift()();
      if (before === writes) break;
      assert.ok(attempt < 9, "component effects must settle");
    }
    const nodes = descendants(root);
    return {
      root, html: () => renderToStaticMarkup(root),
      retention: nodes.find((node) => node.type === "input" && node.props.type === "number"),
      toggle: nodes.find((node) => node.type === "input" && node.props.type === "checkbox"),
      submit: nodes.find((node) => node.type === "button" && node.props.type === "submit"),
      native: nodes.find((node) => typeof node.type === "function" && node.type.name === "NativeSavePolicyFields"),
      reload: nodes.find((node) => node.type === "button" && node.props.type === "button")
    };
  }
  return {
    props, calls, render, coordinator, get writes() { return writes; },
    change(value) { render().retention.props.onChange({ target: { value } }); },
    toggle(checked) { render().toggle.props.onChange({ target: { checked } }); },
    submit() { render().root.props.onSubmit({ preventDefault() {} }); },
    unmount() { for (const hook of hooks) hook?.cleanup?.(); }
  };
}

function selectNativePolicy(state, moduleId = "valheim", settings = {}) {
  state.props.details = details("server-a", {
    summary: { ...details().summary, module_id: moduleId },
    settings_json: JSON.stringify({ server_name: "Keep", mod_ids: ["123"], ...settings })
  });
  state.props.moduleDetails = {
    summary: { id: moduleId, name: moduleId },
    schema_json: fs.readFileSync(path.resolve(__dirname, `../../../modules/${moduleId}/schema.json`), "utf8")
  };
  state.render();
}

function editNative(state, key, value) {
  const native = state.render().native.props;
  const field = native.fields.find((item) => item.key === key);
  assert.ok(field, `missing native control: ${key}`);
  native.onChange(field, value);
}

test("native autosave and backup policy save together without replacing unrelated settings", async () => {
  const state = harness();
  selectNativePolicy(state);
  editNative(state, "save_interval_seconds", "1200");
  editNative(state, "backup_count", "6");
  state.toggle(true);
  state.change("8");
  assert.equal(state.calls.length, 0);
  state.submit();
  await settle();
  assert.deepEqual(JSON.parse(state.calls[0].input.settings_json), {
    server_name: "Keep", mod_ids: ["123"], save_interval_seconds: 1200, backup_count: 6
  });
  assert.equal(state.calls[0].input.auto_backup_on_stop, true);
  assert.equal(state.calls[0].input.backup_retention_count, 8);
  assert.equal(state.calls[0].input.autostart, undefined);
  assert.equal(state.render().submit.props.disabled, true);
  state.unmount();
});

test("invalid native intervals block both native and manager policy persistence", () => {
  const state = harness();
  selectNativePolicy(state);
  for (const value of ["-1", "0.5", "Infinity", "abc"]) {
    editNative(state, "save_interval_seconds", value);
    state.toggle(true);
    assert.ok(state.render().native.props.issues.some((issue) => issue.fieldKey === "save_interval_seconds"));
    assert.equal(state.render().submit.props.disabled, true);
    state.submit();
    assert.equal(state.calls.length, 0);
  }
  state.unmount();
});

test("missing or malformed native definitions block saving without fabricating an empty policy", () => {
  const state = harness();
  selectNativePolicy(state);
  for (const schema_json of [null, "{broken", '{}']) {
    state.props.moduleDetails = { ...state.props.moduleDetails, schema_json };
    state.change("8");
    assert.equal(state.render().submit.props.disabled, true);
    state.submit();
  }
  assert.equal(state.calls.length, 0);
  state.unmount();
});

test("native policy merges concurrent settings and follows refreshed untouched policy fields", async () => {
  const state = harness();
  selectNativePolicy(state, "valheim", { save_interval_seconds: 1800, backup_count: 4 });
  editNative(state, "save_interval_seconds", "900");
  state.props.details = { ...state.props.details, auto_backup_on_stop: true, backup_retention_count: 9,
    settings_json: JSON.stringify({ server_name: "Changed elsewhere", backup_count: 7, save_interval_seconds: 1800 }) };
  assert.equal(state.render().native.props.settings.backup_count, 7);
  assert.equal(state.render().toggle.props.checked, true);
  editNative(state, "backup_count", "8");
  state.submit();
  await settle();
  assert.deepEqual(JSON.parse(state.calls[0].input.settings_json), {
    server_name: "Changed elsewhere", backup_count: 8, save_interval_seconds: 900
  });
  assert.equal(state.calls[0].input.auto_backup_on_stop, true);
  assert.equal(state.calls[0].input.backup_retention_count, 9);
  assert.equal(state.calls[0].options.expectedSettingsJson, state.props.details.settings_json);
  state.unmount();
});

test("same-field native conflicts retain the draft until the user reloads current policy", async () => {
  const state = harness();
  selectNativePolicy(state, "valheim", { save_interval_seconds: 1800 });
  editNative(state, "save_interval_seconds", "900");
  state.props.details = { ...state.props.details, settings_json: '{"save_interval_seconds":600}' };
  state.submit();
  await settle();
  assert.equal(state.calls.length, 0);
  assert.equal(state.render().native.props.settings.save_interval_seconds, 900);
  assert.match(state.render().html(), /servers.savePolicy.conflict/);
  await assert.rejects(state.coordinator.flush("server-a"), /servers.savePolicy.conflict/);
  state.render().reload.props.onClick();
  await state.coordinator.flush("server-a");
  assert.equal(state.render().native.props.settings.save_interval_seconds, 600);
  assert.equal(state.render().submit.props.disabled, true);
  state.unmount();
});

test("malformed current JSON never gets replaced by a save-policy draft", () => {
  const state = harness();
  state.props.details = { ...state.props.details, settings_json: "{broken" };
  state.change("8");
  state.submit();
  assert.equal(state.render().submit.props.disabled, true);
  assert.equal(state.calls.length, 0);
  state.unmount();
});

test("a confirmed native save remains visible and becomes the next CAS baseline if panel refresh fails", async () => {
  const state = harness();
  selectNativePolicy(state, "valheim", { save_interval_seconds: 1800 });
  editNative(state, "save_interval_seconds", "900");
  state.submit();
  await settle();
  assert.equal(state.render().native.props.settings.save_interval_seconds, 900);
  assert.match(state.render().html(), /Policy saved/);
  editNative(state, "save_interval_seconds", "600");
  state.submit();
  await settle();
  assert.equal(state.calls.length, 2);
  assert.equal(state.calls[1].options.expectedSettingsJson, state.calls[0].input.settings_json);
  assert.equal(state.render().native.props.settings.save_interval_seconds, 600);
  state.unmount();
});

test("refreshing untouched native values during a save does not create a false conflict on the next edit", async () => {
  const gate = deferred();
  const state = harness(() => gate.promise);
  selectNativePolicy(state, "valheim", { save_interval_seconds: 1800, backup_count: 4 });
  editNative(state, "save_interval_seconds", "900");
  state.submit();
  state.props.details = { ...state.props.details, settings_json: '{"save_interval_seconds":900,"backup_count":8}' };
  state.render();
  gate.resolve();
  await settle();
  assert.equal(state.render().native.props.settings.backup_count, 8);
  editNative(state, "backup_count", "9");
  state.submit();
  await settle();
  assert.equal(state.calls.length, 2);
  assert.equal(JSON.parse(state.calls[1].input.settings_json).backup_count, 9);
  state.unmount();
});

test("native module cross-field rules still block an invalid backup interval pair", () => {
  const state = harness();
  selectNativePolicy(state);
  editNative(state, "backup_long_seconds", "60");
  assert.equal(state.render().submit.props.disabled, true);
  assert.ok(state.render().native.props.issues.some((issue) => issue.fieldKey === "backup_long_seconds"));
  state.submit();
  assert.equal(state.calls.length, 0);
  state.unmount();
});

test("manager-only refreshes keep an acknowledged native baseline after a failed panel refresh", async () => {
  const state = harness();
  selectNativePolicy(state, "valheim", { save_interval_seconds: 1800 });
  editNative(state, "save_interval_seconds", "900");
  state.submit();
  await settle();
  state.props.details = { ...state.props.details, backup_retention_count: 8 };
  assert.equal(state.render().native.props.settings.save_interval_seconds, 900);
  assert.equal(state.render().retention.props.value, "8");
  editNative(state, "save_interval_seconds", "600");
  state.submit();
  await settle();
  assert.equal(state.calls[1].options.expectedSettingsJson, state.calls[0].input.settings_json);
  assert.equal(state.calls[1].input.backup_retention_count, 8);
  state.unmount();
});

test("manager policy refreshed during native saving is kept for the next transaction", async () => {
  const gate = deferred();
  const state = harness(() => gate.promise);
  selectNativePolicy(state);
  editNative(state, "save_interval_seconds", "900");
  state.submit();
  state.props.details = { ...state.props.details, backup_retention_count: 8, auto_backup_on_stop: true };
  state.render();
  gate.resolve();
  await settle();
  assert.equal(state.render().retention.props.value, "8");
  assert.equal(state.render().toggle.props.checked, true);
  editNative(state, "save_interval_seconds", "600");
  state.submit();
  await settle();
  assert.equal(state.calls[1].input.backup_retention_count, 8);
  assert.equal(state.calls[1].input.auto_backup_on_stop, true);
  state.unmount();
});

test("policy edits submit explicitly and preserve the current instance configuration", async () => {
  const state = harness();
  assert.equal(state.render().submit.props.disabled, true);
  state.change("8");
  state.toggle(true);
  assert.deepEqual(state.calls, [], "editing must not send partial policy updates");
  assert.equal(state.render().submit.props.disabled, false);
  state.submit();
  await settle();
  assert.deepEqual(state.calls, [{
    input: {
      id: "server-a", bind_ip: "127.0.0.1",
      auto_backup_on_stop: true, backup_retention_count: 8,
      settings_json: state.props.details.settings_json, ports: state.props.details.ports
    },
    options: { expectedSettingsJson: state.props.details.settings_json, silent: true, throwOnError: true }
  }]);
  assert.equal(state.render().submit.props.disabled, true);
  assert.match(state.render().html(), /Policy saved/);
  state.unmount();
});

for (const commitInvalidDraft of [false, true]) {
  test(`rapid retention and toggle edits preserve both values with invalid draft ${commitInvalidDraft ? "committed" : "batched"}`, async () => {
    const state = harness();
    state.props.details = details("server-a", { backup_retention_count: 10 });
    let view = state.render();
    view.retention.props.onChange({ target: { value: "0" } });
    if (commitInvalidDraft) {
      view = state.render();
      assert.equal(view.retention.props["aria-invalid"], true);
    }
    // Dispatch both changes from one committed view, before React rerenders.
    view.retention.props.onChange({ target: { value: "3" } });
    view.toggle.props.onChange({ target: { checked: true } });

    const updated = state.render();
    assert.equal(updated.retention.props.value, "3");
    assert.equal(updated.retention.props["aria-invalid"], false);
    assert.equal(updated.toggle.props.checked, true);
    assert.equal(updated.submit.props.disabled, false);
    state.submit();
    await settle();
    assert.equal(state.calls.length, 1);
    assert.equal(state.calls[0].input.backup_retention_count, 3);
    assert.equal(state.calls[0].input.auto_backup_on_stop, true);
    assert.match(state.render().html(), /Policy saved/);
    state.unmount();
  });
}

test("invalid and out-of-range retention never reaches storage or silently truncates", () => {
  const state = harness();
  for (const value of ["", "0", "-1", "1.5", "2e3", "NaN", "Infinity", "4294967296"]) {
    state.change(value);
    const view = state.render();
    assert.equal(view.retention.props["aria-invalid"], true, value);
    assert.equal(view.submit.props.disabled, true, value);
    assert.match(view.html(), /role="alert"/, value);
    state.submit();
  }
  assert.deepEqual(state.calls, []);
  state.change("4294967295");
  assert.equal(state.render().retention.props["aria-invalid"], false);
  state.change("0003");
  assert.equal(state.render().submit.props.disabled, true, "equivalent integer values are not dirty");
  state.unmount();
});

test("an in-flight save disables both fields and rejects duplicate submissions before rerender", async () => {
  const gate = deferred();
  const state = harness(() => gate.promise);
  state.change("6");
  const form = state.render().root;
  form.props.onSubmit({ preventDefault() {} });
  form.props.onSubmit({ preventDefault() {} });
  assert.equal(state.render().retention.props.disabled, true);
  assert.equal(state.render().toggle.props.disabled, true);
  assert.match(state.render().html(), /Saving policy/);
  state.change("9");
  state.toggle(true);
  assert.equal(state.render().retention.props.value, "6");
  assert.equal(state.render().toggle.props.checked, false);
  await settle();
  assert.equal(state.calls.length, 1);
  gate.resolve();
  await settle();
  assert.equal(state.render().retention.props.disabled, false);
  state.unmount();
});

test("failed saves retain the draft and expose the cause with a working retry", async () => {
  let failing = true;
  const state = harness(async () => { if (failing) throw new Error("settings changed while this edit was pending"); });
  state.change("7");
  state.submit();
  await settle();
  assert.equal(state.render().retention.props.value, "7");
  assert.equal(state.render().submit.props.disabled, false);
  assert.match(state.render().html(), /Unable to save policy: settings changed while this edit was pending/);
  assert.doesNotMatch(state.render().html(), /Policy saved/);
  failing = false;
  state.submit();
  await settle();
  assert.equal(state.calls.length, 2);
  assert.match(state.render().html(), /Policy saved/);
  assert.equal(state.render().submit.props.disabled, true);
  state.unmount();
});

for (const outcome of ["resolve", "reject"]) {
  test(`a ${outcome} from the previous instance cannot replace the newly selected draft`, async () => {
    const gate = deferred();
    const state = harness(() => gate.promise);
    state.change("9");
    state.submit();
    state.props.details = details("server-b", { auto_backup_on_stop: true, backup_retention_count: 5 });
    assert.equal(state.render().retention.props.value, "5");
    assert.equal(state.render().toggle.props.checked, true);
    assert.equal(state.render().retention.props.disabled, false);
    state.change("12");
    gate[outcome](new Error("old instance failure"));
    await settle();
    assert.equal(state.render().retention.props.value, "12");
    assert.equal(state.render().submit.props.disabled, false);
    assert.doesNotMatch(state.render().html(), /Policy saved|old instance failure/);
    state.unmount();
  });
}

test("refreshes update clean policies, retain dirty drafts, and submit the latest configuration baseline", async () => {
  const state = harness();
  state.render();
  state.props.details = details("server-a", { backup_retention_count: 5 });
  assert.equal(state.render().retention.props.value, "5");
  state.change("8");
  state.props.details = details("server-a", { backup_retention_count: 6, settings_json: '{"server_name":"Updated elsewhere"}' });
  assert.equal(state.render().retention.props.value, "8");
  state.submit();
  await settle();
  assert.equal(state.calls[0].input.settings_json, state.props.details.settings_json);
  assert.equal(state.calls[0].options.expectedSettingsJson, state.props.details.settings_json);
  state.unmount();
});

test("same-instance details refreshing between field events preserves the unsaved policy", async () => {
  const state = harness();
  state.props.details = details("server-a", { backup_retention_count: 10 });
  const previousView = state.render();
  previousView.retention.props.onChange({ target: { value: "3" } });
  state.props.details = details("server-a", {
    backup_retention_count: 8,
    settings_json: '{"server_name":"Refreshed details"}'
  });
  assert.equal(state.render().retention.props.value, "3");
  previousView.toggle.props.onChange({ target: { checked: true } });
  assert.equal(state.render().retention.props.value, "3");
  state.submit();
  await settle();
  assert.equal(state.calls[0].input.backup_retention_count, 3);
  assert.equal(state.calls[0].input.auto_backup_on_stop, true);
  assert.equal(state.calls[0].input.settings_json, state.props.details.settings_json);
  state.unmount();
});

test("an event retained from the previous instance does not replace the current instance draft", () => {
  const state = harness();
  const previousView = state.render();
  state.props.details = details("server-b", { backup_retention_count: 5 });
  state.change("9");
  previousView.toggle.props.onChange({ target: { checked: true } });
  assert.equal(state.render().retention.props.value, "9");
  assert.equal(state.render().toggle.props.checked, false);
  assert.equal(state.render().submit.props.disabled, false);
  state.unmount();
});

test("a save completing after unmount does not update component state", async () => {
  const gate = deferred();
  const state = harness(() => gate.promise);
  state.change("8");
  state.submit();
  state.unmount();
  const before = state.writes;
  gate.resolve();
  await settle();
  assert.equal(state.writes, before);
});

test("archived save policy keeps its existing controls with exact saved values and no mutation", async () => {
  const state = harness();
  selectNativePolicy(state, "valheim", { save_interval_seconds: 37 });
  state.props.readOnly = true;
  const before = structuredClone(state.props.details);
  const view = state.render();
  assert.equal(view.retention.props.disabled, true);
  assert.equal(view.native.props.readOnly, true);
  assert.equal(view.native.props.settings.save_interval_seconds, 37);
  assert.equal(Object.hasOwn(view.native.props.settings, "backup_count"), false);
  state.change("12");
  state.toggle(true);
  editNative(state, "save_interval_seconds", "2400");
  state.submit();
  await settle();
  assert.deepEqual(state.calls, []);
  assert.deepEqual(state.props.details, before);
  assert.equal(state.render().native.props.settings.save_interval_seconds, 37);
  state.unmount();
});

test("starting an instance waits for a native save-policy write after the editor unmounts", async () => {
  const gate = deferred();
  const state = harness(() => gate.promise);
  selectNativePolicy(state);
  editNative(state, "save_interval_seconds", "900");
  state.submit();
  state.unmount();
  let started = false;
  const start = state.coordinator.flush("server-a").then(() => { started = true; });
  await settle();
  assert.equal(started, false, "start must wait for native settings persistence");
  gate.resolve();
  await start;
  assert.equal(started, true);
});

test("failed policy writes keep the start barrier until retry succeeds", async () => {
  let fail = true;
  const state = harness(async () => { if (fail) throw new Error("policy write failed"); });
  state.change("9");
  state.submit();
  await settle();
  await assert.rejects(state.coordinator.flush("server-a"), /policy write failed/);
  fail = false;
  state.submit();
  await settle();
  await state.coordinator.flush("server-a");
  state.unmount();
});

test("discard acknowledges only the settled policy failure and leaves other pending writes tracked", async () => {
  const state = harness(async () => { throw new Error("discarded policy failure"); });
  state.change("9");
  state.submit();
  await settle();
  await assert.rejects(state.coordinator.flush("server-a"), /discarded policy failure/);
  const gate = deferred();
  const other = state.coordinator.runOperation("server-a", () => gate.promise, "other-settings-editor");
  state.render().reload.props.onClick();
  let ready = false;
  const start = state.coordinator.flush("server-a").then(() => { ready = true; });
  await settle();
  assert.equal(ready, false);
  gate.resolve();
  await Promise.all([other, start]);
  assert.equal(state.render().retention.props.value, "3");
  state.unmount();
});
