const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { queuedSteamCmdProgress } = require("../src/steamcmd-prepare-operation.ts");
const { isInstallationCancelled } = require("../src/installation-cancellation.ts");

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function hookHarness(filename, exportName, dependencies, initialArgs) {
  const cells = [], cleanups = [];
  let cursor = 0, dirty = false, effects = [], args = initialArgs;
  const react = {
    useRef(value) { const index = cursor++; return cells[index] ??= { current: value }; },
    useState(value) {
      const index = cursor++;
      cells[index] ??= { value: typeof value === "function" ? value() : value };
      return [cells[index].value, (update) => { const next = typeof update === "function" ? update(cells[index].value) : update;
        if (!Object.is(next, cells[index].value)) { cells[index].value = next; dirty = true; } }];
    },
    useEffect(callback, deps) {
      const index = cursor++;
      if (!cells[index] || deps.some((value, n) => !Object.is(value, cells[index][n]))) {
        cells[index] = deps;
        effects.push(() => { cleanups[index]?.(); cleanups[index] = callback(); });
      }
    }
  };
  const exports = {};
  const sourcePath = path.join(__dirname, "../src/hooks", filename);
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(sourcePath, "utf8"), sourcePath), {
    exports, Error, crypto: { randomUUID: () => "started" }, window: { setTimeout, clearTimeout },
    require: (id) => id === "react" ? react : dependencies[id] ?? assert.fail(`Unexpected dependency ${id}`)
  });
  return {
    render(nextArgs = args) {
      args = nextArgs;
      let result;
      for (let round = 0; round < 6; round++) {
        cursor = 0; dirty = false; effects = [];
        result = exports[exportName](args);
        effects.forEach((effect) => effect());
        if (!dirty) return result;
      }
      assert.fail("hook state did not settle");
    },
    unmount() { cleanups.forEach((cleanup) => cleanup?.()); }
  };
}

const job = (extra = {}) => ({ id: "game", kind: "DownloadGame", status: "Running", cancellable: true, cancel_requested: false, ...extra });

test("game stop acknowledgement holds stopping until the job reader confirms completion", async () => {
  const pending = deferred(); let calls = 0;
  const harness = hookHarness("useInstallationCancellation.ts", "useInstallationCancellation", {
    "../api": { cancelInstallationJob: (id) => { assert.equal(id, "game"); calls++; return pending.promise; } },
    "../installation-job": require("../src/installation-job.ts")
  }, [job()]);
  let actions = harness.render();
  const request = actions.handleCancelInstallation("game");
  await actions.handleCancelInstallation("game");
  assert.equal(calls, 1);
  assert.equal(harness.render().installationStopPendingIds.includes("game"), true);
  pending.resolve(job({ cancel_requested: true }));
  await request;
  actions = harness.render([job()]);
  assert.equal(actions.installationStopPendingIds.includes("game"), true, "a stale active poll cannot clear stopping");
  actions = harness.render([job({ status: "Cancelled", cancellable: false })]);
  assert.equal(actions.installationStopPendingIds.length, 0);
  harness.unmount();
});

test("game stop failure stays visible, allows retry, and never stops an unrelated or completed job", async () => {
  let calls = 0;
  const harness = hookHarness("useInstallationCancellation.ts", "useInstallationCancellation", {
    "../api": { cancelInstallationJob: async () => { calls++; throw new Error("stop rejected"); } },
    "../installation-job": require("../src/installation-job.ts")
  }, [job()]);
  await harness.render().handleCancelInstallation("other");
  assert.equal(calls, 0);
  await harness.render().handleCancelInstallation("game");
  let actions = harness.render();
  assert.equal(actions.installationStopPendingIds.length, 0);
  assert.equal(actions.installationStopErrors.game, "stop rejected");
  await actions.handleCancelInstallation("game");
  assert.equal(calls, 2);
  actions = harness.render([job({ status: "Completed" })]);
  await actions.handleCancelInstallation("game");
  assert.equal(calls, 2);
  assert.equal(actions.installationStopErrors.game, undefined);
  harness.unmount();
});

