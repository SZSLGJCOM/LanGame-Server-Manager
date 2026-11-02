const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const ts = require("@typescript/typescript6");
const { parseSource, sourceText, transpileTypeScript, visitSyntax } = require("../scripts/typescript_source_tools.cjs");
for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (loaded, filename) => loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}
const { createRuntimeActionStore } = require("../src/runtime-action-store.ts");
const { isRuntimeStartCancelled } = require("../src/server-start-error.ts");

function action(file, name, context) {
  const filename = path.resolve(__dirname, file);
  const source = fs.readFileSync(filename, "utf8");
  let declaration;
  visitSyntax(parseSource(source, filename), node => {
    if (ts.isFunctionDeclaration(node) && node.name?.text === name) { declaration = node; return false; }
  });
  assert.ok(declaration, name);
  return vm.runInNewContext(transpileTypeScript(sourceText(source, declaration), filename) + "; " + name, context);
}

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function harness(overrides = {}) {
  const activities = [], calls = [], logs = [];
  const context = {
    options: {
      setActivity: value => activities.push(value),
      reloadBootstrap: async () => { calls.push("refresh"); },
      markRuntimeRefreshed() {},
      refreshInstallationState: async () => { calls.push("installation-refresh"); }
    },
    settingsSaveCoordinator: { flush: async () => {} },
    startInstance: async () => { calls.push("start"); return {}; },
    stopInstance: async () => { calls.push("stop"); return { outcome: "stopped" }; },
    message: (key, params, options) => ({ key, params, ...options }),
    serverStartSuccessMessage: () => ({ key: "activity.serverStartSent" }),
    serverStartFailureMessage: () => ({ key: "activity.startServerFailed" }),
    isRuntimeStartCancelled,
    readModuleNotReadyError: () => null,
    describeError: error => error instanceof Error ? error.message : String(error),
    logFrontendEvent: async (...args) => { logs.push(args); },
    reportOperationRefreshFailure: (_completed, error) => { throw error; },
    ...overrides
  };
  const file = "../src/hooks/useDesktopActions.ts";
  return { activities, calls, logs,
    start: action(file, "handleStartServer", context), stop: action(file, "handleStopServer", context) };
}

test("LAN HTTP startup identities retain UUID v4 format when randomUUID is unavailable", () => {
  let entropyReads = 0;
  const createId = action("../src/runtime-action-store.ts", "createStartupAttemptId", {
    crypto: { getRandomValues(bytes) {
      entropyReads++;
      for (let index = 0; index < bytes.length; index++) bytes[index] = index;
      return bytes;
    } }
  });
  assert.equal(createId(), "00010203-0405-4607-8809-0a0b0c0d0e0f");
  assert.equal(entropyReads, 1);
});

test("cancelling during the final settings barrier prevents the late start IPC", async () => {
  const flush = deferred();
  const store = createRuntimeActionStore();
  const start = store.begin("asa", "start");
  const actions = harness({ settingsSaveCoordinator: { flush: () => flush.promise } });
  const pendingStart = actions.start("asa", undefined, start);
  const stop = store.begin("asa", "stop");
  assert.equal(stop.localCancellation, true);
  await actions.stop("asa", stop);
  stop.finish();
  flush.resolve();
  assert.equal(await pendingStart, false);
  start.finish();
  assert.deepEqual(actions.calls, ["refresh"]);
  assert.equal(actions.activities.at(-1).key, "activity.serverStartupCancelled");
  assert.deepEqual(actions.logs, []);
  assert.deepEqual(store.getSnapshot(), {});
});

test("cancelling during the page preflight prevents invoking start and skips the live settings stop barrier", async () => {
  const entered = deferred(), flush = deferred();
  const store = createRuntimeActionStore(() => ({ ready: Promise.resolve(), dispose() {} }));
  const calls = [], activities = [];
  const run = action("../src/views/ServersView.tsx", "runRuntimeAction", {
    props: {
      instances: [{ id: "asa", module_id: "arksa", status: "Stopped" }], selectedDetails: null,
      onActivity: value => activities.push(value), onSelectInstance() {}, onWorkspaceSectionChange() {},
      onStart: async () => { calls.push("start"); },
      onStop: async (_id, operation) => {
        assert.equal(operation.localCancellation, true);
        calls.push("cancel");
      }
    },
    runtimeActionStore: store, collectRuntimeLogPaths: () => [],
    normalizeInstanceStatus: value => value?.toLowerCase(), setActiveDetailTab() {},
    message: key => ({ key }),
    settingsSaveCoordinator: {
      flush: () => { entered.resolve(); return flush.promise; },
      flushBeforeStop: async () => { calls.push("stop-settings"); }
    }
  });
  const pendingStart = run("asa", "start");
  await entered.promise;
  await run("asa", "stop");
  flush.resolve();
  await pendingStart;
  assert.deepEqual(calls, ["cancel"]);
  assert.equal(activities.at(-1).key, "activity.cancellingServerStartup");
  assert.deepEqual(store.getSnapshot(), {});
});

