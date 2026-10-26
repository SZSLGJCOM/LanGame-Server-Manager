const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { parseSource, sourceText, transpileTypeScript, visitSyntax } = require("../../scripts/typescript_source_tools.cjs");

function deferred() {
  let resolve, reject;
  const promise = new Promise((accept, fail) => { resolve = accept; reject = fail; });
  return { promise, resolve, reject };
}

async function flush() {
  for (let index = 0; index < 30; index++) await Promise.resolve();
}

function instanceActionFixture(overrides = {}) {
  const details = { summary: { id: "a", name: "Server A" }, settings_json: "{}", ports: [] };
  const log = { lines: ["current log"], source_path: "a/server.log", read_error: null };
  const receipt = { instance_id: "a", display_name: "Server A", response_text: "accepted" };
  const state = {
    selectedId: "a", selected: null, cache: {}, activity: [], progress: [], settled: [],
    calls: [], bootstrapRefreshes: 0, instances: [{ id: "a" }, { id: "b" }], retiring: new Set(),
    detailsById: {}, backupsById: {}, opened: [], runtimeRefreshes: 0
  };
  const api = {
    readInstanceDetails: async () => details,
    listInstanceBackups: async () => [],
    readInstanceRuntime: async () => ({ log_tail: log }),
    readInstanceRuntimeWindowSnapshot: async () => ({ windows: [] }),
    readInstanceLogDocument: async () => log,
    previewInstanceLaunch: async () => ({ executable: "server" }),
    sendInstanceRuntimeCommand: async () => receipt,
    logFrontendEvent: async () => {},
    ...overrides.api
  };
  for (const [name, execute] of Object.entries(api)) {
    api[name] = (...args) => { state.calls.push({ name, args }); return execute(...args); };
  }
  const timers = new Map();
  let nextTimer = 0;
  const cache = new Map();
  const appUi = { message: (key, params, options) => ({ key, params, ...options }) };
  const dependencies = {
    react: {
      useRef: (value) => ({ current: value }),
      useState: (initial) => [typeof initial === "function" ? initial() : initial, () => {}]
    },
    "./api": api,
    "../api": api,
    "../app-ui": appUi, "./app-ui": appUi,
    "./useSteamCmdActions": {}, "../install-state-presentation": {}, "../steam-installation-account": {},
    "../i18n": { useI18n: () => ({ t: (key, params) => `${key}: ${params?.error ?? ""}` }) },
    "../views/settings/InstanceSettingsSaveContext": { useInstanceSettingsSaveCoordinator: () => ({ flush: async () => {} }) }
  };
  function load(relativePath) {
    const filename = path.resolve(__dirname, "../../src", relativePath);
    if (cache.has(filename)) return cache.get(filename);
    const exports = {};
    cache.set(filename, exports);
    vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
      exports, Error, AbortController,
      setTimeout: (callback) => { timers.set(++nextTimer, callback); return nextTimer; },
      clearTimeout: (id) => timers.delete(id),
      require: (id) => {
        if (Object.hasOwn(dependencies, id)) return dependencies[id];
        assert.ok(["app-state", "instance-panel-loader", "instance-panel-refresh", "server-start-error",
          "runtime-start-warning", "installation-cancellation"].some((name) => id.endsWith(name)),
          `Unexpected dependency ${id}`);
        return load(path.relative(path.resolve(__dirname, "../../src"), path.resolve(path.dirname(filename), `${id}.ts`)));
      }
    }, { filename });
    return exports;
  }
  const appFilename = path.resolve(__dirname, "../../src/App.tsx");
  const appSource = fs.readFileSync(appFilename, "utf8");
  let selectedPanelHandler;
  visitSyntax(parseSource(appSource, appFilename), (node) => {
    if (node.name?.text === "handleSelectedPanelLoaded" && node.initializer) {
      selectedPanelHandler = node;
      return false;
    }
  });
  assert.ok(selectedPanelHandler, "App owns the selected panel publication boundary");
  const onPanelLoaded = vm.runInNewContext(transpileTypeScript(
    `const ${sourceText(appSource, selectedPanelHandler)}; handleSelectedPanelLoaded`, appFilename
  ), {
    useEffectEvent: (handler) => handler,
    retirement: { isPending: (id) => state.retiring.has(id) },
    getCurrentBootstrap: () => ({ state: { instances: state.instances } }),
    selectedInstanceCursor: { current: () => state.selectedId },
    cacheInstancePanel: (id, patch) => { state.cache[id] = { ...state.cache[id], ...patch }; },
    applySelectedInstancePanel: (patch) => { state.selected = { ...state.selected, ...patch }; }
  });
  const options = {
    getCurrentInstanceId: () => state.selectedId,
    retirement: { isPending: () => false },
    instanceDetailsById: state.detailsById,
    setInstanceDetailsById: (update) => { state.detailsById = update(state.detailsById); },
    setInstanceBackupsById: (update) => { state.backupsById = update(state.backupsById); },
    setSelectedInstanceBackups: (value) => { state.selectedBackups = value; },
    setSelectedInstanceId: (value) => { state.selectedId = value; },
    markRuntimeRefreshed: () => { state.runtimeRefreshes++; },
    openInstanceView: (section, id) => state.opened.push({ section, id }),
    onPanelLoaded,
    onPanelProgress: (value) => state.progress.push(value),
    onPanelSettled: (value) => state.settled.push(value),
    reloadBootstrap: async () => {
      state.bootstrapRefreshes++;
      await overrides.reloadBootstrap?.();
    },
    setActivity: (value) => state.activity.push(value)
  };
  const actions = load("hooks/useDesktopActions.ts").useInstanceActions(options);
  return {
    actions, state, details, receipt, reader: () => load("instance-panel-loader.ts").instancePanelReader,
    expireReads: () => { for (const callback of [...timers.values()]) callback(); }
  };
}

module.exports = { deferred, flush, instanceActionFixture };
