const assert = require("node:assert/strict");
const fs = require("node:fs");
const { createRequire } = require("node:module");
const vm = require("node:vm");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".css"] = () => {};
require.extensions[".tsx"] = require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const { WorkshopDownloadOperations, activeWorkshopDownloadJob } = require("../src/views/servers/workshop-download-operations.ts");
const { InstanceSettingsSaveCoordinator } = require("../src/views/settings/instance-settings-save-coordinator.ts");
const request = (...ids) => ({ ids, contentIds: [...ids], items: {}, locale: "zh-CN" });
const job = (target = "server-a", overrides = {}) => ({ id: `workshop-download:${target}:one`, target_id: target,
  kind: "DownloadWorkshop", status: "Running", progress_percent: 42, cancellable: false, cancel_requested: false,
  label: "Download", ...overrides });
const deferred = () => Promise.withResolvers();
const tick = () => new Promise(setImmediate);
const entries = (store, instance = "server-a") => store.snapshot(instance).requests;
const apply = (body = async () => {}) => (operation) => operation.apply(() => body(operation));

test("LAN HTTP requests generate canonical UUID attempts when randomUUID is unavailable", async () => {
  const filename = require.resolve("../src/views/servers/workshop-download-operations.ts"), exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports, crypto: { getRandomValues: (bytes) => crypto.getRandomValues(bytes) },
    require: (id) => createRequire(filename)(id)
  });
  const store = new exports.WorkshopDownloadOperations();
  const tokens = [];
  await store.enqueue("server-a", request("1"), async (operation) => {
    tokens.push(operation.beginAttempt(), operation.beginAttempt());
    await operation.apply(async () => {});
  });
  for (const token of tokens) assert.match(token, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  assert.notEqual(tokens[0], tokens[1]);
  assert.equal(entries(store)[0].jobId, `workshop-download:server-a:${tokens[1]}`);
});

test("two preparations run together while ready tickets and settings saves preserve commit FIFO", async () => {
  const store = new WorkshopDownloadOperations();
  const preparations = [deferred(), deferred(), deferred()], saves = [deferred(), deferred(), deferred()];
  const preparing = [], applying = [];
  const completions = preparations.map((gate, index) => store.enqueue("server-a", request(String(index)), async (operation) => {
    preparing.push(index);
    await gate.promise;
    await operation.apply(async () => { applying.push(index); await saves[index].promise; });
  }));
  const accepted = store.snapshot("server-a");
  assert.deepEqual(accepted.requests.map((entry) => entry.status), ["queued", "queued", "queued"]);
  assert.deepEqual(preparing, [], "Admission registers startup barriers before beginning any preparation");
  await tick();
  assert.deepEqual(preparing, [0, 1]);
  assert.deepEqual(entries(store).map((entry) => entry.status), ["preparing", "preparing", "queued"]);
  preparations[1].resolve();
  await tick();
  assert.deepEqual(applying, [], "A later fast download cannot deploy before the earlier request");
  assert.deepEqual(entries(store).map((entry) => entry.status), ["preparing", "ready", "queued"]);
  preparations[0].resolve();
  await tick();
  assert.deepEqual(applying, [0]);
  assert.deepEqual(preparing, [0, 1], "Ready tickets and pending settings saves retain their global slots");
  saves[0].resolve();
  await completions[0];
  await tick();
  assert.deepEqual(preparing, [0, 1, 2]);
  assert.deepEqual(applying, [0, 1]);
  preparations[2].resolve();
  await tick();
  assert.equal(entries(store)[2].status, "ready");
  saves[1].resolve();
  await completions[1];
  await tick();
  assert.deepEqual(applying, [0, 1, 2]);
  saves[2].resolve();
  await completions[2];
  assert.deepEqual(entries(store).map((entry) => entry.status), ["success", "success", "success"]);
  assert.deepEqual(accepted.requests.map((entry) => entry.status), ["queued", "queued", "queued"], "Published snapshots remain immutable");
});

