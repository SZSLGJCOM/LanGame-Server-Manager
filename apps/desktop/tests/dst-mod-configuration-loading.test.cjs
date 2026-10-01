const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((onResolve, onReject) => { resolve = onResolve; reject = onReject; });
  return { promise, resolve, reject };
}

function loadSource(name, dependencies) {
  const filename = path.join(__dirname, "../src/views/servers", name);
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports,
    Error,
    require(id) {
      assert.ok(Object.hasOwn(dependencies, id), `unexpected dependency ${id}`);
      return dependencies[id];
    }
  }, { filename });
  return exports;
}

function createHarness() {
  const states = [];
  const effects = [];
  const calls = [];
  let stateIndex = 0;
  let effectIndex = 0;
  let locale = "en-US";
  const { useDstModConfigurationSpec } = loadSource("useDstModConfigurationSpec.ts", {
    react: {
      useState(initial) {
        const index = stateIndex++;
        if (!(index in states)) states[index] = initial;
        return [states[index], (next) => {
          states[index] = typeof next === "function" ? next(states[index]) : next;
        }];
      },
      useEffect(callback, dependencies) {
        const index = effectIndex++;
        const previous = effects[index];
        if (!previous || dependencies.some((value, position) => !Object.is(value, previous.dependencies[position]))) {
          effects[index] = { callback, dependencies, cleanup: previous?.cleanup, pending: true };
        }
      }
    },
    "../../i18n": { useI18n: () => ({ locale, t: (_key, _params, fallback) => fallback }) },
    "../../api": {
      readDontStarveModConfigurationSpecs(instanceId, ids, requestLocale) {
        const response = deferred();
        calls.push({ instanceId, ids: Array.from(ids), locale: requestLocale, ...response });
        return response.promise;
      }
    }
  });
  return {
    calls,
    setLocale(nextLocale) { locale = nextLocale; },
    render(instanceId, modId, scanNonce = 0, enabled = true) {
      stateIndex = 0;
      effectIndex = 0;
      return useDstModConfigurationSpec(instanceId, modId, scanNonce, enabled);
    },
    commitEffects() {
      for (const effect of effects) {
        if (!effect.pending) continue;
        effect.pending = false;
        effect.cleanup?.();
        effect.cleanup = effect.callback();
      }
    },
    unmount() { effects.forEach((effect) => effect.cleanup?.()); }
  };
}

const flush = async () => { await Promise.resolve(); await Promise.resolve(); };
const spec = (modId) => ({ mod_id: modId, status: "loaded", options: [{ name: "enabled" }] });

test("archive mode disables native Mod definition reads and hides prior live results immediately", async () => {
  const harness = createHarness();
  let state = harness.render("instance-a", "123456", 0, false);
  harness.commitEffects();
  assert.equal(harness.calls.length, 0);
  assert.equal(state.loading, false);
  state.retry();
  harness.render("instance-a", "123456", 1, false);
  harness.commitEffects();
  assert.equal(harness.calls.length, 0, "retry and rescan cannot escape archive scope");

  harness.render("instance-a", "123456", 1, true);
  harness.commitEffects();
  assert.equal(harness.calls.length, 1);
  state = harness.render("instance-a", "123456", 1, false);
  assert.equal(state.specs.length, 0);
  harness.commitEffects();
  harness.calls[0].resolve([spec("123456")]);
  await flush();
  state = harness.render("instance-a", "123456", 1, false);
  assert.equal(state.specs.length, 0);
  assert.equal(state.loading, false);
  harness.unmount();
});

test("configuration reads only the selected Mod and hides previous fields before effects commit", async () => {
  const harness = createHarness();
  let state = harness.render("instance-a", "123456");
  assert.equal(state.loading, true);
  assert.equal(state.specs.length, 0);
  harness.commitEffects();
  assert.deepEqual(harness.calls[0].ids, ["123456"]);
  harness.calls[0].resolve([spec("123456")]);
  await flush();
  assert.equal(harness.render("instance-a", "123456").specs[0].mod_id, "123456");

  state = harness.render("instance-a", "234567");
  assert.equal(state.loading, true);
  assert.equal(state.specs.length, 0, "old Mod fields cannot be edited after selecting another Mod");
  harness.commitEffects();
  assert.deepEqual(harness.calls[1].ids, ["234567"]);
});

test("late success or failure from another selection cannot replace the current configuration", async () => {
  for (const rejectOld of [false, true]) {
    const harness = createHarness();
    harness.render("instance-a", "123456");
    harness.commitEffects();
    harness.render("instance-b", "234567");
    harness.commitEffects();
    harness.calls[1].resolve([spec("234567")]);
    await flush();
    if (rejectOld) harness.calls[0].reject(new Error("old read failed"));
    else harness.calls[0].resolve([spec("123456")]);
    await flush();
    const state = harness.render("instance-b", "234567");
    assert.equal(state.specs[0].mod_id, "234567");
    assert.equal(state.error, null);
  }
});

