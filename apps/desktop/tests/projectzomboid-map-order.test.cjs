const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".tsx"] = require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
require.extensions[".css"] = (module) => module._compile("", module.filename);
const { changedSettingKeys, mergeSettingPatch } = require("../src/views/servers/mod-settings-patch.ts");
const { buildModSettingsRemovePlan } = require("../src/views/servers/mod-workbench-plans.ts");
const { validateProjectZomboidMapOrder } = require("../src/views/servers/ProjectZomboidMapOrderEditor.tsx");
const { EN_US_PROJECT_ZOMBOID_MESSAGES: en } = require("../src/i18n/games/projectzomboid.en.ts");
const { ZH_CN_PROJECT_ZOMBOID_MESSAGES: zh } = require("../src/i18n/games/projectzomboid.zh-cn.ts");
const settle = () => new Promise((resolve) => setImmediate(resolve));
const translate = (catalog) => (key, params, fallback) => Object.entries(params ?? {}).reduce(
  (text, [name, value]) => text.replaceAll(`{${name}}`, String(value)), catalog[key] ?? fallback ?? key);
const t = translate(en);
function descendants(node) {
  return React.isValidElement(node) ? [node, ...React.Children.toArray(node.props.children).flatMap(descendants)] : [];
}
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function harness(value = "CustomMap", save = async () => {}, catalog = en) {
  const hooks = [], effects = [], calls = [];
  let cursor = 0, writes = 0;
  const equal = (a, b) => a?.length === b?.length && a.every((value, i) => Object.is(value, b[i]));
  const react = { ...React,
    useId: () => "pz-map-test",
    useState(initial) {
      const slot = cursor++;
      if (!(slot in hooks)) hooks[slot] = { value: typeof initial === "function" ? initial() : initial };
      return [hooks[slot].value, (next) => {
        const value = typeof next === "function" ? next(hooks[slot].value) : next;
        if (!Object.is(value, hooks[slot].value)) writes++;
        hooks[slot].value = value;
      }];
    },
    useRef(initial) { const slot = cursor++; return hooks[slot] ?? (hooks[slot] = { current: initial }); },
    useEffect(create, deps) {
      const slot = cursor++;
      if (equal(hooks[slot]?.deps, deps)) return;
      const previous = hooks[slot]; hooks[slot] = { deps };
      effects.push(() => { previous?.cleanup?.(); hooks[slot].cleanup = create(); });
    }
  };
  const filename = path.resolve(__dirname, "../src/views/servers/ProjectZomboidMapOrderEditor.tsx");
  const loaded = new Module(filename, module); loaded.filename = filename;
  const requireFromFile = Module.createRequire(filename);
  loaded.require = (id) => id === "react" ? react : id === "../../i18n"
    ? { useI18n: () => ({ locale: catalog === zh ? "zh-CN" : "en-US", t: translate(catalog) }) } : requireFromFile(id);
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  const props = { value, disabled: false, async onReload() { return props.value; },
    async onSave(value, expected) { calls.push({ value, expected }); await save(value, expected); } };
  function render() {
    let root;
    for (let i = 0; i < 10; i++) {
      cursor = 0; const before = writes;
      root = loaded.exports.ProjectZomboidMapOrderEditor(props);
      while (effects.length) effects.shift()();
      if (before === writes) break;
      assert.ok(i < 9, "effects settle");
    }
    const nodes = descendants(root);
    return { root, nodes, html: () => renderToStaticMarkup(root), input: nodes.find((n) => n.type === "textarea"),
      button: (name) => nodes.find((n) => n.type === "button" && (n.props["aria-label"] === name || n.props.children === name)),
      maps: nodes.filter((n) => n.type === "li").map((n) => n.props.children.props.children[0].props.children) };
  }
  return { props, calls, render, get writes() { return writes; },
    click(name) { const button = render().button(name); assert.ok(button, name); button.props.onClick(); },
    type(value) { render().input.props.onChange({ target: { value } }); },
    unmount() { for (const hook of hooks) hook?.cleanup?.(); } };
}

