const ts = require("@typescript/typescript6");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { parseSource, sourceText, transpileTypeScript, visitSyntax } = require("../scripts/typescript_source_tools.cjs");

const filename = path.resolve(__dirname, "../src/hooks/useDesktopActions.ts");
const source = fs.readFileSync(filename, "utf8");
let declaration;
visitSyntax(parseSource(source, filename), (node) => {
  if (ts.isFunctionDeclaration(node) && node.name?.text === "handleSaveSettings") {
    declaration = node;
    return false;
  }
});
assert.ok(declaration, "settings persistence action exists");
const actionSource = transpileTypeScript(sourceText(source, declaration), filename) + "; handleSaveSettings";

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function details(id = "server-a", overrides = {}) {
  return {
    summary: { id, name: id, module_id: "valheim", status: "stopped", autostart: true, bind_ip: "127.0.0.1", port_count: 1 },
    ports: [{ name: "game", port: 2456, protocol: "udp" }],
    settings_json: '{"save_interval_seconds":1800,"mods":["retained"]}',
    config_file_path: `${id}/config/instance.json`,
    saves_path: `${id}/saves`,
    backup_uses_declared_saves_path: true,
    auto_backup_on_stop: false,
    backup_retention_count: 3,
    active_run: null,
    ...overrides
  };
}

function harness() {
  const original = details();
  const saved = details("server-a", {
    settings_json: '{\n  "save_interval_seconds": 900,\n  "mods": ["retained"]\n}',
    auto_backup_on_stop: true,
    backup_retention_count: 8,
    saves_path: "server-a/saves/resolved-world"
  });
  saved.summary = { ...saved.summary, bind_ip: "0.0.0.0", port_count: 2 };
  saved.ports = [...saved.ports, { name: "query", port: 2457, protocol: "udp" }];
  const input = {
    id: "server-a", bind_ip: saved.summary.bind_ip, ports: saved.ports,
    settings_json: '{"save_interval_seconds":900,"mods":["retained"]}',
    auto_backup_on_stop: true, backup_retention_count: 8
  };
  const mutation = deferred();
  const refresh = deferred();
  const refreshStarted = deferred();
  const model = {
    selectedId: "server-a", selected: original, cache: { "server-a": original },
    activity: null, mutations: [], refreshes: []
  };
  const options = {
    instanceDetailsById: model.cache,
    getCurrentInstanceId: () => model.selectedId,
    setActivity: (activity) => { model.activity = activity; },
    setSelectedInstanceDetails: (update) => { model.selected = update(model.selected); },
    setInstanceDetailsById: (update) => { model.cache = update(model.cache); }
  };
  const run = vm.runInNewContext(actionSource, {
    options,
    updateInstance: (nextInput, expectedSettingsJson) => {
      model.mutations.push({ input: nextInput, expectedSettingsJson });
      return mutation.promise;
    },
    refreshInstancePanelAfterMutation: (id) => {
      model.refreshes.push(id);
      refreshStarted.resolve();
      return refresh.promise;
    },
    message: (key, params) => ({ key, params }),
    describeError: (error) => error.message,
    t: (key) => key
  });
  return {
    model, original, saved, input, mutation, refresh, refreshStarted,
    save: (overrides = {}) => run(input, { expectedSettingsJson: original.settings_json, throwOnError: true, ...overrides })
  };
}

async function persistUntilRefresh(state) {
  const pending = state.save();
  state.mutation.resolve(state.saved);
  await state.refreshStarted.promise;
  assert.deepEqual(state.model.refreshes, ["server-a"]);
  return { pending };
}

test("failed refresh publishes acknowledged normalized settings to selection and cache", async () => {
  const state = harness();
  const { pending } = await persistUntilRefresh(state);
  state.refresh.reject(new Error("panel unavailable"));
  await pending;
  assert.equal(state.model.mutations[0].expectedSettingsJson, state.original.settings_json);
  for (const current of [state.model.selected, state.model.cache["server-a"]]) {
    assert.equal(current.settings_json, state.saved.settings_json);
    assert.deepEqual(JSON.parse(current.settings_json).mods, ["retained"]);
    assert.equal(current.auto_backup_on_stop, true);
    assert.equal(current.backup_retention_count, 8);
    assert.equal(current.saves_path, state.saved.saves_path);
    assert.equal(current.summary.bind_ip, "0.0.0.0");
    assert.equal(current.summary.port_count, 2);
    assert.equal(current.ports.length, 2);
  }
  assert.equal(state.model.activity.key, "activity.settingsRefreshFailed");
});

