const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { createArkToolsInstallationStore } = require("../src/ark-tools-installation-store.ts");
const { queuedArkToolsInstallation } = require("../src/ark-tools-installation.ts");

const ready = { installed: true, updateAvailable: false, connected: false, canUninstall: true, issue: null };
const removed = { installed: false, updateAvailable: false, connected: false, canUninstall: false, issue: null, frameworkRetained: false };
const flush = () => new Promise(resolve => setImmediate(resolve));
function deferred() { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
function progress(instanceId, operationId, change = {}) { return { ...queuedArkToolsInstallation(instanceId, operationId), ...change }; }
function prepared(instanceId, operationId, elapsedSeconds = 7) {
  return { ...ready, progress: progress(instanceId, operationId, { active: false, phase: "ready", elapsedSeconds }) };
}
function setup(statusReader = async () => ready) {
  const reads = [], preparations = [], removals = [], statusReads = [], timers = new Map();
  let timerId = 0, operationId = 0, now = 100;
  const store = createArkToolsInstallationStore({
    prepare(instanceId, operationId) { const request = deferred(); preparations.push({ instanceId, operationId, ...request }); return request.promise; },
    remove(instanceId) { const request = deferred(); removals.push({ instanceId, ...request }); return request.promise; },
    readProgress(instanceId, operationId) { const request = deferred(); reads.push({ instanceId, operationId, ...request }); return request.promise; },
    readStatus(instanceId) { statusReads.push(instanceId); return statusReader(instanceId); },
    newOperationId: () => `install-${++operationId}`, now: () => now,
    schedule(callback, delay) { assert.equal(delay, 1000); const id = ++timerId; timers.set(id, callback); return id; },
    cancel: id => timers.delete(id)
  });
  return { store, reads, preparations, removals, statusReads, timers,
    at(value) { now = value; },
    tick() { const [id, callback] = timers.entries().next().value; timers.delete(id); callback(); },
    entry(instanceId) { return store.getSnapshot().find(entry => entry.instanceId === instanceId); }
  };
}
async function start(run, instanceId = "ark-a", instanceName = "ARK A") {
  run.store.install(instanceId, instanceName);
  const recovery = run.reads.at(-1);
  assert.equal(recovery.instanceId, instanceId);
  assert.equal(recovery.operationId, undefined);
  recovery.resolve(null);
  await flush();
  return run.preparations.at(-1);
}

test("construction is inert and recover/install before provider connect share one recovery", async () => {
  const run = setup();
  assert.deepEqual(run.store.getSnapshot(), []);
  run.store.recover("ark-a", "Old name");
  run.store.install("ark-a", "ARK A");
  assert.equal(run.reads.length, 0);
  assert.equal(run.preparations.length, 0);
  run.store.connect();
  run.store.connect();
  run.store.recover("ark-a", "ARK A");
  run.store.install("ark-a", "ARK A");
  assert.equal(run.reads.length, 1);
  run.reads[0].resolve(progress("ark-a", "existing", { phase: "downloading", elapsedSeconds: 90 }));
  await flush();
  assert.equal(run.preparations.length, 0, "an already active operation must be observed rather than repeated");
  assert.equal(run.reads[1].operationId, "existing");
  assert.equal(run.entry("ark-a").instanceName, "ARK A");
  assert.equal(run.entry("ark-a").observing, true);
  run.store.disconnect();
});

test("subscriber navigation does not stop a single per-instance installer", async () => {
  const run = setup();
  let notifications = 0;
  const unsubscribe = run.store.subscribe(() => notifications++);
  run.store.connect();
  const install = await start(run);
  run.store.install("ark-a", "ARK A");
  run.store.recover("ark-a", "ARK A");
  assert.equal(run.preparations.length, 1);
  const notified = notifications;
  unsubscribe();
  run.reads.at(-1).resolve(progress("ark-a", install.operationId, { phase: "downloading", downloadedBytes: 512, totalBytes: 1024 }));
  await flush();
  assert.equal(run.entry("ark-a").progress.downloadedBytes, 512);
  assert.equal(notifications, notified);
  assert.equal(run.timers.size, 1, "page unsubscription must leave the application-owned observer running");
  const snapshot = run.store.getSnapshot();
  assert.equal(run.store.getSnapshot(), snapshot, "external-store snapshots are stable between changes");
  install.resolve(prepared("ark-a", install.operationId));
  await flush();
  assert.equal(run.entry("ark-a").observing, false);
  assert.equal(run.entry("ark-a").status.installed, true);
  assert.equal(run.timers.size, 0);
  run.store.disconnect();
});

test("separate instances keep independent progress, names and final outcomes", async () => {
  const run = setup();
  run.store.connect();
  const first = await start(run, "ark-a", "Island");
  const firstRead = run.reads.at(-1);
  const second = await start(run, "ark-b", "Scorched Earth");
  const secondRead = run.reads.at(-1);
  firstRead.resolve(progress("ark-a", first.operationId, { phase: "downloading", downloadedBytes: 100, totalBytes: 200 }));
  secondRead.resolve(progress("ark-b", second.operationId, { phase: "verifying", elapsedSeconds: 12 }));
  await flush();
  assert.equal(run.entry("ark-a").instanceName, "Island");
  assert.equal(run.entry("ark-a").progress.downloadedBytes, 100);
  assert.equal(run.entry("ark-b").progress.phase, "verifying");
  first.resolve(prepared("ark-a", first.operationId));
  await flush();
  assert.equal(run.entry("ark-b").observing, true);
  second.resolve(prepared("ark-b", second.operationId));
  await flush();
  assert.equal(run.preparations.length, 2);
  run.store.disconnect();
});

test("StrictMode disconnect/reconnect invalidates old recovery and retains pending intent", async () => {
  const run = setup();
  run.store.recover("ark-a", "ARK A");
  run.store.connect();
  const old = run.reads[0];
  run.store.disconnect();
  run.store.recover("ark-a", "ARK A");
  run.store.connect();
  assert.equal(run.reads.length, 2);
  old.resolve(progress("ark-a", "stale-operation"));
  await flush();
  assert.equal(run.entry("ark-a").progress, null);
  assert.equal(run.entry("ark-a").recovering, true);
  run.reads[1].resolve(null);
  await flush();
  assert.equal(run.entry("ark-a").recovering, false);
  assert.equal(run.entry("ark-a").progress, null);
  assert.equal(run.preparations.length, 0);
  run.store.disconnect();
});

test("reconnect resumes the same operation and late previous-session receipts cannot replace it", async () => {
  const run = setup();
  run.store.connect();
  const install = await start(run);
  const oldRead = run.reads.at(-1);
  run.at(5000);
  run.store.disconnect();
  assert.equal(run.timers.size, 0);
  assert.equal(run.entry("ark-a").observing, false);
  run.store.connect();
  const currentRead = run.reads.at(-1);
  assert.equal(currentRead.operationId, install.operationId);
  assert.equal(run.preparations.length, 1);
  oldRead.resolve(progress("ark-a", install.operationId, { phase: "failed", active: false, error: "stale failure" }));
  install.resolve(prepared("ark-a", install.operationId));
  await flush();
  assert.equal(run.entry("ark-a").observing, true);
  assert.equal(run.entry("ark-a").status, null);
  currentRead.resolve(progress("ark-a", install.operationId, { phase: "downloading", elapsedSeconds: 5 }));
  await flush();
  assert.equal(run.entry("ark-a").progress.phase, "downloading");
  run.tick();
  run.reads.at(-1).resolve(prepared("ark-a", install.operationId).progress);
  await flush();
  assert.equal(run.entry("ark-a").progress.phase, "ready");
  assert.equal(run.entry("ark-a").observing, false);
  assert.equal(run.preparations.length, 1);
  run.store.disconnect();
});

test("late receipts from a completed operation cannot overwrite a new operation", async () => {
  const run = setup();
  run.store.connect();
  const first = await start(run);
  run.reads.at(-1).resolve(progress("ark-a", first.operationId, { phase: "failed", active: false, error: "download failed" }));
  await flush();
  const second = await start(run);
  first.resolve(prepared("ark-a", first.operationId));
  await flush();
  assert.equal(run.entry("ark-a").progress.operationId, second.operationId);
  assert.equal(run.entry("ark-a").observing, true);
  assert.equal(run.entry("ark-a").status, null);
  second.resolve(prepared("ark-a", second.operationId));
  await flush();
  run.store.disconnect();
});

test("failed recovery never dispatches installation until a fresh read confirms no active operation", async () => {
  const run = setup();
  run.store.connect();
  run.store.install("ark-a", "ARK A");
  run.reads[0].reject(new Error("service unavailable"));
  await flush();
  assert.equal(run.preparations.length, 0);
  assert.equal(run.entry("ark-a").recovering, false);
  assert.equal(run.entry("ark-a").issue.message, "service unavailable");
  run.store.install("ark-a", "ARK A");
  run.store.install("ark-a", "ARK A");
  assert.equal(run.reads.length, 2);
  run.reads[1].resolve(progress("ark-a", "native-active", { phase: "connecting" }));
  await flush();
  assert.equal(run.preparations.length, 0);
  run.store.install("ark-a", "ARK A");
  assert.equal(run.preparations.length, 0);
  run.store.disconnect();
});

test("unknown install receipts stay owned and cannot be retried by another page", async () => {
  const run = setup();
  run.store.connect();
  const install = await start(run);
  install.reject("Runtime response timed out; the operation may still be running");
  await flush();
  run.reads.at(-1).reject(new Error("progress unavailable"));
  await flush();
  run.store.recover("ark-a", "Other page name");
  run.store.install("ark-a", "Other page name");
  assert.equal(run.entry("ark-a").observing, true);
  assert.equal(run.entry("ark-a").issue.kind, "receipt");
  assert.equal(run.preparations.length, 1);
  run.store.disconnect();
});

test("dismissal affects only the matching completed operation", async () => {
  const run = setup();
  run.store.connect();
  const first = await start(run);
  run.store.dismiss(first.operationId);
  assert.equal(run.entry("ark-a").dismissed, false);
  first.resolve(prepared("ark-a", first.operationId));
  await flush();
  run.store.dismiss(first.operationId);
  assert.equal(run.entry("ark-a").dismissed, true);
  const second = await start(run);
  assert.equal(run.entry("ark-a").dismissed, false);
  run.store.dismiss(first.operationId);
  assert.equal(run.entry("ark-a").dismissed, false);
  second.resolve(prepared("ark-a", second.operationId));
  await flush();
  run.store.disconnect();
});

test("completed history is bounded without evicting active installations", async () => {
  const run = setup();
  run.store.connect();
  await start(run, "active", "Still downloading");
  for (let index = 0; index < 40; index++) {
    const id = `done-${index}`;
    run.store.recover(id, id);
    run.reads.at(-1).resolve(progress(id, `operation-${index}`, { phase: "failed", active: false, error: "known failure" }));
    await flush();
  }
  assert.equal(run.store.getSnapshot().length, 33);
  assert.equal(run.entry("active").observing, true);
  assert.equal(run.entry("done-0"), undefined);
  assert.equal(run.entry("done-39").progress.phase, "failed");
  run.store.disconnect();
});

test("progress owns the timing anchor; issue, status, subscription and session changes preserve it", async () => {
  const status = deferred();
  const run = setup(() => status.promise);
  run.store.connect();
  run.store.recover("ark-a", "ARK A");
  assert.equal(run.entry("ark-a").observedAt, 100);
  run.at(1000);
  run.reads[0].resolve(progress("ark-a", "native", { phase: "downloading", elapsedSeconds: 90 }));
  await flush();
  assert.equal(run.entry("ark-a").observedAt, 1000);
  run.at(4000);
  run.reads.at(-1).reject(new Error("progress read failed"));
  await flush();
  assert.equal(run.entry("ark-a").observedAt, 1000);
  run.store.recover("ark-a", "Renamed server");
  const unsubscribe = run.store.subscribe(() => {});
  unsubscribe();
  run.store.disconnect();
  run.at(6000);
  run.store.connect();
  assert.equal(run.entry("ark-a").observedAt, 1000, "replaying the same observer initial snapshot must not reset elapsed time");
  run.at(7000);
  run.reads.at(-1).resolve(progress("ark-a", "native", { active: false, phase: "ready", elapsedSeconds: 96 }));
  await flush();
  assert.equal(run.entry("ark-a").observedAt, 7000);
  run.at(9000);
  status.resolve(ready);
  await flush();
  const terminalAnchor = run.entry("ark-a").observedAt;
  run.at(10000);
  run.store.dismiss("native");
  run.store.recover("ark-a", "Final name");
  assert.equal(run.entry("ark-a").observedAt, terminalAnchor);
  assert.equal(run.entry("ark-a").progress.elapsedSeconds, 96);
  run.store.disconnect();
});

test("removal rejects recovery, installation and duplicate removal, and blocks a new install across navigation", async () => {
  const run = setup();
  run.store.connect();
  run.store.recover("ark-a", "ARK A");
  assert.equal(await run.store.remove("ark-a", "ARK A"), null);
  assert.equal(run.removals.length, 0);
  run.reads[0].resolve(null);
  await flush();
  const installation = await start(run);
  assert.equal(await run.store.remove("ark-a", "ARK A"), null);
  assert.equal(run.removals.length, 0);
  installation.resolve(prepared("ark-a", installation.operationId));
  await flush();
  const removal = run.store.remove("ark-a", "ARK A");
  assert.equal(run.entry("ark-a").removing, true);
  assert.equal(await run.store.remove("ark-a", "ARK A"), null);
  run.store.install("ark-a", "Another page");
  run.store.recover("ark-a", "Another page");
  const unsubscribe = run.store.subscribe(() => {});
  unsubscribe();
  run.store.disconnect();
  run.store.connect();
  assert.equal(run.entry("ark-a").removing, true);
  assert.equal(run.removals.length, 1);
  assert.equal(run.preparations.length, 1);
  run.removals[0].resolve(removed);
  assert.deepEqual(await removal, removed);
  assert.equal(run.entry("ark-a").removing, false);
  assert.equal(run.entry("ark-a").progress, null);
  assert.deepEqual(run.entry("ark-a").status, removed);
  run.store.recover("ark-a", "Returning page");
  assert.equal(run.entry("ark-a").status.installed, false);
  run.store.disconnect();
});

test("failed removal stays inspectable and can be retried without altering installed state", async () => {
  const run = setup();
  run.store.connect();
  const installation = await start(run);
  installation.resolve(prepared("ark-a", installation.operationId));
  await flush();
  const first = run.store.remove("ark-a", "ARK A");
  run.removals[0].reject(new Error("extension file is in use"));
  assert.equal(await first, null);
  assert.equal(run.entry("ark-a").removing, false);
  assert.deepEqual(run.entry("ark-a").status, ready);
  assert.equal(run.entry("ark-a").removalError, "extension file is in use");
  const retry = run.store.remove("ark-a", "ARK A");
  assert.equal(run.entry("ark-a").removalError, null);
  run.removals[1].resolve({ ...removed, frameworkRetained: true });
  await retry;
  assert.equal(run.entry("ark-a").removalResult.frameworkRetained, true);
  const next = await start(run);
  assert.equal(run.entry("ark-a").removalResult, null);
  next.resolve(prepared("ark-a", next.operationId));
  await flush();
  run.store.disconnect();
});

test("a late installation receipt cannot resurrect an extension after removal", async () => {
  const run = setup();
  run.store.connect();
  const installation = await start(run);
  run.reads.at(-1).resolve(prepared("ark-a", installation.operationId).progress);
  await flush();
  assert.equal(run.entry("ark-a").observing, false);
  const removal = run.store.remove("ark-a", "ARK A");
  run.removals[0].resolve(removed);
  await removal;
  installation.resolve(prepared("ark-a", installation.operationId));
  await flush();
  assert.equal(run.entry("ark-a").status.installed, false);
  assert.equal(run.entry("ark-a").status.canUninstall, false);
  assert.equal(run.entry("ark-a").progress, null);
  run.store.disconnect();
});
