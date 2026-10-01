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
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function details(id = "server-a", overrides = {}) {
  return {
    summary: { id, bind_ip: "127.0.0.1", autostart: true },
    auto_backup_on_stop: false, backup_retention_count: 3,
    settings_json: '{"server_name":"Preserve this configuration"}',
    ports: [{ name: "game", port: 27015, protocol: "udp" }],
    ...overrides
  };
}
function descendants(node) {
  if (!React.isValidElement(node)) return [];
  return [node, ...React.Children.toArray([node.props.children, node.props.action]).flatMap(descendants)];
}

function harness(save = async () => {}) {
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
  const filename = path.resolve(__dirname, "../src/views/servers/InstanceAutostartEditor.tsx");
  const loaded = new Module(filename, module);
  loaded.filename = filename;
  const requireFromFile = Module.createRequire(filename);
  loaded.require = (id) => id === "react" ? react : requireFromFile(id);
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  const props = {
    details: details(), t: (key, _params, fallback) => fallback ?? key,
    async onSaveAutostart(instanceId, autostart) {
      calls.push({ instanceId, autostart });
      await save(instanceId, autostart);
      if (props.details.summary.id === instanceId) props.details = { ...props.details, summary: { ...props.details.summary, autostart } };
    }
  };
  function render() {
    let root;
    for (let attempt = 0; attempt < 10; attempt++) {
      cursor = 0;
      const before = writes;
      root = loaded.exports.InstanceAutostartEditor(props);
      while (effects.length) effects.shift()();
      if (before === writes) break;
      assert.ok(attempt < 9, "component effects must settle");
    }
    const nodes = descendants(root);
    return {
      root, html: () => renderToStaticMarkup(root),
      toggle: nodes.find((node) => node.type === "input" && node.props.type === "checkbox"),
      retry: nodes.find((node) => node.type === "button")
    };
  }
  return {
    props, calls, render, get writes() { return writes; },
    toggle(checked) { render().toggle.props.onChange({ target: { checked } }); },
    retry() { render().retry.props.onClick(); },
    unmount() { for (const hook of hooks) hook?.cleanup?.(); }
  };
}

test("autostart saves only the selected instance flag and preserves all configuration and policy fields", async () => {
  const state = harness();
  const before = structuredClone(state.props.details);
  state.toggle(false);
  await settle();
  assert.deepEqual(state.calls, [{ instanceId: "server-a", autostart: false }]);
  assert.deepEqual(state.props.details, { ...before, summary: { ...before.summary, autostart: false } });
  assert.equal(state.render().toggle.props.checked, false);
  assert.match(state.render().html(), /Policy saved/);
  state.unmount();
});

test("saving disables toggling and suppresses duplicate requests before the next render", async () => {
  const gate = deferred();
  const state = harness(() => gate.promise);
  const toggle = state.render().toggle;
  toggle.props.onChange({ target: { checked: false } });
  toggle.props.onChange({ target: { checked: true } });
  assert.equal(state.calls.length, 1);
  assert.equal(state.render().toggle.props.disabled, true);
  assert.equal(state.render().toggle.props.checked, false);
  assert.match(state.render().html(), /Saving policy/);
  gate.resolve();
  await settle();
  assert.equal(state.render().toggle.props.disabled, false);
  state.unmount();
});

test("failed updates expose the cause and retry the intended value without changing unrelated fields", async () => {
  let failing = true;
  const state = harness(async () => { if (failing) throw new Error("Database is temporarily unavailable"); });
  state.toggle(false);
  await settle();
  assert.equal(state.props.details.summary.autostart, true);
  assert.equal(state.render().toggle.props.checked, false);
  assert.match(state.render().html(), /role="alert"/);
  assert.match(state.render().html(), /Database is temporarily unavailable/);
  assert.doesNotMatch(state.render().html(), /Policy saved/);
  failing = false;
  state.retry();
  await settle();
  assert.equal(state.calls.length, 2);
  assert.equal(state.props.details.summary.autostart, false);
  assert.equal(state.render().retry, undefined);
  state.unmount();
});

for (const outcome of ["resolve", "reject"]) {
  test(`previous-instance ${outcome} does not affect the newly selected instance`, async () => {
    const gate = deferred();
    const state = harness(() => gate.promise);
    const staleToggle = state.render().toggle;
    state.toggle(false);
    state.props.details = details("server-b");
    assert.equal(state.render().toggle.props.checked, true);
    gate[outcome](new Error("Old save failed"));
    await settle();
    staleToggle.props.onChange({ target: { checked: false } });
    assert.equal(state.calls.length, 1);
    assert.equal(state.render().toggle.props.checked, true);
    assert.doesNotMatch(state.render().html(), /Old save failed|Policy saved/);
    state.unmount();
  });
}