test("failed refresh preserves concurrent autostart and runtime changes", async () => {
  const state = harness();
  const { pending } = await persistUntilRefresh(state);
  const run = { run_id: 7, pid: 42 };
  const latest = {
    ...state.original,
    summary: { ...state.original.summary, autostart: false, status: "running" },
    active_run: run
  };
  state.model.selected = latest;
  state.model.cache = { "server-a": latest };
  state.refresh.reject(new Error("panel unavailable"));
  await pending;
  for (const current of [state.model.selected, state.model.cache["server-a"]]) {
    assert.equal(current.settings_json, state.saved.settings_json);
    assert.equal(current.summary.autostart, false);
    assert.equal(current.summary.status, "running");
    assert.ok(Object.is(current.active_run, run));
  }
});

for (const selectedStillOld of [false, true]) {
  test(`failed refresh does not replace a changed selection (old details retained: ${selectedStillOld})`, async () => {
    const state = harness();
    const { pending } = await persistUntilRefresh(state);
    state.model.selectedId = "server-b";
    state.model.selected = selectedStillOld ? state.original : details("server-b");
    const selected = state.model.selected;
    state.refresh.reject(new Error("panel unavailable"));
    await pending;
    assert.ok(Object.is(state.model.selected, selected));
    assert.equal(state.model.cache["server-a"].settings_json, state.saved.settings_json);
  });
}

for (const change of [
  { settings_json: '{"save_interval_seconds":600,"mods":["added later"]}' },
  { backup_retention_count: 12 },
  { auto_backup_on_stop: true },
  { ports: [{ name: "game", port: 2500, protocol: "udp" }] },
  { saves_path: "server-a/saves/newer-world" }
]) {
  test(`failed refresh preserves later ${Object.keys(change)[0]} updates`, async () => {
    const state = harness();
    const { pending } = await persistUntilRefresh(state);
    const latest = { ...state.original, ...change };
    state.model.selected = latest;
    state.model.cache = { "server-a": latest };
    state.refresh.reject(new Error("panel unavailable"));
    await pending;
    assert.ok(Object.is(state.model.selected, latest));
    assert.ok(Object.is(state.model.cache["server-a"], latest));
  });
}

test("bootstrap failure after panel caching does not roll back a newer cached configuration", async () => {
  const state = harness();
  const { pending } = await persistUntilRefresh(state);
  const newer = { ...state.saved, settings_json: '{"save_interval_seconds":600,"mods":["added later"]}' };
  state.model.cache = { "server-a": newer };
  state.refresh.reject(new Error("bootstrap unavailable"));
  await pending;
  assert.ok(Object.is(state.model.cache["server-a"], newer));
  assert.equal(state.model.selected.settings_json, state.saved.settings_json);
});

test("failed refresh does not resurrect a removed cached instance or empty selection", async () => {
  const state = harness();
  const { pending } = await persistUntilRefresh(state);
  state.model.cache = {};
  state.model.selected = null;
  state.model.selectedId = null;
  state.refresh.reject(new Error("instance removed"));
  await pending;
  assert.equal(Object.hasOwn(state.model.cache, "server-a"), false);
  assert.equal(state.model.selected, null);
});

test("failed mutation preserves local data and rejects when requested", async () => {
  const state = harness();
  const pending = state.save();
  const conflict = new Error("settings changed");
  state.mutation.reject(conflict);
  await assert.rejects(pending, (error) => Object.is(error, conflict));
  assert.ok(Object.is(state.model.selected, state.original));
  assert.ok(Object.is(state.model.cache["server-a"], state.original));
  assert.equal(state.model.refreshes.length, 0);
  assert.equal(state.model.activity.key, "activity.saveSettingsFailed");
});

test("successful refresh retains the refreshed panel without invoking the fallback", async () => {
  const state = harness();
  const { pending } = await persistUntilRefresh(state);
  const refreshed = { ...state.saved, backup_retention_count: 12 };
  state.model.cache = { "server-a": refreshed };
  state.model.selected = refreshed;
  state.refresh.resolve({ details: refreshed });
  await pending;
  assert.ok(Object.is(state.model.selected, refreshed));
  assert.ok(Object.is(state.model.cache["server-a"], refreshed));
  assert.equal(state.model.activity.key, "activity.settingsSaved");
});

for (const refreshFails of [false, true]) {
  test(`save returns its own persistence acknowledgement when refresh fails: ${refreshFails}`, async () => {
    const state = harness();
    const { pending } = await persistUntilRefresh(state);
    if (refreshFails) state.refresh.reject(new Error("panel unavailable"));
    else state.refresh.resolve({ details: { ...state.saved, settings_json: '{"later":"edit"}' } });
    assert.ok(Object.is(await pending, state.saved), "the caller needs the saved acknowledgement, not its submitted JSON or a later refresh");
  });
}