test("the two whole-request slots are global across instances and retain admission order", async () => {
  const store = new WorkshopDownloadOperations();
  const gates = [deferred(), deferred(), deferred(), deferred()], started = [], committed = [];
  const completions = ["server-a", "server-b", "server-a", "server-c"].map((instance, index) => store.enqueue(instance, request(String(index)), async (operation) => {
    started.push(index);
    await gates[index].promise;
    await operation.apply(async () => { committed.push(index); });
  }));
  await tick();
  assert.deepEqual(started, [0, 1]);
  gates[1].resolve();
  await completions[1];
  await tick();
  assert.deepEqual(started, [0, 1, 2]);
  gates[2].resolve();
  await tick();
  assert.equal(entries(store)[1].status, "ready");
  assert.deepEqual(committed, [1], "Different instances commit independently; A's later request still waits");
  assert.equal(entries(store, "server-c")[0].status, "queued");
  gates[0].resolve();
  await Promise.all(completions.slice(0, 3));
  await tick();
  assert.deepEqual(started, [0, 1, 2, 3]);
  assert.deepEqual(committed, [1, 0, 2]);
  gates[3].resolve();
  await completions[3];
});

test("an early preparation failure cannot let a third commit overtake an unfinished first request", async () => {
  const store = new WorkshopDownloadOperations(), firstGate = deferred(), committed = [];
  const first = store.enqueue("server-a", request("1"), async (operation) => {
    await firstGate.promise;
    await operation.apply(async () => { committed.push(1); });
  });
  const failed = assert.rejects(store.enqueue("server-a", request("2"), async () => { throw new Error("preparation failed"); }), /preparation failed/);
  const third = store.enqueue("server-a", request("3"), apply(async () => { committed.push(3); }));
  await failed;
  await tick();
  assert.deepEqual(entries(store).map((entry) => entry.status), ["preparing", "error", "ready"]);
  assert.deepEqual(committed, []);
  firstGate.resolve();
  await Promise.all([first, third]);
  assert.deepEqual(committed, [1, 3]);
});

test("synchronous duplicates are rejected while unrelated and partially overlapping groups remain complete", async () => {
  const store = new WorkshopDownloadOperations(), gate = deferred();
  const first = store.enqueue("server-a", request("123456"), apply(() => gate.promise));
  const second = store.enqueue("server-a", request("654321"), apply());
  const duplicate = () => { assert.fail("A duplicate must never execute"); };
  assert.equal(store.enqueue("server-a", request("123456"), duplicate), null);
  assert.equal(store.enqueue("server-a", request("654321"), duplicate), null);
  assert.equal(store.enqueue("server-a", request("123456", "654321"), duplicate), null);
  const collection = store.enqueue("server-a", request("123456", "654321", "777777"), apply());
  assert.ok(collection);
  assert.deepEqual(entries(store)[2].contentIds, ["123456", "654321", "777777"], "Overlap cannot strip a group's original intent");
  gate.resolve();
  await Promise.all([first, second, collection]);
});

test("accepted downloads survive subscribers leaving and keep instances isolated", async () => {
  const store = new WorkshopDownloadOperations(), firstGate = deferred(), secondGate = deferred();
  let firstOperation;
  const unsubscribe = store.subscribe(() => {});
  const first = store.enqueue("server-a", request("123456"), async (operation) => {
    firstOperation = operation;
    operation.beginAttempt();
    await firstGate.promise;
    await operation.apply(async () => {});
  });
  const second = store.enqueue("server-b", request("123456"), apply(() => secondGate.promise));
  await tick();
  const current = entries(store)[0], own = job("server-a", { id: current.jobId });
  store.recordJob("server-a", current.requestId, current.attemptToken, own);
  unsubscribe();
  assert.equal(entries(store)[0].job.progress_percent, 42);
  assert.equal(entries(store, "server-b")[0].status, "applying");
  assert.equal(entries(store, "server-b")[0].job, null);
  firstGate.resolve();
  await first;
  assert.throws(() => firstOperation.beginAttempt(), /no longer active/);
  store.recordJob("server-a", current.requestId, current.attemptToken, own);
  assert.equal(entries(store)[0].status, "success");
  assert.equal(entries(store)[0].job, null, "Completed requests ignore late progress");
  assert.equal(entries(store, "server-b")[0].status, "applying");
  secondGate.resolve();
  await second;
});

test("failed deployments reject their completion and preserve their error while later commits continue", async () => {
  const store = new WorkshopDownloadOperations(), firstGate = deferred(), secondGate = deferred();
  const rejected = assert.rejects(store.enqueue("server-a", request("123456"), apply(() => firstGate.promise)), /deployment failed/);
  const second = store.enqueue("server-a", request("654321"), apply(() => secondGate.promise));
  await tick();
  firstGate.reject(new Error("deployment failed"));
  await rejected;
  await tick();
  assert.deepEqual(entries(store).map((entry) => entry.status), ["error", "applying"]);
  secondGate.resolve();
  await second;
  assert.deepEqual(entries(store).map((entry) => entry.status), ["error", "success"]);
  assert.equal(entries(store)[0].error, "deployment failed", "Another successful Mod cannot hide the failure");
});

