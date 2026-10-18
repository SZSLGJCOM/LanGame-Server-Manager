const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../../scripts/typescript_source_tools.cjs");
const helpHarness = require("./configuration-help-harness.cjs");
const { InstanceSettingsSaveCoordinator } = require("../../src/views/settings/instance-settings-save-coordinator.ts");

// The autosave hook schedules through window timers; tests run them explicitly to observe debounce delays.
const timers = new Map();
let timerId = 0;
globalThis.window = globalThis.window ?? {};
Object.assign(globalThis.window, {
  setTimeout(callback, ms) { timers.set(++timerId, { callback, ms }); return timerId; },
  clearTimeout(id) { timers.delete(id); }
});
const settle = () => new Promise((resolve) => setImmediate(resolve));
function pendingDelays() { return [...timers.values()].map((timer) => timer.ms); }
async function runTimers() {
  while (timers.size) {
    const [id, timer] = timers.entries().next().value;
    timers.delete(id);
    timer.callback();
  }
  for (let index = 0; index < 4; index++) await settle();
}
function deferred() {
  let resolve; let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

/**
 * Renders one Maintenance editor with the real auto-save hook, queue and coordinator, a minimal hook runtime
 * and explicit timers. Hook-free presentational components named in `expand` are inlined for inspection.
 */
function createAutosaveHarness({ editorPath, exportName, props, expand = [] }) {
  timers.clear();
  const coordinator = new InstanceSettingsSaveCoordinator();
  const hooks = [];
  const layoutEffects = [];
  const effects = [];
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
  let idCounter = 0;
  const react = {
    ...React,
    useId() { const slot = cursor++; if (!(slot in hooks)) hooks[slot] = { value: `autosave-id-${++idCounter}` }; return hooks[slot].value; },
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
    useCallback(callback, deps) { return react.useMemo(() => callback, deps); },
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
  const autosave = load("../../src/views/settings/useAutoSaveInstanceSettings.ts", { "./InstanceSettingsSaveContext": coordinatorModule });
  const footer = load("../../src/views/servers/MaintenanceAutoSaveFooter.tsx", {});
  const editor = load(editorPath, {
    "../settings/InstanceSettingsSaveContext": coordinatorModule,
    "../settings/useAutoSaveInstanceSettings": autosave,
    "../settings/ConfigurationFieldHelp": helpHarness,
    "./MaintenanceAutoSaveFooter": footer
  });
  const inline = new Set([footer.MaintenanceAutoSaveFooter, helpHarness.ConfigurationHelp, helpHarness.ConfigurationActionHelp, ...expand]);
  function descendants(node) {
    if (React.isValidElement(node) && inline.has(node.type)) return descendants(node.type(node.props));
    if (Array.isArray(node)) return node.flatMap(descendants);
    if (!React.isValidElement(node)) return [];
    return [node, ...React.Children.toArray(node.props.children).flatMap(descendants)];
  }
  function inlineTree(node) {
    if (React.isValidElement(node) && inline.has(node.type)) return inlineTree(node.type(node.props));
    if (!React.isValidElement(node) || node.props.children === undefined) return node;
    return React.cloneElement(node, undefined, ...React.Children.toArray(node.props.children).map(inlineTree));
  }
  function render() {
    let root;
    for (let attempt = 0; attempt < 10; attempt++) {
      cursor = 0;
      const before = writes;
      root = editor[exportName](props);
      // Keyed wrappers render one child component; evaluate it with the same hook runtime.
      while (React.isValidElement(root) && typeof root.type === "function" && !inline.has(root.type)) root = root.type(root.props);
      while (layoutEffects.length) layoutEffects.shift()();
      while (effects.length) effects.shift()();
      if (before === writes) break;
      assert.ok(attempt < 9, "component effects must settle");
    }
    const nodes = descendants(root);
    const footerNode = nodes.find((node) => typeof node.props.className === "string"
      && node.props.className.split(/\s+/).includes("server-backup-policy-footer"));
    const buttons = nodes.filter((node) => node.type === "button");
    return {
      root, nodes, html: () => renderToStaticMarkup(inlineTree(root)),
      footer: footerNode, state: footerNode?.props["data-save-state"],
      input: (name) => nodes.find((node) => node.type === "input" && node.props.name === name),
      submit: buttons.find((node) => node.props.type === "submit"),
      retry: buttons.find((node) => node.props.className === "secondary-button"),
      discard: buttons.find((node) => node.props.className === "ghost-button")
    };
  }
  return {
    props, coordinator, render,
    edit(name, value) {
      const control = render().input(name);
      assert.ok(control, `missing control ${name}`);
      control.props.onChange({ target: { value, checked: value } });
      return render();
    },
    unmount() {
      for (const hook of hooks) hook?.cleanup?.();
    }
  };
}

module.exports = { createAutosaveHarness, deferred, pendingDelays, runTimers, settle };