test("an unmounted editor ignores save completion and stale events", async () => {
  const gate = deferred();
  const state = harness(() => gate.promise);
  const toggle = state.render().toggle;
  state.toggle(false);
  state.unmount();
  const writes = state.writes;
  gate.resolve();
  await settle();
  assert.equal(state.writes, writes);
  toggle.props.onChange({ target: { checked: true } });
  assert.equal(state.calls.length, 1);
});

test("clean flags follow refreshes and running instances remain editable for next app launch", () => {
  const state = harness();
  state.render();
  state.props.details = { ...state.props.details, summary: { ...state.props.details.summary, status: "Running", autostart: false } };
  assert.equal(state.render().toggle.props.checked, false);
  assert.equal(state.render().toggle.props.disabled, false);
  state.unmount();
});

test("the selected instance autostart failure is reported as activity feedback", () => {
  const state = harness();
  state.props.jobs = [
    { id: "other", target_id: "server-b", kind: "StartInstance", label: "Autostart Other", status: "Failed", detail: "Other instance" },
    { id: "this", target_id: "server-a", kind: "StartInstance", label: "Autostart Server", status: "Failed", detail: "Executable unavailable" }
  ];
  assert.match(state.render().html(), /Executable unavailable/);
  assert.doesNotMatch(state.render().html(), /Other instance/);
  state.unmount();
});

test("autosave payload and signatures never include a stale autostart flag", () => {
  const { buildAutoSaveInstanceSettingsInput, instanceSettingsInputSignature } = require("../src/views/settings/useAutoSaveInstanceSettings.ts");
  const input = buildAutoSaveInstanceSettingsInput({ details: details(), bindIp: "127.0.0.1", autostart: false,
    autoBackupOnStop: false, backupRetentionCount: "3", settingsJson: "{}" });
  assert.equal(Object.hasOwn(input, "autostart"), false);
  assert.equal(Object.hasOwn(JSON.parse(instanceSettingsInputSignature(input)), "autostart"), false);
});
const vm = require('node:vm');
const { parseSource, sourceText, visitSyntax } = require('../scripts/typescript_source_tools.cjs');
test('autostart action patches current selection, cached details and list without rolling back concurrent fields', async () => {
  const filename = path.resolve(__dirname, '../src/hooks/useDesktopActions.ts');
  const source = fs.readFileSync(filename, 'utf8');
  let declaration;
  visitSyntax(parseSource(source, filename), node => {
    if (node.type === 'FunctionDeclaration' && node.identifier?.value === 'handleSaveAutostart') { declaration = node; return false; }
  });
  assert.ok(declaration);
  const gate = deferred();
  const saved = details('server-a', { settings_json: '{"name":"Old"}' });
  const latest = details('server-a', { settings_json: '{"name":"Changed meanwhile"}', backup_retention_count: 12 });
  const model = { selected: latest, cache: { 'server-a': latest }, bootstrap: { state: { instances: [latest.summary] } } };
  const options = {
    setSelectedInstanceDetails: update => model.selected = update(model.selected),
    setInstanceDetailsById: update => model.cache = update(model.cache),
    setBootstrap: update => model.bootstrap = update(model.bootstrap)
  };
  const run = vm.runInNewContext(transpileTypeScript(sourceText(source, declaration), filename) + '; handleSaveAutostart', {
    options, updateInstanceAutostart: () => gate.promise
  });
  const pending = run('server-a', false);
  gate.resolve({ ...saved, summary: { ...saved.summary, autostart: false } });
  await pending;
  for (const current of [model.selected, model.cache['server-a']]) {
    assert.equal(current.summary.autostart, false);
    assert.equal(current.settings_json, latest.settings_json);
    assert.equal(current.backup_retention_count, 12);
  }
  assert.equal(model.bootstrap.state.instances[0].autostart, false);
  model.selected = details('server-b');
  const previousSelected = model.selected;
  await run('server-a', false);
  assert.ok(Object.is(model.selected, previousSelected));
});

test("archived autostart uses the same control and refuses writes even when called directly", async () => {
  const state = harness();
  state.props.readOnly = true;
  const before = structuredClone(state.props.details);
  assert.equal(state.render().toggle.props.disabled, true);
  state.toggle(false);
  await settle();
  assert.deepEqual(state.calls, []);
  assert.deepEqual(state.props.details, before);
  assert.equal(state.render().toggle.props.checked, true);
  state.unmount();
});