test("a completed native job at 100 percent does not finish configuration or release its slot", async () => {
  const store = new WorkshopDownloadOperations(), gate = deferred();
  const first = store.enqueue("server-a", request("123456"), apply((operation) => { operation.beginAttempt(); return gate.promise; }));
  const rejected = assert.rejects(first, /settings failed/);
  const second = store.enqueue("server-a", request("654321"), apply());
  const third = store.enqueue("server-b", request("777777"), apply());
  await tick();
  const current = entries(store)[0];
  store.recordJob("server-a", current.requestId, current.attemptToken, job("server-a", { id: current.jobId, progress_percent: 100, status: "Completed" }));
  assert.deepEqual(entries(store).map((entry) => entry.status), ["applying", "ready"]);
  assert.equal(entries(store, "server-b")[0].status, "queued");
  gate.reject(new Error("settings failed"));
  await Promise.all([rejected, second, third]);
  assert.equal(entries(store)[0].status, "error");
  assert.equal(entries(store)[0].job, null);
});

test("external activity ignores startup Workshop jobs, finished jobs and other instances", () => {
  const manual = job();
  assert.equal(activeWorkshopDownloadJob([job("server-a", { id: "instance-workshop-update:server-a:one" }),
    job("server-a", { status: "Completed" }), job("server-b"), manual], "server-a"), manual);
});

test("concurrent attempts accept only exact jobs and reject late account retry progress", async () => {
  const store = new WorkshopDownloadOperations(), gates = [deferred(), deferred()], operations = [];
  const completions = gates.map((gate, index) => store.enqueue("server-a", request(String(index)), async (operation) => {
    operations[index] = operation;
    operation.beginAttempt();
    await gate.promise;
    await operation.apply(async () => {});
  }));
  await tick();
  const [first, second] = entries(store);
  const firstJob = job("server-a", { id: first.jobId, progress_percent: 37 });
  const secondJob = job("server-a", { id: second.jobId, progress_percent: 24 });
  store.recordJob("server-a", first.requestId, first.attemptToken, job());
  store.recordJob("server-a", first.requestId, first.attemptToken, secondJob);
  assert.equal(entries(store)[0].job, null);
  store.recordJob("server-a", first.requestId, first.attemptToken, firstJob);
  store.recordJob("server-a", second.requestId, second.attemptToken, secondJob);
  assert.deepEqual(entries(store).map((entry) => entry.job.progress_percent), [37, 24]);
  const newToken = operations[0].beginAttempt(), retryJob = job("server-a", { id: `workshop-download:server-a:${newToken}` });
  assert.notEqual(newToken, first.attemptToken);
  assert.equal(entries(store)[0].requestId, first.requestId);
  store.recordJob("server-a", first.requestId, first.attemptToken, retryJob);
  store.recordJob("server-a", first.requestId, newToken, firstJob);
  assert.equal(entries(store)[0].job, null, "Neither stale token nor stale job can claim the retry");
  store.recordJob("server-a", first.requestId, newToken, retryJob);
  assert.equal(entries(store)[0].job.id, retryJob.id);
  gates.forEach((gate) => gate.resolve());
  await Promise.all(completions);
});

test("retry replaces its own terminal entry without clearing other failures", async () => {
  const store = new WorkshopDownloadOperations();
  await assert.rejects(store.enqueue("server-a", request("123456", "654321"), async () => { throw new Error("first failed"); }));
  await assert.rejects(store.enqueue("server-a", request("777777"), async () => { throw "second failed"; }));
  const previousId = entries(store)[0].requestId;
  const retry = store.enqueue("server-a", request("654321", "123456"), apply());
  assert.equal(entries(store).length, 2);
  assert.equal(entries(store)[0].error, "second failed");
  assert.equal(entries(store)[1].status, "queued");
  assert.notEqual(entries(store)[1].requestId, previousId);
  await retry;
  assert.deepEqual(entries(store).map((entry) => entry.status), ["error", "success"]);
});

