const ts = require("@typescript/typescript6");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { parseSource, sourceText, transpileTypeScript, visitSyntax } = require("../scripts/typescript_source_tools.cjs");

const filename = path.join(__dirname, "../src/App.tsx");
const source = fs.readFileSync(filename, "utf8");
function loadHandler(name, dependencies) {
  let declaration;
  visitSyntax(parseSource(source, filename), node => {
    if (ts.isFunctionDeclaration(node) && node.name?.text === name) { declaration = node; return false; }
  });
  assert.ok(declaration, `The application must own ${name}`);
  const exports = {};
  vm.runInNewContext(transpileTypeScript(`${sourceText(source, declaration)}\nexport { ${name} };`, "migration-completion.ts"), {
    exports, message: (key, params) => ({ key, params: { ...params } }),
    describeError: error => error.message, ...dependencies
  }, { filename });
  return exports[name];
}
function deferred() { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; }
const flush = () => new Promise(resolve => setImmediate(resolve));
const input = Object.freeze({ games_root: "D:/games", servers_root: "D:/instances", archives_root: "D:/instances/.trash", steamcmd_root: "D:/steamcmd" });
const completed = Object.freeze({ operation_id: "migration-one", phase: "completed", input, cleanup_warnings: ["E:/locked remains"] });
function setup(overrides = {}) {
  const events = [];
  let bootstrap = { state: { settings: { games_root: "E:/games", modules_root: "E:/modules" }, instances: [{ id: "kept" }] } };
  const complete = loadHandler("handleStorageMigrationCompleted", {
    setBootstrap: change => { events.push("commit-confirmed-paths"); bootstrap = change(bootstrap); },
    getCurrentBootstrap: () => bootstrap,
    instancePanelReader: { invalidate: id => { assert.equal(id, "kept"); events.push("invalidate-instance"); } },
    setInstanceDetailsById: cache => { assert.equal(Object.keys(cache).length, 0); events.push("clear-cache"); },
    clearSelectedInstancePanel: () => { events.push("clear-selected-panel"); },
    setPanelRetryGeneration: advance => { assert.equal(advance(3), 4); events.push("refresh-selected-panel"); },
    reloadBootstrap: async () => { events.push("reload"); },
    runSteamCmdProbe: async options => { events.push("probe"); assert.equal(options.propagateError, true); },
    ...overrides
  });
  return { complete, events, bootstrap: () => bootstrap };
}

test("migration completion publishes committed paths before refreshing bootstrap and SteamCMD", async () => {
  const reload = deferred(), probe = deferred(), events = [];
  const run = setup({ reloadBootstrap: () => { events.push("reload"); return reload.promise; },
    runSteamCmdProbe: options => { assert.equal(options.propagateError, true); events.push("probe"); return probe.promise; } });
  let finished = false;
  const refreshing = run.complete(completed).then(() => { finished = true; });
  assert.deepEqual({ ...run.bootstrap().state.settings }, { ...input, modules_root: "E:/modules" });
  assert.equal(run.bootstrap().state.instances[0].id, "kept");
  assert.deepEqual(events, ["reload"]); assert.equal(finished, false);
  reload.resolve(); await flush();
  assert.deepEqual(events, ["reload", "probe"]); assert.equal(finished, false);
  probe.resolve(); await refreshing; assert.equal(finished, true);
});

test("a bootstrap refresh failure retains the committed paths and returns its actual cause", async () => {
  const error = new Error("bootstrap temporarily unavailable");
  const run = setup({ reloadBootstrap: async () => { throw error; } });
  await assert.rejects(run.complete(completed), failure => failure === error);
  assert.equal(run.bootstrap().state.settings.servers_root, input.servers_root);
  assert.deepEqual(run.events, ["commit-confirmed-paths", "invalidate-instance", "clear-cache", "clear-selected-panel", "refresh-selected-panel"]);
  assert.equal(completed.phase, "completed");
});

test("retrying post-migration refresh repeats only reads and preserves cleanup warnings", async () => {
  let probes = 0, reloads = 0;
  const error = new Error("SteamCMD probe failed");
  const run = setup({ reloadBootstrap: async () => { reloads++; }, runSteamCmdProbe: async () => { if (++probes === 1) throw error; } });
  await assert.rejects(run.complete(completed), failure => failure === error);
  await run.complete(completed);
  assert.equal(reloads, 2); assert.equal(probes, 2);
  assert.equal(run.bootstrap().state.settings.steamcmd_root, input.steamcmd_root);
  assert.deepEqual(completed.cleanup_warnings, ["E:/locked remains"]);
});

test("SteamCMD probe propagates failures for migration refresh while retaining ordinary probe behavior", async () => {
  const error = new Error("native probe failed");
  const busy = [], statuses = [];
  const probe = loadHandler("runSteamCmdProbe", {
    steamCmdOperationBusyRef: { current: false }, steamCmdProbeTokenRef: { current: 0 },
    setSteamCmdProgress: () => {}, setSteamCmdBusy: value => busy.push(value), setSteamCmdMessage: () => {},
    setSteamCmdStatus: value => statuses.push(value), setActivity: () => {}, probeSteamCmdStatus: async () => { throw error; }
  });
  await assert.rejects(probe({ propagateError: true }), failure => failure === error);
  await probe();
  assert.deepEqual(busy, [true, false, true, false]); assert.deepEqual(statuses, [null, null]);
});
