const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const { resumeSteamCmdPreparation, queuedSteamCmdProgress } = require("../src/steamcmd-prepare-operation.ts");

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const flush = () => new Promise((resolve) => setImmediate(resolve));
const snapshot = (fields = {}) => ({ ...queuedSteamCmdProgress("existing"), phase: "updating", elapsed_seconds: 40, ...fields });

function setup() {
  const discovery = deferred(), probe = deferred();
  const reads = [], tasks = new Map(), values = [], statuses = [], errors = [], readErrors = [];
  let nextHandle = 0, recovered = 0, settled = 0, probed = 0;
  const operation = resumeSteamCmdPreparation({
    read: (id) => {
      if (id === null) return discovery.promise;
      assert.equal(id, "existing");
      const result = deferred(); reads.push(result); return result.promise;
    },
    probe: () => { probed += 1; return probe.promise; },
    ensure: () => assert.fail("recovery must never restart installation"),
    schedule: (callback, delay) => { assert.equal(delay, 1000); const handle = ++nextHandle; tasks.set(handle, callback); return handle; },
    cancel: (handle) => tasks.delete(handle),
    onRecovered: () => { recovered += 1; },
    onProgress: (value) => values.push(value), onStatus: (value) => statuses.push(value),
    onError: (value) => errors.push(value), onReadError: (value) => readErrors.push(value),
    onSettled: () => { settled += 1; }
  });
  return { operation, discovery, probe, reads, tasks, values, statuses, errors, readErrors,
    recovered: () => recovered, settled: () => settled, probed: () => probed,
    tick: () => { const [handle, callback] = tasks.entries().next().value; tasks.delete(handle); callback(); } };
}

test("refresh recovers the existing operation, polls its ID and probes status after success", async () => {
  const run = setup();
  run.discovery.resolve(snapshot());
  await flush();
  assert.equal(run.recovered(), 1);
  assert.equal(run.values[0].elapsed_seconds, 40);
  assert.equal(run.reads.length, 1);
  assert.equal(run.tasks.size, 0, "in-flight reads cannot overlap");
  run.reads[0].resolve(snapshot({ operation_id: "foreign" }));
  await flush();
  assert.equal(run.values.length, 1, "another operation cannot replace the recovered state");
  run.tick();
  run.reads[1].resolve(snapshot({ phase: "ready", active: false }));
  await flush();
  assert.equal(run.probed(), 1);
  assert.equal(run.settled(), 0, "busy remains owned until the final status is known");
  assert.equal(run.tasks.size, 0);
  run.probe.resolve({ executable_exists: true });
  await run.operation.finished;
  assert.deepEqual(run.statuses, [{ executable_exists: true }]);
  assert.equal(run.settled(), 1);
});

test("missing or already completed operations do not acquire busy state or start polling", async () => {
  for (const current of [null, snapshot({ active: false, phase: "ready" })]) {
    const run = setup();
    run.discovery.resolve(current);
    await run.operation.finished;
    assert.equal(run.recovered(), 0);
    assert.equal(run.reads.length, 0);
    assert.equal(run.settled(), 0);
    assert.equal(run.probed(), 0);
  }
});

test("a manual action or unmount invalidates a late discovery response", async () => {
  const run = setup();
  run.operation.dispose();
  run.discovery.resolve(snapshot());
  await run.operation.finished;
  assert.equal(run.recovered(), 0);
  assert.equal(run.values.length, 0);
  assert.equal(run.reads.length, 0);
  assert.equal(run.settled(), 0);
});

test("recovered failure preserves its diagnostic, releases busy state and never probes success", async () => {
  const run = setup();
  run.discovery.resolve(snapshot());
  await flush();
  const error = '{"code":"steamcmd_preparation_stalled","message":"stalled","timeout_seconds":90,"output_excerpt":"Connecting"}';
  run.reads[0].resolve(snapshot({ active: false, error }));
  await run.operation.finished;
  assert.deepEqual(run.errors, [error]);
  assert.equal(run.values[1].error, error);
  assert.equal(run.settled(), 1);
  assert.equal(run.probed(), 0);
  assert.equal(run.tasks.size, 0);
});

test("a superseded progress record terminates recovery explicitly instead of polling forever", async () => {
  const run = setup();
  run.discovery.resolve(snapshot());
  await flush();
  run.reads[0].resolve(null);
  await run.operation.finished;
  assert.equal(run.values[1].active, false);
  assert.equal(run.values[1].operation_id, "existing");
  assert.equal(JSON.parse(run.values[1].error).code, "steamcmd_prepare_progress_lost");
  assert.equal(run.errors[0], run.values[1].error);
  assert.equal(run.settled(), 1);
  assert.equal(run.probed(), 0);
  assert.equal(run.tasks.size, 0);
});

test("recovered cancellation ends observation without probing readiness or reporting failure", async () => {
  const run = setup();
  run.discovery.resolve(snapshot({ cancellable: true, cancel_requested: true }));
  await flush();
  run.reads[0].resolve(snapshot({ active: false, cancellable: false, cancel_requested: true, cancelled: true }));
  await run.operation.finished;
  assert.equal(run.settled(), 1);
  assert.equal(run.probed(), 0);
  assert.equal(run.errors.length, 0);
  assert.equal(run.tasks.size, 0);
});

test("unmount ends the observer and ignores an in-flight response", async () => {
  const run = setup();
  run.discovery.resolve(snapshot());
  await flush();
  run.operation.dispose();
  await run.operation.finished;
  run.reads[0].resolve(snapshot({ active: false, phase: "ready" }));
  await flush();
  assert.equal(run.values.length, 1);
  assert.equal(run.settled(), 0);
  assert.equal(run.probed(), 0);
  assert.equal(run.tasks.size, 0);
});

test("late status probe cannot overwrite another mounted owner", async () => {
  const run = setup();
  run.discovery.resolve(snapshot());
  await flush();
  run.reads[0].resolve(snapshot({ active: false, phase: "ready" }));
  await flush();
  run.operation.dispose();
  run.probe.resolve({ executable_exists: true });
  await run.operation.finished;
  assert.equal(run.statuses.length, 0);
  assert.equal(run.settled(), 0);
});

test("transient read failure retries without releasing the recovered operation", async () => {
  const run = setup();
  run.discovery.resolve(snapshot());
  await flush();
  run.reads[0].reject(new Error("temporarily unavailable"));
  await flush();
  assert.equal(run.readErrors[0].message, "temporarily unavailable");
  assert.equal(run.settled(), 0);
  assert.equal(run.tasks.size, 1);
  run.operation.dispose();
  await run.operation.finished;
  assert.equal(run.tasks.size, 0);
});