test("bounded success history never discards unfinished downloads", async () => {
  const store = new WorkshopDownloadOperations(), gate = deferred();
  const first = store.enqueue("pending", request("123456"), apply(() => gate.promise));
  for (let i = 0; i < 40; i++) await store.enqueue(`server-${i}`, request("777777"), apply());
  assert.equal(entries(store, "pending")[0].status, "applying");
  assert.deepEqual(entries(store, "server-0"), []);
  assert.equal(entries(store, "server-39")[0].status, "success");
  for (let i = 0; i < 40; i++) await store.enqueue("history", request(String(i)), apply());
  assert.equal(entries(store, "history").length, 32);
  assert.deepEqual(entries(store, "history")[0].ids, ["8"]);
  const second = store.enqueue("pending", request("654321"), apply()), third = store.enqueue("pending", request("888888"), apply());
  await tick();
  assert.deepEqual(entries(store, "pending").map((entry) => entry.status), ["applying", "ready", "queued"]);
  gate.resolve();
  await Promise.all([first, second, third]);
});

// Exercise the real hook and coordinator; replace only React's renderer boundary.
function hookHarness() {
  const coordinator = new InstanceSettingsSaveCoordinator();
  const filename = require.resolve("../src/views/servers/useWorkshopDownloadOperation.ts"), module = { exports: {} };
  const localRequire = createRequire(filename);
  const dependencies = {
    react: { useCallback: (callback) => callback, useEffect: () => {}, useSyncExternalStore: (_subscribe, snapshot) => snapshot() },
    "../../api": { readBackgroundJobs: async () => [] },
    "../settings/InstanceSettingsSaveContext": { useInstanceSettingsSaveCoordinator: () => coordinator },
  };
  new Function("require", "module", "exports", transpileTypeScript(fs.readFileSync(filename, "utf8"), filename))(
    (id) => dependencies[id] ?? localRequire(id), module, module.exports);
  return { coordinator, render: module.exports.useWorkshopDownloadOperation };
}

test("startup owns accepted preparation, ready and queued work immediately across remounts", async () => {
  const { coordinator, render } = hookHarness(), gates = [deferred(), deferred(), deferred()];
  const completions = gates.map((gate, index) => render("server-a", false).enqueue(request(String(index)), apply(() => gate.promise)));
  let flushed = false;
  const flush = coordinator.flush("server-a").then(() => { flushed = true; });
  assert.equal(render("server-a", false).state.requests.length, 3);
  assert.deepEqual(render("server-b", false).state.requests, []);
  await tick();
  assert.deepEqual(render("server-a", false).state.requests.map((entry) => entry.status), ["applying", "ready", "queued"]);
  gates[0].resolve(); await completions[0]; assert.equal(flushed, false);
  gates[1].resolve(); await completions[1]; assert.equal(flushed, false);
  gates[2].resolve(); await Promise.all([...completions, flush]); assert.equal(flushed, true);
  assert.equal(render("server-a", true).enqueue(request("777777"), async () => { assert.fail("Read-only view submitted work"); }), null);
});

test("later successes cannot clear a failed start barrier, but replacing that group can", async () => {
  const { coordinator, render } = hookHarness(), hook = render("server-a", false);
  const rejected = assert.rejects(hook.enqueue(request("123456"), async () => { throw new Error("Mod A failed"); }), /Mod A failed/);
  await Promise.all([rejected, hook.enqueue(request("654321"), apply())]);
  await assert.rejects(coordinator.flush("server-a"), /Mod A failed/);
  await hook.enqueue(request("123456"), apply());
  await coordinator.flush("server-a");
  assert.deepEqual(render("server-a", false).state.requests.map((entry) => entry.status), ["success", "success"]);
});

test("history limits cannot hide a failed request still blocking startup", async () => {
  const { coordinator, render } = hookHarness();
  await assert.rejects(render("server-a", false).enqueue(request("123456"), async () => { throw new Error("Visible retry required"); }));
  for (let i = 0; i < 40; i++) await render("server-a", false).enqueue(request(String(900000 + i)), apply());
  const afterDownloads = render("server-a", false).state.requests;
  assert.equal(afterDownloads.length, 33);
  assert.equal(afterDownloads[0].status, "error");
  assert.deepEqual(afterDownloads[0].ids, ["123456"]);
  for (let i = 0; i < 40; i++) await render(`other-server-${i}`, false).enqueue(request("654321"), apply());
  assert.equal(render("server-a", false).state.requests[0].error, "Visible retry required");
  await assert.rejects(coordinator.flush("server-a"), /Visible retry required/);
  await render("server-a", false).enqueue(request("123456"), apply());
  await coordinator.flush("server-a");
  assert.equal(render("server-a", false).state.requests.some((entry) => entry.status === "error"), false);
});