test("last map removal is invalid until a vanilla or custom map is added", async () => {
  const view = harness();
  view.click("Remove CustomMap");
  assert.deepEqual(view.render().maps, []);
  assert.equal(view.render().button("Save map order").props.disabled, true);
  assert.match(view.render().html(), /Keep at least one map/);
  view.click("Save map order"); assert.equal(view.calls.length, 0);
  view.click("Add vanilla map");
  assert.deepEqual(view.render().maps, ["Muldraugh, KY"]);
  view.click("Save map order"); await settle();
  assert.deepEqual(view.calls, [{ value: "Muldraugh, KY", expected: "CustomMap" }]);
  assert.deepEqual(view.render().maps, ["Muldraugh, KY"], "stale parent props cannot undo acknowledged save");
  view.props.value = "Muldraugh, KY";
  assert.match(view.render().html(), /Map order saved/);
});

test("names retain commas, support ordering and deduplicate without a second map list", async () => {
  const view = harness("Muldraugh, KY");
  view.type("Custom A; Custom B\ncustom a"); view.click("Add maps");
  assert.deepEqual(view.render().maps, ["Muldraugh, KY", "Custom A", "Custom B"]);
  view.click("Move Custom B up"); view.click("Move Custom B up");
  view.click("Remove Custom A");
  view.click("Save map order"); await settle();
  assert.deepEqual(view.calls[0], { value: "Custom B\nMuldraugh, KY", expected: "Muldraugh, KY" });
  assert.equal((view.render().html().match(/data-field-key="map_name"/g) ?? []).length, 1);
});

test("background refresh and hidden Store view preserve typed drafts; failed saves remain recoverable", async () => {
  const view = harness("Muldraugh, KY", async () => { throw new Error("Map order changed elsewhere"); });
  view.type("MyMap");
  view.props.value = "RemoteMap"; view.props.hidden = true;
  assert.equal(view.render().input.props.value, "MyMap");
  assert.deepEqual(view.render().maps, ["Muldraugh, KY"]);
  view.props.hidden = false; view.click("Save map order"); await settle();
  assert.deepEqual(view.calls[0], { value: "Muldraugh, KY\nMyMap", expected: "Muldraugh, KY" });
  assert.equal(view.render().input.props.value, "MyMap");
  assert.match(view.render().html(), /Map order changed elsewhere/);
  view.props.value = "Old stale props";
  view.props.onReload = async () => "RemoteMap";
  view.click("Reload saved order"); await settle();
  assert.deepEqual(view.render().maps, ["RemoteMap"]);
  assert.equal(view.render().input.props.value, "");
  view.type("RetryMap"); view.click("Save map order"); await settle();
  assert.deepEqual(view.calls[1], { value: "RemoteMap\nRetryMap", expected: "RemoteMap" });
});

test("busy, duplicate submissions and unmounted completion cannot mutate the draft", async () => {
  const gate = deferred(); const view = harness("CustomMap", () => gate.promise);
  view.type("AnotherMap");
  const save = view.render().button("Save map order"); save.props.onClick(); save.props.onClick();
  assert.equal(view.calls.length, 1);
  assert.equal(view.render().input.props.disabled, true);
  view.unmount(); const writes = view.writes; gate.resolve(); await settle();
  assert.equal(view.writes, writes);
  const blocked = harness(); blocked.props.disabled = true; blocked.type("Ignored");
  assert.equal(blocked.render().input.props.value, "");
  assert.equal(blocked.render().button("Add vanilla map").props.disabled, true);
});

test("Chinese controls use the same nonempty map validator and complete localized actions", () => {
  const view = harness("CustomMap", async () => {}, zh);
  for (const name of ["上移 CustomMap", "下移 CustomMap", "移除 CustomMap", "添加地图", "添加原版地图", "保存地图顺序", "重新加载已保存顺序"]) assert.ok(view.render().button(name), name);
  view.click("移除 CustomMap");
  assert.equal(view.render().button("保存地图顺序").props.disabled, true);
  assert.ok(validateProjectZomboidMapOrder(" ; \n", "zh-CN", translate(zh)));
});