for (const lateResult of ["success", "cancellation", "failure"]) {
  test(`a taken-over start's late ${lateResult} cannot replace stop progress or completion`, async () => {
    const entered = deferred(), startReply = deferred(), stopReply = deferred();
    let stopCalls = 0;
    let submittedAttemptId;
    const actions = harness({
      startInstance: (id, preview, startupAttemptId) => {
        assert.equal(id, "asa");
        assert.equal(preview, null);
        assert.equal(typeof startupAttemptId, "string");
        submittedAttemptId = startupAttemptId;
        entered.resolve();
        return startReply.promise;
      },
      stopInstance: (id, startupAttemptId) => {
        assert.equal(id, "asa");
        assert.equal(startupAttemptId, submittedAttemptId, "Cancellation must target the submitted attempt even if transport delivery is reordered");
        stopCalls++;
        return stopReply.promise;
      }
    });
    const store = createRuntimeActionStore();
    const start = store.begin("asa", "start");
    const pendingStart = actions.start("asa", undefined, start);
    await entered.promise;
    assert.equal(submittedAttemptId, start.startupAttemptId);
    const stop = store.begin("asa", "stop");
    assert.equal(stop.localCancellation, false);
    const pendingStop = actions.stop("asa", stop);
    if (lateResult === "success") startReply.resolve({});
    else startReply.reject(new Error(lateResult === "cancellation"
      ? JSON.stringify({ code: "runtime_start_cancelled" }) : "late start failure"));
    assert.equal(await pendingStart, false);
    start.finish();
    assert.equal(store.getSnapshot().asa, "cancelling");
    assert.equal(actions.activities.at(-1).key, "activity.cancellingServerStartup");
    stopReply.resolve({ outcome: "startup_cancelled" });
    await pendingStop;
    stop.finish();
    assert.equal(stopCalls, 1);
    assert.equal(actions.activities.at(-1).key, "activity.serverStartupCancelled");
    assert.deepEqual(actions.logs, []);
  });
}

test("a server that completed startup before stop reports the backend's normal stop outcome", async () => {
  const store = createRuntimeActionStore();
  const start = store.begin("asa", "start");
  start.markStartDispatched();
  const stop = store.begin("asa", "stop");
  const actions = harness();
  await actions.stop("asa", stop);
  assert.deepEqual(actions.calls, ["stop", "refresh"]);
  assert.equal(actions.activities.at(-1).key, "activity.serverStopSent");
  start.finish();
  stop.finish();
});

test("ordinary running stop does not send a startup cancellation identity", async () => {
  const store = createRuntimeActionStore();
  const stop = store.begin("asa", "stop");
  const actions = harness({ stopInstance: async (...args) => {
    assert.deepEqual(args, ["asa"]);
    return { outcome: "stopped" };
  } });
  await actions.stop("asa", stop);
  assert.equal(actions.activities.at(-1).key, "activity.serverStopSent");
  stop.finish();
});

test("startup cancellation cleanup failure remains visible after a late start failure", async () => {
  const entered = deferred(), startReply = deferred();
  const actions = harness({
    startInstance: () => { entered.resolve(); return startReply.promise; },
    stopInstance: async () => { throw new Error("startup cleanup failed"); }
  });
  const store = createRuntimeActionStore();
  const start = store.begin("asa", "start");
  const pendingStart = actions.start("asa", undefined, start);
  await entered.promise;
  const stop = store.begin("asa", "stop");
  await actions.stop("asa", stop);
  stop.finish();
  startReply.reject(new Error("start worker failed during cancellation cleanup"));
  assert.equal(await pendingStart, false);
  start.finish();
  assert.equal(actions.activities.at(-1).key, "activity.cancelServerStartupFailed");
  assert.equal(actions.activities.at(-1).params.message, "startup cleanup failed");
  assert.equal(actions.activities.at(-1).tone, "error");
  assert.equal(actions.logs.length, 1);
  assert.equal(actions.logs[0][1], "instance.stop.frontend_failed");
  assert.deepEqual(actions.calls, []);
});

test("backend cancellation from another client is neutral and refreshes the actual stopped state", async () => {
  for (const error of [JSON.stringify({ code: "runtime_start_cancelled" }), new Error(JSON.stringify({ code: "runtime_start_cancelled" }))]) {
    const actions = harness({ startInstance: async () => { throw error; } });
    assert.equal(await actions.start("asa"), false);
    assert.deepEqual(actions.calls, ["refresh"]);
    assert.equal(actions.activities.at(-1).key, "activity.serverStartupCancelled");
    assert.notEqual(actions.activities.at(-1).tone, "error");
    assert.deepEqual(actions.logs, []);
  }
});

test("cancelling startup skips shutdown broadcasts, while a normal stop preserves them", async () => {
  const calls = [];
  const stop = action("../src/App.tsx", "handleStopServerWithBroadcast", {
    maybeRunLifecycleBroadcast: async (id, source) => { calls.push([id, source]); },
    handleStopServer: async id => { calls.push([id, "stop"]); }
  });
  await stop("asa", { cancelsStartup: true });
  assert.deepEqual(calls, [["asa", "stop"]]);
  calls.length = 0;
  await stop("asa");
  assert.deepEqual(calls, [["asa", "shutdown"], ["asa", "stop"]]);
});
