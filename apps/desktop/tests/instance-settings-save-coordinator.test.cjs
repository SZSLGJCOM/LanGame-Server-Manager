const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const { InstanceSettingsSaveQueue } = require("../src/views/settings/instance-settings-save-queue.ts");
const coordinatorPath = path.resolve(__dirname, "../src/views/settings/instance-settings-save-coordinator.ts");
function coordinator() {
  assert.ok(fs.existsSync(coordinatorPath), "the app must own a save coordinator across editor unmounts");
  const { InstanceSettingsSaveCoordinator } = require(coordinatorPath);
  return new InstanceSettingsSaveCoordinator();
}
function deferred() {
  let resolve, reject;
  const promise = new Promise((a, b) => { resolve = a; reject = b; });
  return { promise, resolve, reject };
}
const settle = () => new Promise((resolve) => setImmediate(resolve));

test("Mod operations join the start barrier before their first asynchronous read", async () => {
  const owner = coordinator();
  assert.equal(typeof owner.runOperation, "function");
  const read = deferred();
  let saved = false;
  const operation = owner.runOperation("server-a", async () => { await read.promise; saved = true; }, "mods");
  let started = false;
  const start = owner.flush("server-a").then(() => { started = true; });
  await settle();
  assert.equal(started, false);
  read.resolve();
  await Promise.all([operation, start]);
  assert.equal(saved, true);
});

test("Mod failures survive panel disposal and a successful retry clears only its own failure", async () => {
  const owner = coordinator();
  assert.equal(typeof owner.runOperation, "function");
  await assert.rejects(owner.runOperation("server-a", async () => { throw new Error("disk failed"); }, "mods"), /disk failed/);
  await assert.rejects(owner.flush("server-a"), /disk failed/);
  await owner.runOperation("server-a", async () => undefined, "mods");
  await owner.flush("server-a");
  owner.register("server-a", async () => { throw new Error("invalid settings"); });
  await owner.runOperation("server-a", async () => undefined, "mods");
  await assert.rejects(owner.flush("server-a"), /invalid settings/);
});

test("a new Mod operation cannot erase a detached editor failure", async () => {
  const owner = coordinator();
  assert.equal(typeof owner.runOperation, "function");
  const registration = owner.register("server-a", async () => undefined);
  registration.detach(Promise.reject(new Error("invalid draft")));
  await settle();
  await owner.runOperation("server-a", async () => undefined, "mods");
  await assert.rejects(owner.flush("server-a"), /invalid draft/);
});

test("reopening editors or retrying Mods does not forget an older pending operation", async () => {
  const owner = coordinator();
  assert.equal(typeof owner.runOperation, "function");
  const first = deferred();
  const pending = owner.runOperation("server-a", () => first.promise, "mods");
  owner.register("server-a", async () => undefined);
  await owner.runOperation("server-a", async () => undefined, "mods");
  let started = false;
  const start = owner.flush("server-a").then(() => { started = true; });
  await settle();
  assert.equal(started, false);
  first.resolve();
  await Promise.all([pending, start]);
});
const request = (id, value) => ({
  signature: value,
  input: { id, bind_ip: "0.0.0.0", auto_backup_on_stop: false, backup_retention_count: 3, settings_json: value, ports: [] }
});
function editor(owner, execute, id = "server-a") {
  const queue = new InstanceSettingsSaveQueue({ instanceId: id, settingsBaseline: "base", savedSignature: "base", execute });
  let latest = request(id, "latest");
  const registration = owner.register(id, () => queue.flushLatest(latest));
  return {
    queue,
    invalidate() { latest = null; queue.markDirty("invalid"); },
    leave() {
      queue.flushAndDispose(latest);
      registration.detach(latest ? queue.whenIdle() : Promise.reject(new Error("invalid draft")));
    }
  };
}

test("start waits for the closing editor's in-flight save", async () => {
  const owner = coordinator();
  const save = deferred();
  const active = editor(owner, () => save.promise);
  active.leave();
  let started = false;
  const start = owner.flush("server-a").then(() => { started = true; });
  await settle();
  assert.equal(started, false);
  save.resolve("latest");
  await start;
  await owner.flush("server-a");
});

for (const reason of ["disk failed", "precondition failed"]) {
  test(`a closing editor retains ${reason} as a startup blocker`, async () => {
    const owner = coordinator();
    const save = deferred();
    editor(owner, () => save.promise).leave();
    const failedStart = assert.rejects(owner.flush("server-a"), new RegExp(reason));
    save.reject(new Error(reason));
    await failedStart;
    await assert.rejects(owner.flush("server-a"), new RegExp(reason));
  });
}