test("explicit batch retry preserves its full download-only intent at the commit tail with a fresh token", async () => {
  const { coordinator, render } = hookHarness(), ids = ["123456", "654321"], intent = { enable: false, purpose: "prepare" };
  const executions = [], tokens = [], retryGate = deferred(), activeGate = deferred();
  const execute = async (operation) => {
    tokens.push(operation.beginAttempt());
    await operation.apply(async () => {
      executions.push({ ids: [...ids], ...intent });
      if (executions.length === 1) throw new Error("Batch failed");
      await retryGate.promise;
    });
  };
  await assert.rejects(render("server-a", false).enqueue(request(...ids), execute), /Batch failed/);
  await render("server-a", false).enqueue(request("777777"), apply());
  await assert.rejects(coordinator.flush("server-a"), /Batch failed/);
  const failedId = render("server-a", false).state.requests[0].requestId;
  assert.equal(render("server-a", true).retry(failedId), null);
  assert.equal(render("server-b", false).retry(failedId), null);
  const active = render("server-a", false).enqueue(request("888888"), apply(() => activeGate.promise));
  const retry = render("server-a", false).retry(failedId);
  assert.ok(retry);
  assert.equal(render("server-a", false).retry(failedId), null);
  const queued = render("server-a", false).state.requests.at(-1);
  assert.deepEqual(queued.ids, ids);
  assert.equal(queued.status, "queued");
  assert.equal(render("server-a", false).retry(queued.requestId), null);
  await tick();
  assert.equal(executions.length, 1, "Retry may prepare concurrently but cannot apply ahead of prior work");
  assert.notEqual(tokens[0], tokens[1]);
  activeGate.resolve(); await active; await tick();
  assert.deepEqual(executions, [{ ids, ...intent }, { ids, ...intent }]);
  const running = render("server-a", false).state.requests.at(-1);
  assert.equal(running.status, "applying");
  assert.equal(render("server-a", false).retry(running.requestId), null);
  let started = false;
  const flush = coordinator.flush("server-a").then(() => { started = true; });
  await tick(); assert.equal(started, false);
  retryGate.resolve(); await Promise.all([retry, flush]); assert.equal(started, true);
  assert.equal(render("server-a", false).retry(running.requestId), null);
});

test("manual replacement retires the old failed executor and retains the new retry intent", async () => {
  const store = new WorkshopDownloadOperations();
  let oldAttempts = 0, newAttempts = 0;
  await assert.rejects(store.enqueue("server-a", request("123456"), async () => { oldAttempts++; throw new Error("Old intent failed"); }));
  const oldId = entries(store)[0].requestId;
  await assert.rejects(store.enqueue("server-a", request("123456"), async () => { newAttempts++; if (newAttempts === 1) throw new Error("New intent failed"); }));
  assert.equal(store.retry("server-a", oldId), null);
  await store.retry("server-a", entries(store)[0].requestId);
  assert.equal(oldAttempts, 1); assert.equal(newAttempts, 2); assert.equal(entries(store)[0].status, "success");
});

test("a failed batch can retry behind separately queued members and repair its group barrier", async () => {
  const { coordinator, render } = hookHarness(), firstGate = deferred(), secondGate = deferred();
  let batchAttempts = 0;
  await assert.rejects(render("server-a", false).enqueue(request("123456", "654321"), apply(async () => {
    batchAttempts++; if (batchAttempts === 1) throw new Error("Batch enablement failed");
  })), /Batch enablement failed/);
  const failedId = render("server-a", false).state.requests[0].requestId;
  const first = render("server-a", false).enqueue(request("123456"), apply(() => firstGate.promise));
  const second = render("server-a", false).enqueue(request("654321"), apply(() => secondGate.promise));
  const retry = render("server-a", false).retry(failedId);
  assert.ok(retry); assert.equal(render("server-a", false).retry(failedId), null);
  assert.equal(render("server-a", false).enqueue(request("123456", "654321"), async () => { assert.fail("Duplicate batch accepted"); }), null);
  assert.deepEqual(render("server-a", false).state.requests.map((entry) => entry.ids), [["123456"], ["654321"], ["123456", "654321"]]);
  let flushed = false;
  const flush = coordinator.flush("server-a").then(() => { flushed = true; });
  firstGate.resolve(); await first; await tick();
  assert.equal(batchAttempts, 1); assert.equal(flushed, false);
  secondGate.resolve(); await Promise.all([second, retry, flush]);
  assert.equal(batchAttempts, 2); assert.equal(flushed, true);
  assert.deepEqual(render("server-a", false).state.requests.map((entry) => entry.status), ["success", "success", "success"]);
});