test("read errors remain visible until retry starts and empty results are not treated as no options", async () => {
  const harness = createHarness();
  harness.render("instance-a", "123456");
  harness.commitEffects();
  harness.calls[0].reject(new Error("disk temporarily unavailable"));
  await flush();
  let state = harness.render("instance-a", "123456");
  assert.equal(state.loading, false);
  assert.equal(state.error, "disk temporarily unavailable");
  state.retry();
  state = harness.render("instance-a", "123456");
  assert.equal(state.loading, true);
  assert.equal(state.error, null);
  harness.commitEffects();
  assert.equal(harness.calls.length, 2);
  harness.calls[1].resolve([]);
  await flush();
  state = harness.render("instance-a", "123456");
  assert.match(state.error, /did not return a result/);
  assert.equal(state.loading, false);
});

test("locale and installation rescans read fresh configuration and clearing selection issues no request", async () => {
  const harness = createHarness();
  harness.render("instance-a", "123456");
  harness.commitEffects();
  harness.calls[0].resolve([spec("123456")]);
  await flush();
  harness.setLocale("zh-CN");
  assert.equal(harness.render("instance-a", "123456").loading, true);
  harness.commitEffects();
  assert.equal(harness.calls[1].locale, "zh-CN");
  harness.render("instance-a", "123456", 1);
  harness.commitEffects();
  assert.equal(harness.calls.length, 3);
  const cleared = harness.render("instance-a", null, 1);
  harness.commitEffects();
  assert.equal(cleared.loading, false);
  assert.equal(cleared.specs.length, 0);
  assert.equal(harness.calls.length, 3);
  harness.calls[2].resolve([spec("123456")]);
  await flush();
  assert.equal(harness.render("instance-a", null, 1).specs.length, 0);
  harness.unmount();
});

function createConfigurationComponentHarness(readConfiguration) {
  let shard = "all";
  const Panel = () => {};
  const node = (type, props) => ({ type, props });
  const { DstWorkshopConfiguration } = loadSource("DstWorkshopConfiguration.tsx", {
    react: { useId: () => "shard-select", useState: () => [shard, (value) => { shard = value; }] },
    "../../i18n": { useI18n: () => ({ t: (_key, _params, fallback) => fallback }) },
    "react/jsx-runtime": { jsx: node, jsxs: node, Fragment: "fragment" },
    "./DstModConfigPanel": { DstModConfigPanel: Panel },
    "../settings/modules/dontstarve-shards": loadSource("../settings/modules/dontstarve-shards.ts", {}),
    "./mod-workbench-dst-policy": loadSource("mod-workbench-dst-policy.ts", {
      "../settings/modules/dontstarve-shards": loadSource("../settings/modules/dontstarve-shards.ts", {})
    }),
    "./useDstModConfigurationSpec": { useDstModConfigurationSpec: readConfiguration }
  });
  function nodes(tree) {
    if (!tree || typeof tree !== "object") return [];
    return [tree, ...[].concat(tree.props?.children ?? []).flatMap(nodes)];
  }
  return {
    render(props) {
      const rendered = nodes(DstWorkshopConfiguration(props));
      return {
        panel: rendered.find((item) => item.type === Panel),
        selector: rendered.find((item) => item.type === "select"),
        notices: rendered.filter((item) => item.props?.role === "status")
      };
    }
  };
}

test("Workshop configuration forwards read state and keeps installation and setting writes in the host", () => {
  const settings = { master_enabled_workshop_mod_ids: "123456" };
  const onSettingsChange = () => {};
  const onInstallMissingMod = () => {};
  const retry = () => {};
  const harness = createConfigurationComponentHarness((instanceId, modId, nonce) => {
      assert.equal(instanceId, "instance-a");
      assert.equal(modId, "123456");
      assert.equal(nonce, 4);
      return { specs: [], loading: false, error: "read failed", retry };
  });
  const { panel, selector } = harness.render({
    instanceId: "instance-a", selectedModId: "123456", scanNonce: 4, settings,
    disabled: true, canInstallMissingMod: false, installingMissingMod: true,
    onSettingsChange, onInstallMissingMod
  });
  assert.equal(selector.props.value, "all", "default behavior still synchronizes both shards");
  assert.equal(panel.props.settings, settings);
  assert.equal(panel.props.configurationError, "read failed");
  assert.equal(panel.props.onRetryConfiguration, retry);
  assert.equal(panel.props.onSettingsChange, onSettingsChange);
  assert.equal(panel.props.onInstallMissingMod, onInstallMissingMod);
  assert.equal(panel.props.disabled, true);
  assert.equal(panel.props.installingMissingMod, true);
});

