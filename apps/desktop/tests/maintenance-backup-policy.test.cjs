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
// Browser coverage verifies styles and real timing; these Node tests exercise policy state and persistence.
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
function hasClass(node, name) {
  return typeof node.props.className === "string" && node.props.className.split(/\s+/).includes(name);
}

// The autosave hook schedules through window timers; tests run them explicitly to observe debounce delays.
const timers = new Map();
let timerId = 0;
globalThis.window = {
  setTimeout(callback, ms) { timers.set(++timerId, { callback, ms }); return timerId; },
  clearTimeout(id) { timers.delete(id); }
};
function pendingDelays() {
  return [...timers.values()].map((timer) => timer.ms);
}

function harness(save) {
  timers.clear();
  const coordinator = new InstanceSettingsSaveCoordinator();
  const hooks = [];
  const layoutEffects = [];
  const effects = [];
  const calls = [];
  let cursor = 0;
  let writes = 0;
  const equal = (left, right) => left?.length === right?.length && left.every((value, index) => Object.is(value, right[index]));
  function effectHook(queue) {
    return (create, deps) => {
      const slot = cursor++;
      if (deps && equal(hooks[slot]?.deps, deps)) return;
      const previous = hooks[slot];
      hooks[slot] = { deps, create };
      queue.push(() => { previous?.cleanup?.(); hooks[slot].cleanup = create(); });
    };
  }
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
    useCallback(callback, deps) {
      return react.useMemo(() => callback, deps);
    },
    useRef(initial) {
      const slot = cursor++;
      if (!(slot in hooks)) hooks[slot] = { current: initial };
      return hooks[slot];
    },
    useLayoutEffect: effectHook(layoutEffects),
    useEffect: effectHook(effects)
  };
  const coordinatorModule = { useInstanceSettingsSaveCoordinator: () => coordinator };
  function load(relative, mocks) {
    const filename = path.resolve(__dirname, relative);
    const loaded = new Module(filename, module);
    loaded.filename = filename;
    const requireFromFile = Module.createRequire(filename);
    loaded.require = (id) => id === "react" ? react : Object.hasOwn(mocks, id) ? mocks[id] : requireFromFile(id);
    loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
    return loaded.exports;
  }
  const autosave = load("../src/views/settings/useAutoSaveInstanceSettings.ts", { "./InstanceSettingsSaveContext": coordinatorModule });
  const editor = load("../src/views/servers/SavePolicyEditor.tsx", {
    "../settings/InstanceSettingsSaveContext": coordinatorModule,
    "../settings/useAutoSaveInstanceSettings": autosave
  });
  const props = {
    details: details(), moduleDetails: null, locale: "en-US", t: (key, _params, fallback) => fallback ?? key,
    onSaveSettings(input, options) {
      calls.push({ input, options });
      return save ? save(input, options) : Promise.resolve({ ...props.details, settings_json: input.settings_json,
        auto_backup_on_stop: input.auto_backup_on_stop, backup_retention_count: input.backup_retention_count });
    }
  };
  function render() {
    let root;
    for (let attempt = 0; attempt < 10; attempt++) {
      cursor = 0;
      const before = writes;
      root = editor.SavePolicyEditor(props);
      while (layoutEffects.length) layoutEffects.shift()();
      while (effects.length) effects.shift()();
      if (before === writes) break;
      assert.ok(attempt < 9, "component effects must settle");
    }
    const nodes = descendants(root);
    const footer = nodes.find((node) => hasClass(node, "server-backup-policy-footer"));
    return {
      root, html: () => renderToStaticMarkup(root), footer, state: footer?.props["data-save-state"],
      retention: nodes.find((node) => node.type === "input" && node.props.type === "number"),
      toggle: nodes.find((node) => node.type === "input" && node.props.type === "checkbox"),
      submit: nodes.find((node) => node.type === "button" && node.props.type === "submit"),
      native: nodes.find((node) => typeof node.type === "function" && node.type.name === "NativeSavePolicyFields"),
      retry: nodes.find((node) => node.type === "button" && hasClass(node, "secondary-button")),
      reload: nodes.find((node) => node.type === "button" && hasClass(node, "ghost-button"))
    };
  }
  /** Runs due autosave timers, lets storage settle, and renders the resulting status. */
  async function flush() {
    render();
    for (const [id, timer] of [...timers]) {
      timers.delete(id);
      timer.callback();
    }
    await settle();
    await settle();
    return render();
  }
  return {
    props, calls, render, flush, coordinator, get writes() { return writes; },
    change(value) { render().retention.props.onChange({ target: { value } }); },
    toggle(checked) { render().toggle.props.onChange({ target: { checked } }); },
    unmount() {
      for (const hook of hooks) hook?.cleanup?.();
    }
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

/** Applies a successful write to the props, as the server view does after persistence. */
function persist(state, call) {
  state.props.details = { ...state.props.details, settings_json: call.input.settings_json,
    auto_backup_on_stop: call.input.auto_backup_on_stop, backup_retention_count: call.input.backup_retention_count };
}

test("the manual save button is gone and policy edits autosave together without replacing unrelated settings", async () => {
  const state = harness();
  selectNativePolicy(state);
  assert.equal(state.render().submit, undefined, "autosave must not keep a manual save button");
  editNative(state, "save_interval_seconds", "1200");
  editNative(state, "backup_count", "6");
  state.toggle(true);
  state.change("8");
  state.render();
  assert.equal(state.calls.length, 0, "typed values wait for the debounce");
  assert.deepEqual(pendingDelays(), [650]);
  await state.flush();
  assert.equal(state.calls.length, 1, "one write carries every pending policy edit");
  assert.deepEqual(JSON.parse(state.calls[0].input.settings_json), {
    server_name: "Keep", mod_ids: ["123"], save_interval_seconds: 1200, backup_count: 6
  });
  assert.equal(state.calls[0].input.auto_backup_on_stop, true);
  assert.equal(state.calls[0].input.backup_retention_count, 8);
  assert.equal(state.calls[0].input.autostart, undefined);
  assert.deepEqual(state.calls[0].options, { expectedSettingsJson: state.props.details.settings_json, silent: true, throwOnError: true });
  assert.equal(state.render().state, "saved");
  assert.match(state.render().html(), /Policy saved/);
  state.unmount();
});

test("toggles save immediately while typed values are debounced", async () => {
  const state = harness();
  state.toggle(true);
  state.render();
  assert.deepEqual(pendingDelays(), [0], "a toggle schedules its write for the next tick");
  await state.flush();
  assert.equal(state.calls.length, 1);
  assert.equal(state.calls[0].input.auto_backup_on_stop, true);
  assert.equal(state.calls[0].input.settings_json, state.props.details.settings_json, "untouched settings keep their exact text");
  persist(state, state.calls[0]);
  state.change("7");
  state.render();
  assert.deepEqual(pendingDelays(), [650]);
  assert.equal(state.render().state, "dirty");
  assert.match(state.render().html(), /Changes waiting to save/);
  await state.flush();
  assert.equal(state.calls.length, 2);
  assert.equal(state.calls[1].input.backup_retention_count, 7);
  assert.equal(state.calls[1].input.auto_backup_on_stop, true);
  state.unmount();
});

test("native checkbox and select fields save immediately; native numbers are debounced", () => {
  const state = harness();
  selectNativePolicy(state);
  const fields = state.render().native.props.fields;
  const numeric = fields.find((field) => field.control === "number");
  assert.ok(numeric, "fixture module must expose a numeric native policy");
  editNative(state, numeric.key, "1200");
  state.render();
  assert.deepEqual(pendingDelays(), [650]);
  const discrete = fields.find((field) => field.control === "checkbox" || field.control === "select");
  if (discrete) {
    editNative(state, discrete.key, discrete.control === "checkbox" ? true : discrete.options?.[0]?.value);
    state.render();
    assert.deepEqual(pendingDelays(), [0]);
  }
  state.unmount();
});

test("invalid native intervals block both native and manager policy persistence", async () => {
  const state = harness();
  selectNativePolicy(state);
  for (const value of ["-1", "0.5", "Infinity", "abc"]) {
    editNative(state, "save_interval_seconds", value);
    state.toggle(true);
    const view = await state.flush();
    assert.ok(view.native.props.issues.some((issue) => issue.fieldKey === "save_interval_seconds"));
    assert.equal(view.state, "blocked");
    assert.match(view.html(), /Fix invalid settings to save/);
    assert.equal(state.calls.length, 0);
  }
  await assert.rejects(state.coordinator.flush("server-a"), /draft is invalid/);
  state.unmount();
});

test("missing or malformed native definitions block saving without fabricating an empty policy", async () => {
  const state = harness();
  selectNativePolicy(state);
  for (const schema_json of [null, "{broken", '{}']) {
    state.props.moduleDetails = { ...state.props.moduleDetails, schema_json };
    state.change("8");
    await state.flush();
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
  await state.flush();
  assert.equal(state.calls.length, 1);
  assert.deepEqual(JSON.parse(state.calls[0].input.settings_json), {
    server_name: "Changed elsewhere", backup_count: 8, save_interval_seconds: 900
  });
  assert.equal(state.calls[0].input.auto_backup_on_stop, true);
  assert.equal(state.calls[0].input.backup_retention_count, 9);
  assert.equal(state.calls[0].options.expectedSettingsJson, state.props.details.settings_json,
    "a record written elsewhere becomes the compare-and-swap baseline");
  state.unmount();
});

test("same-field native conflicts retain the draft unsaved until the user reloads current policy", async () => {
  const state = harness();
  selectNativePolicy(state, "valheim", { save_interval_seconds: 1800 });
  editNative(state, "save_interval_seconds", "900");
  state.props.details = { ...state.props.details, settings_json: '{"save_interval_seconds":600}' };
  const view = await state.flush();
  assert.equal(state.calls.length, 0);
  assert.equal(view.native.props.settings.save_interval_seconds, 900);
  assert.match(view.html(), /servers.savePolicy.conflict/);
  await assert.rejects(state.coordinator.flush("server-a"), /draft is invalid/);
  view.reload.props.onClick();
  await state.flush();
  await state.coordinator.flush("server-a");
  assert.equal(state.render().native.props.settings.save_interval_seconds, 600);
  assert.equal(state.render().state, "saved");
  assert.equal(state.calls.length, 0);
  state.unmount();
});

test("malformed current JSON never gets replaced by a save-policy draft", async () => {
  const state = harness();
  state.props.details = { ...state.props.details, settings_json: "{broken" };
  state.change("8");
  await state.flush();
  assert.equal(state.calls.length, 0);
  state.unmount();
});

test("a confirmed native save remains visible and becomes the next CAS baseline if panel refresh fails", async () => {
  const state = harness();
  selectNativePolicy(state, "valheim", { save_interval_seconds: 1800 });
  editNative(state, "save_interval_seconds", "900");
  await state.flush();
  assert.equal(state.render().native.props.settings.save_interval_seconds, 900);
  assert.match(state.render().html(), /Policy saved/);
  editNative(state, "save_interval_seconds", "600");
  await state.flush();
  assert.equal(state.calls.length, 2);
  assert.equal(state.calls[1].options.expectedSettingsJson, state.calls[0].input.settings_json);
  assert.equal(state.render().native.props.settings.save_interval_seconds, 600);
  state.unmount();
});

test("refreshing untouched native values during a save does not create a false conflict on the next edit", async () => {
  const gate = deferred();
  const state = harness((input) => gate.promise.then(() => ({ ...state.props.details, settings_json: input.settings_json })));
  selectNativePolicy(state, "valheim", { save_interval_seconds: 1800, backup_count: 4 });
  editNative(state, "save_interval_seconds", "900");
  await state.flush();
  state.props.details = { ...state.props.details, settings_json: '{"save_interval_seconds":900,"backup_count":8}' };
  state.render();
  gate.resolve();
  await state.flush();
  assert.equal(state.render().native.props.settings.backup_count, 8);
  editNative(state, "backup_count", "9");
  await state.flush();
  assert.equal(state.calls.length, 2);
  assert.equal(JSON.parse(state.calls[1].input.settings_json).backup_count, 9);
  assert.equal(JSON.parse(state.calls[1].input.settings_json).save_interval_seconds, 900);
  state.unmount();
});

test("native module cross-field rules still block an invalid backup interval pair", async () => {
  const state = harness();
  selectNativePolicy(state);
  editNative(state, "backup_long_seconds", "60");
  const view = await state.flush();
  assert.ok(view.native.props.issues.some((issue) => issue.fieldKey === "backup_long_seconds"));
  assert.equal(view.state, "blocked");
  assert.equal(state.calls.length, 0);
  state.unmount();
});

test("a write by another maintenance editor is adopted instead of triggering a policy write or conflict", async () => {
  const state = harness();
  state.change("8");
  await state.flush();
  persist(state, state.calls[0]);
  await state.flush();
  state.props.details = { ...state.props.details, settings_json: '{"server_name":"Preserve this configuration","runtime_restart":{"enabled":true}}' };
  await state.flush();
  assert.equal(state.calls.length, 1, "another editor's write is not echoed back");
  assert.equal(state.render().state, "saved");
  state.toggle(true);
  await state.flush();
  assert.equal(state.calls.length, 2);
  assert.equal(state.calls[1].options.expectedSettingsJson, state.props.details.settings_json);
  assert.equal(state.calls[1].input.settings_json, state.props.details.settings_json);
  assert.equal(state.calls[1].input.backup_retention_count, 8);
  state.unmount();
});

test("invalid and out-of-range retention never reaches storage or silently truncates", async () => {
  const state = harness();
  for (const value of ["", "0", "-1", "1.5", "2e3", "NaN", "Infinity", "4294967296"]) {
    state.change(value);
    const view = await state.flush();
    assert.equal(view.retention.props["aria-invalid"], true, value);
    assert.equal(view.state, "blocked", value);
    assert.match(view.html(), /role="alert"/, value);
  }
  assert.deepEqual(state.calls, []);
  state.change("0003");
  const view = await state.flush();
  assert.equal(view.retention.props["aria-invalid"], false);
  assert.equal(view.state, "saved", "an equivalent integer is not a new policy");
  assert.deepEqual(state.calls, []);
  state.change("4294967295");
  await state.flush();
  assert.equal(state.calls.length, 1);
  assert.equal(state.calls[0].input.backup_retention_count, 4294967295);
  state.unmount();
});

test("edits during an in-flight save stay editable and the latest value wins", async () => {
  const gates = [deferred(), deferred()];
  let index = 0;
  const state = harness((input) => gates[index++].promise.then(() => ({ ...state.props.details, settings_json: input.settings_json })));
  state.change("6");
  await state.flush();
  assert.equal(state.calls.length, 1);
  assert.equal(state.render().state, "saving");
  assert.match(state.render().html(), /Saving policy/);
  assert.equal(state.render().retention.props.disabled, false);
  assert.equal(state.render().toggle.props.disabled, false);
  state.change("9");
  state.toggle(true);
  await state.flush();
  assert.equal(state.calls.length, 1, "a newer draft waits for the running write instead of racing it");
  gates[0].resolve();
  await state.flush();
  assert.equal(state.calls.length, 2);
  assert.equal(state.calls[1].input.backup_retention_count, 9);
  assert.equal(state.calls[1].input.auto_backup_on_stop, true);
  assert.equal(state.render().state, "saving");
  gates[1].resolve();
  await state.flush();
  assert.equal(state.render().state, "saved");
  assert.equal(state.render().retention.props.value, "9");
  state.unmount();
});

test("a superseded draft is skipped so only the latest pending value is written", async () => {
  const gate = deferred();
  let first = true;
  const state = harness((input) => {
    const result = { ...state.props.details, settings_json: input.settings_json };
    if (!first) return Promise.resolve(result);
    first = false;
    return gate.promise.then(() => result);
  });
  state.change("5");
  await state.flush();
  for (const value of ["6", "7", "8"]) {
    state.change(value);
    await state.flush();
  }
  gate.resolve();
  await state.flush();
  assert.deepEqual(state.calls.map((call) => call.input.backup_retention_count), [5, 8]);
  state.unmount();
});

test("failed saves retain the draft and expose the cause with a working retry", async () => {
  let failing = true;
  const state = harness(async (input) => {
    if (failing) throw new Error("settings changed while this edit was pending");
    return { ...state.props.details, settings_json: input.settings_json };
  });
  state.change("7");
  let view = await state.flush();
  assert.equal(view.retention.props.value, "7");
  assert.ok(["failed", "conflict"].includes(view.state), view.state);
  assert.match(view.html(), /Unable to save policy: settings changed while this edit was pending/);
  assert.doesNotMatch(view.html(), /Policy saved/);
  assert.ok(view.retry, "a failed save offers retry");
  failing = false;
  view.retry.props.onClick();
  view = await state.flush();
  assert.equal(state.calls.length, 2);
  assert.match(view.html(), /Policy saved/);
  assert.equal(view.retry, undefined);
  state.unmount();
});

for (const outcome of ["resolve", "reject"]) {
  test(`a ${outcome} from the previous instance cannot replace the newly selected draft`, async () => {
    const gate = deferred();
    const state = harness(() => gate.promise.then(() => state.props.details));
    state.change("9");
    await state.flush();
    state.props.details = details("server-b", { auto_backup_on_stop: true, backup_retention_count: 5 });
    assert.equal(state.render().retention.props.value, "5");
    assert.equal(state.render().toggle.props.checked, true);
    assert.equal(state.render().retention.props.disabled, false);
    state.change("12");
    gate[outcome](new Error("old instance failure"));
    await settle();
    const view = state.render();
    assert.equal(view.retention.props.value, "12");
    assert.doesNotMatch(view.html(), /Policy saved|old instance failure/);
    state.unmount();
  });
}

test("refreshes update clean policies, retain dirty drafts, and save against the latest configuration baseline", async () => {
  const state = harness();
  state.render();
  state.props.details = details("server-a", { backup_retention_count: 5 });
  assert.equal(state.render().retention.props.value, "5");
  state.change("8");
  state.props.details = details("server-a", { backup_retention_count: 6, settings_json: '{"server_name":"Updated elsewhere"}' });
  assert.equal(state.render().retention.props.value, "8");
  await state.flush();
  assert.equal(state.calls.length, 1);
  assert.equal(state.calls[0].input.backup_retention_count, 8);
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
  await state.flush();
  assert.equal(state.calls.length, 1);
  assert.equal(state.calls[0].input.backup_retention_count, 3);
  assert.equal(state.calls[0].input.auto_backup_on_stop, true);
  assert.equal(state.calls[0].input.settings_json, state.props.details.settings_json);
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
    await state.flush();
    assert.equal(state.calls.length, 1);
    assert.equal(state.calls[0].input.backup_retention_count, 3);
    assert.equal(state.calls[0].input.auto_backup_on_stop, true);
    assert.match(state.render().html(), /Policy saved/);
    state.unmount();
  });
}

test("an event retained from the previous instance does not replace the current instance draft", () => {
  const state = harness();
  const previousView = state.render();
  state.props.details = details("server-b", { backup_retention_count: 5 });
  state.change("9");
  previousView.toggle.props.onChange({ target: { checked: true } });
  assert.equal(state.render().retention.props.value, "9");
  assert.equal(state.render().toggle.props.checked, false);
  state.unmount();
});

test("a save completing after unmount does not update component state", async () => {
  const gate = deferred();
  const state = harness(() => gate.promise.then(() => state.props.details));
  state.change("8");
  await state.flush();
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
  assert.equal(view.footer, undefined, "archives show no save status");
  state.change("12");
  state.toggle(true);
  editNative(state, "save_interval_seconds", "2400");
  await state.flush();
  assert.deepEqual(state.calls, []);
  assert.deepEqual(state.props.details, before);
  assert.equal(state.render().native.props.settings.save_interval_seconds, 37);
  state.unmount();
});

test("leaving the tab inside the typing pause writes the pending edit and start waits for it", async () => {
  const gate = deferred();
  const state = harness((input) => gate.promise.then(() => ({ ...state.props.details, settings_json: input.settings_json })));
  selectNativePolicy(state);
  editNative(state, "save_interval_seconds", "900");
  state.render();
  assert.equal(state.calls.length, 0);
  state.unmount();
  await settle();
  assert.equal(state.calls.length, 1, "unmounting flushes the debounced draft instead of dropping it");
  assert.equal(JSON.parse(state.calls[0].input.settings_json).save_interval_seconds, 900);
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
  const state = harness(async (input) => {
    if (fail) throw new Error("policy write failed");
    return { ...state.props.details, settings_json: input.settings_json };
  });
  state.change("9");
  await state.flush();
  await assert.rejects(state.coordinator.flush("server-a"), /policy write failed/);
  fail = false;
  state.render().retry.props.onClick();
  await state.flush();
  await state.coordinator.flush("server-a");
  state.unmount();
});

test("discard acknowledges only the settled policy failure and leaves other pending writes tracked", async () => {
  const state = harness(async () => { throw new Error("discarded policy failure"); });
  state.change("9");
  await state.flush();
  await assert.rejects(state.coordinator.flush("server-a"), /discarded policy failure/);
  const gate = deferred();
  const other = state.coordinator.runOperation("server-a", () => gate.promise, "other-settings-editor");
  state.render().reload.props.onClick();
  await state.flush();
  let ready = false;
  const start = state.coordinator.flush("server-a").then(() => { ready = true; });
  await settle();
  assert.equal(ready, false);
  gate.resolve();
  await Promise.all([other, start]);
  assert.equal(state.render().retention.props.value, "3");
  assert.equal(state.calls.length, 1);
  state.unmount();
});

test("the native scope edits only game autosave fields and passes backup policy through", async () => {
  const state = harness();
  selectNativePolicy(state);
  state.props.scope = "native";
  const view = state.render();
  assert.ok(view.native, "native fields stay in the saves entry");
  assert.equal(view.toggle, undefined, "stop backup belongs to the backups entry");
  assert.equal(view.retention, undefined, "retention belongs to the backups entry");
  editNative(state, "save_interval_seconds", "1200");
  await state.flush();
  assert.equal(state.calls.length, 1);
  assert.equal(JSON.parse(state.calls[0].input.settings_json).save_interval_seconds, 1200);
  assert.equal(state.calls[0].input.auto_backup_on_stop, false);
  assert.equal(state.calls[0].input.backup_retention_count, 3);
  state.unmount();
});

test("the backup scope saves retention without native fields or their validation", async () => {
  const state = harness();
  selectNativePolicy(state, "valheim", { save_interval_seconds: -1 });
  state.props.scope = "backup";
  const view = state.render();
  assert.equal(view.native, undefined, "native autosave fields belong to the saves entry");
  assert.ok(view.toggle && view.retention);
  state.change("9");
  await state.flush();
  assert.equal(state.calls.length, 1, "an invalid native value edited elsewhere must not block backup retention");
  assert.equal(state.calls[0].input.backup_retention_count, 9);
  assert.equal(state.calls[0].input.settings_json, state.props.details.settings_json, "native settings keep their exact text");
  state.unmount();
});