test("invalid drafts block start both before and after leaving settings", async () => {
  const owner = coordinator();
  const active = editor(owner, () => assert.fail("invalid settings must not be saved"));
  active.invalidate();
  await assert.rejects(owner.flush("server-a"), /invalid/i);
  active.leave();
  await assert.rejects(owner.flush("server-a"), /invalid/i);
});

test("a blocked instance does not prevent another instance from starting", async () => {
  const owner = coordinator();
  const active = editor(owner, () => assert.fail("invalid settings must not be saved"));
  active.invalidate();
  active.leave();
  let saved = false;
  editor(owner, async (nextInput) => { saved = true; return nextInput.settings_json; }, "server-b");
  await owner.flush("server-b");
  assert.equal(saved, true);
  await owner.flush("unopened-server");
  await assert.rejects(owner.flush("server-a"), /invalid/i);
});

test("separate app coordinators never share drafts", async () => {
  const first = coordinator();
  const active = editor(first, () => assert.fail("invalid settings must not be saved"));
  active.invalidate();
  const second = coordinator();
  await second.flush("server-a");
  await assert.rejects(first.flush("server-a"), /invalid/i);
});

test("a flush started before unmount observes the detached completion", async () => {
  const owner = coordinator();
  const oldFlush = deferred();
  const closing = deferred();
  const registration = owner.register("server-a", () => oldFlush.promise);
  let started = false;
  const barrier = owner.flush("server-a").then(() => { started = true; });
  registration.detach(closing.promise);
  oldFlush.resolve();
  await settle();
  assert.equal(started, false);
  closing.resolve();
  await barrier;
});

test("returning to settings cannot forget an earlier editor's still active write", async () => {
  const owner = coordinator();
  const save = deferred();
  editor(owner, () => save.promise).leave();
  owner.register("server-a", async () => undefined);
  let started = false;
  const barrier = owner.flush("server-a").then(() => { started = true; });
  await settle();
  assert.equal(started, false);
  save.resolve("latest");
  await barrier;
});

test("a running-only game API does not become a prerequisite for starting its server", async () => {
  const owner = coordinator();
  let nativeCalls = 0, ordinaryCalls = 0;
  owner.register("server-a", async () => { ordinaryCalls += 1; });
  owner.register("server-a", async () => { nativeCalls += 1; throw new Error("API requires running server"); }, {}, { beforeStop: true });
  await owner.flush("server-a");
  assert.equal(ordinaryCalls, 1);
  assert.equal(nativeCalls, 0);
  await assert.rejects(owner.flushBeforeStop("server-a"), /API requires running server/);
  await owner.flush("server-a");
  assert.equal(nativeCalls, 1, "starting never resends a failed live mutation");
});

test("shutdown waits for live API persistence without flushing an unrelated invalid startup draft", async () => {
  const owner = coordinator();
  const save = deferred();
  owner.register("server-a", async () => { throw new Error("invalid startup settings"); });
  owner.register("server-a", () => save.promise, {}, { beforeStop: true });
  let stopped = false;
  const stopping = owner.flushBeforeStop("server-a").then(() => { stopped = true; });
  await settle();
  assert.equal(stopped, false);
  save.resolve();
  await stopping;
  assert.equal(stopped, true);
  await assert.rejects(owner.flush("server-a"), /invalid startup settings/);
});

test("reopening ordinary settings does not acknowledge a detached live API failure", async () => {
  const owner = coordinator();
  owner.register("server-a", async () => undefined, {}, { beforeStop: true })
    .detach(Promise.reject(new Error("native result unknown")));
  await settle();
  owner.register("server-a", async () => undefined);
  await owner.flush("server-a");
  await assert.rejects(owner.flushBeforeStop("server-a"), /native result unknown/);
  owner.register("server-a", async () => undefined, {}, { beforeStop: true });
  await owner.flushBeforeStop("server-a");
});

test("save phases and native failures remain isolated between instances", async () => {
  const owner = coordinator();
  owner.register("server-a", async () => { throw new Error("native unavailable"); }, {}, { beforeStop: true });
  let persisted = false;
  owner.register("server-b", async () => { persisted = true; }, {}, { beforeStop: true });
  await owner.flushBeforeStop("server-b");
  assert.equal(persisted, true);
  await owner.flush("server-a");
  await assert.rejects(owner.flushBeforeStop("server-a"), /native unavailable/);
});