test("the external Mod editor isolates Master and Caves patches while retaining all other settings", () => {
  const settings = {
    enable_caves: true,
    shared_workshop_mod_ids: "123456",
    master_enabled_workshop_mod_ids: "123456",
    caves_enabled_workshop_mod_ids: "123456\n234567",
    master_mod_configuration_options: { "123456": { enabled: true } },
    caves_mod_configuration_options: { "123456": { enabled: false } },
    master_modoverrides_lua: "return {}",
    caves_modoverrides_lua: "return make_caves_mods()",
    cluster_name: "Preserve this room"
  };
  const snapshot = structuredClone(settings);
  const harness = createConfigurationComponentHarness(() => ({ specs: [], loading: false, error: null, retry() {} }));
  let updated;
  const props = { instanceId: "instance-a", selectedModId: "123456", scanNonce: 0, settings,
    onSettingsChange(value) { updated = value; } };
  for (const shard of ["master", "caves"]) {
    harness.render(props).selector.props.onChange({ target: { value: shard } });
    const { panel } = harness.render(props);
    assert.equal(panel.props.disabled, false);
    assert.equal(panel.props.settings.master_mod_configuration_options, settings[`${shard}_mod_configuration_options`]);
    assert.equal(panel.props.settings.caves_mod_configuration_options, settings[`${shard}_mod_configuration_options`]);
    assert.equal(panel.props.settings.master_modoverrides_lua, settings[`${shard}_modoverrides_lua`]);
    const nextOptions = { "123456": { enabled: true, strength: 2 } };
    panel.props.onSettingsChange({ master_mod_configuration_options: nextOptions, caves_mod_configuration_options: nextOptions });
    assert.deepEqual({ ...updated }, { ...settings, [`${shard}_mod_configuration_options`]: nextOptions });
    panel.props.onSettingsChange({});
    const reset = { ...settings };
    delete reset[`${shard}_mod_configuration_options`];
    assert.deepEqual({ ...updated }, reset, "resetting one shard keeps the other shard and native scripts");
  }
  assert.deepEqual(settings, snapshot);
});

test("the external Caves Mod editor preserves inactive settings until Caves are enabled", () => {
  const settings = { enable_caves: false, caves_enabled_workshop_mod_ids: "123456",
    caves_mod_configuration_options: { "123456": { enabled: false } } };
  const snapshot = structuredClone(settings);
  const harness = createConfigurationComponentHarness(() => ({ specs: [], loading: false, error: null, retry() {} }));
  const props = { instanceId: "instance-a", selectedModId: "123456", scanNonce: 0, settings,
    onSettingsChange() { assert.fail("inactive Caves options must not be rewritten"); } };
  harness.render(props).selector.props.onChange({ target: { value: "caves" } });
  const inactive = harness.render(props);
  assert.equal(inactive.panel.props.disabled, true);
  assert.match(inactive.notices[0].props.children, /Caves are disabled/);
  inactive.panel.props.onSettingsChange({ master_mod_configuration_options: { "123456": { enabled: true } } });
  assert.deepEqual(settings, snapshot);
  const active = harness.render({ ...props, settings: { ...settings, enable_caves: true } });
  assert.equal(active.panel.props.disabled, false);
  assert.equal(active.notices.length, 0);
});

test("Island Adventures selects all four shards and isolates Islands and Volcano writes", () => {
  const settings = { shard_layout: "island_adventures", enable_caves: false, cluster_name: "Keep" };
  for (const [index, shard] of ["master", "caves", "islands", "volcano"].entries()) {
    settings[`${shard}_enabled_workshop_mod_ids`] = "1467214795";
    settings[`${shard}_mod_configuration_options`] = { "1467214795": { difficulty: index + 1 } };
  }
  const harness = createConfigurationComponentHarness(() => ({ specs: [], loading: false, error: null, retry() {} }));
  let updated;
  const props = { instanceId: "instance-ia", selectedModId: "1467214795", scanNonce: 0, settings,
    onSettingsChange(value) { updated = value; } };
  const options = harness.render(props).selector.props.children.flatMap((child) => Array.isArray(child) ? child : [child]);
  assert.deepEqual(Array.from(options, (option) => option.props.value), ["all", "master", "caves", "islands", "volcano"]);
  for (const shard of ["caves", "islands", "volcano"]) {
    harness.render(props).selector.props.onChange({ target: { value: shard } });
    const { panel } = harness.render(props);
    assert.equal(panel.props.disabled, false, "all IA shards stay active even if the optional standard Caves flag is false");
    assert.equal(panel.props.settings.master_mod_configuration_options, settings[`${shard}_mod_configuration_options`]);
    const nextOptions = { "1467214795": { difficulty: 7 } };
    panel.props.onSettingsChange({ master_mod_configuration_options: nextOptions });
    assert.deepEqual({ ...updated }, { ...settings, [`${shard}_mod_configuration_options`]: nextOptions });
  }
  const standard = harness.render({ ...props, settings: { ...settings, shard_layout: "standard" } });
  assert.equal(standard.selector.props.value, "all", "a selected extra shard cannot remain selected after a layout change");
});