function steamHarness(cancel) {
  const preparation = deferred(), discovery = deferred(), busy = [], messages = [];
  let startCallbacks, recoveryCallbacks, ensureCalls = 0;
  const options = { setSteamCmdBusy: (value) => busy.push(value), setSteamCmdProgress() {}, setSteamCmdStatus() {},
    setSteamCmdMessage: (value) => messages.push(value), setActivity() {}, onSteamCmdOperationStart() {} };
  const harness = hookHarness("useSteamCmdActions.ts", "useSteamCmdActions", {
    "../api": { cancelSteamCmdPreparation: cancel },
    "../app-ui": { message: (key, params) => ({ key, params }) },
    "../steamcmd-ui": {},
    "../steamcmd-prepare-operation": {
      startSteamCmdPreparation: (callbacks) => { ensureCalls++; startCallbacks = callbacks;
        callbacks.onProgress({ ...queuedSteamCmdProgress("started"), cancellable: true });
        return { finished: preparation.promise, dispose() {} }; },
      resumeSteamCmdPreparation: (callbacks) => { recoveryCallbacks = callbacks; return { finished: discovery.promise, dispose() {} }; }
    }
  }, options);
  return { harness, busy, messages, preparation, start: () => startCallbacks, recover: () => recoveryCallbacks, ensureCalls: () => ensureCalls };
}

test("SteamCMD stop cannot release busy on acknowledgement and cannot duplicate the request", async () => {
  const ack = deferred(); let calls = 0;
  const run = steamHarness((id) => { assert.equal(id, "started"); calls++; return ack.promise; });
  const actions = run.harness.render();
  const operation = actions.handleEnsureSteamCmd();
  const request = actions.handleCancelSteamCmd("started");
  await actions.handleCancelSteamCmd("started");
  assert.equal(calls, 1);
  ack.resolve({ ...queuedSteamCmdProgress("started"), cancel_requested: true });
  await request;
  assert.equal(run.harness.render().steamCmdStopPending, true);
  assert.equal(run.busy.includes(false), false);
  run.start().onProgress({ ...queuedSteamCmdProgress("started"), active: false, cancelled: true });
  run.start().onSettled(); run.preparation.resolve(); await operation;
  assert.equal(run.harness.render().steamCmdStopPending, false);
  assert.equal(run.messages[run.messages.length - 1].key, "steamcmd.cancelled");
  assert.equal(run.busy[run.busy.length - 1], false);
  run.harness.unmount();
});

test("recovered SteamCMD preparation can be stopped without restarting installation", async () => {
  let calls = 0;
  const run = steamHarness(async (id) => { assert.equal(id, "recovered"); calls++; throw new Error("not accepted"); });
  run.harness.render();
  run.recover().onRecovered();
  run.recover().onProgress({ ...queuedSteamCmdProgress("recovered"), cancellable: true });
  await run.harness.render().handleCancelSteamCmd("recovered");
  assert.equal(run.harness.render().steamCmdStopError, "not accepted");
  assert.equal(run.harness.render().steamCmdStopPending, false);
  assert.equal(run.busy.includes(false), false);
  await run.harness.render().handleCancelSteamCmd("recovered");
  assert.equal(calls, 2);
  assert.equal(run.ensureCalls(), 0);
  run.harness.unmount();
});

test("late stop failure cannot overwrite a completed SteamCMD result", async () => {
  const ack = deferred();
  const run = steamHarness(() => ack.promise);
  run.harness.render(); run.recover().onRecovered();
  run.recover().onProgress({ ...queuedSteamCmdProgress("recovered"), cancellable: true });
  const request = run.harness.render().handleCancelSteamCmd("recovered");
  run.recover().onProgress({ ...queuedSteamCmdProgress("recovered"), active: false, phase: "ready" });
  run.recover().onSettled();
  ack.reject(new Error("late")); await request;
  assert.equal(run.harness.render().steamCmdStopError, null);
  run.harness.unmount();
});

test("backend cancellation remains stopped with its diagnostic log suffix", () => {
  for (const message of ["installation_cancelled", "installation_cancelled. See app log: D:/User files/app.log",
    "installation_cancelled. Application diagnostic log unavailable: access denied (1 record(s) not persisted). Log path: D:/User files/app.log"]) {
    assert.equal(isInstallationCancelled(message), true);
    assert.equal(isInstallationCancelled(new Error(message)), true);
  }
});

test("failures mentioning cancellation must not be reported as a successful stop", () => {
  for (const error of [new Error("installation failed"), "installation_cancelled: process cleanup failed",
    "Could not complete installation_cancelled. See app log: D:/User files/app.log", null, { message: "installation_cancelled" }]) {
    assert.equal(isInstallationCancelled(error), false);
  }
});