function persistence(base, fresh = base, readOnly = false) {
  const filename = path.resolve(__dirname, "../src/views/servers/ModWorkbench.tsx");
  const source = fs.readFileSync(filename, "utf8");
  const fn = source.match(/  async function persistSettings\([\s\S]*?\r?\n  }/)[0];
  const exports = {}, writes = [], latestSettingsRef = { current: base };
  let reads = 0;
  const currentDetails = { summary: { id: "pz", bind_ip: "127.0.0.1" }, settings_json: JSON.stringify(fresh),
    ports: [{ name: "game", port: 16261 }], auto_backup_on_stop: true, backup_retention_count: 5 };
  vm.runInNewContext(transpileTypeScript(`export ${fn.trim()}`, filename), {
    exports, moduleId: "projectzomboid", locale: "en-US", t, latestSettingsRef, readOnly,
    changedSettingKeys, mergeSettingPatch, validateProjectZomboidMapOrder,
    readWritableInstanceState: async () => { reads++; return { details: currentDetails, settings: fresh }; },
    props: { onSaveSettings: async (input, options) => writes.push({ input, options }) }
  });
  return { persist: exports.persistSettings, writes, currentDetails, get reads() { return reads; } };
}

test("archived ModWorkbench persistence rejects before reading or saving live instance state", async () => {
  const base = { map_name: "CustomMap", mods: "ExistingMod" };
  const state = persistence(base, base, true);
  await assert.rejects(state.persist({ ...base, map_name: "Muldraugh, KY" }, base),
    /^Error: Restore this instance to manage Mods\.$/);
  assert.equal(state.reads, 0);
  assert.equal(state.writes.length, 0);
  assert.deepEqual(base, { map_name: "CustomMap", mods: "ExistingMod" });
});

test("actual ModWorkbench persistence saves map order against fresh metadata without losing other settings", async () => {
  const base = { map_name: "CustomMap", server_name: "Old", mods: "Example" };
  const fresh = { ...base, server_name: "Changed elsewhere", max_players: 12 };
  const state = persistence(base, fresh);
  await state.persist({ ...base, map_name: "Muldraugh, KY" }, base);
  assert.deepEqual(JSON.parse(state.writes[0].input.settings_json), { ...fresh, map_name: "Muldraugh, KY" });
  assert.equal(state.writes[0].options.expectedSettingsJson, JSON.stringify(fresh));
  assert.equal(state.writes[0].input.backup_retention_count, 5);
});

test("actual persistence rejects stale map drafts but permits an already-applied value", async () => {
  const base = { map_name: "CustomMap" }, fresh = { map_name: "ExternalMap" };
  const state = persistence(base, fresh);
  await assert.rejects(state.persist({ map_name: "MyMap" }, base), /changed elsewhere/);
  assert.equal(state.writes.length, 0);
  await state.persist(fresh, base);
  assert.equal(state.writes.length, 1);
});

test("removing the last map through a Workshop uninstall plan is blocked at the common save boundary", async () => {
  const id = "100010";
  const base = { map_name: "CustomMap", workshop_items: id, mods: "CustomMod" };
  const lookup = { [id]: { id, status: "resolved", item_kind: "item", consumer_app_id: 108600, children: [] } };
  const snapshot = { workshop_root_exists: true, items: [{ workshop_item_id: id, status: "installed",
    mods: [{ mod_id: "CustomMod", status: "loaded", map_ids: ["CustomMap"] }] }] };
  const plan = buildModSettingsRemovePlan("projectzomboid", base, [id], lookup, 108600, snapshot);
  assert.equal(plan.canRemove, true); assert.equal(plan.nextSettings.map_name, "");
  const state = persistence(base);
  await assert.rejects(state.persist(plan.nextSettings), /Keep at least one map/);
  assert.equal(state.writes.length, 0);
  const unrelated = persistence({ map_name: "", mods: "Old" });
  await unrelated.persist({ map_name: "", mods: "New" });
  assert.equal(unrelated.writes.length, 1, "other Mod fields remain writable");
});
