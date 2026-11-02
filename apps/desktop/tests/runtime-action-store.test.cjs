const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (loaded, filename) => loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { createRuntimeActionStore } = require("../src/runtime-action-store.ts");

test("navigation releases observers but preserves the same start and its capture", async () => {
  let captures = 0;
  let releases = 0;
  const store = createRuntimeActionStore(() => {
    captures++;
    return { ready: Promise.resolve(), dispose: () => { releases++; } };
  });
  let renders = 0;
  const unsubscribe = store.subscribe(() => { renders++; });
  const boundary = { instanceId: "a", startedAtUnixMs: 100, previousLogPaths: new Set(["old.log"]) };
  const start = store.begin("a", "start", { startup: { moduleId: "sevendaystodie", boundary } });
  assert.match(start.startupAttemptId, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  await start.ready;
  unsubscribe();
  assert.equal(store.getSnapshot().a, "starting");
  assert.deepEqual(store.startupBoundary("a"), { ...boundary, startupAttemptId: start.startupAttemptId });
  assert.equal(store.begin("a", "start", { startup: { moduleId: "sevendaystodie", boundary } }), null);
  assert.equal(captures, 1);
  assert.equal(releases, 0);
  const stopOther = store.begin("b", "stop");
  assert.equal(stopOther.startupAttemptId, undefined, "Ordinary stop must not inherit another instance's startup identity");
  assert.deepEqual(store.getSnapshot(), { a: "starting", b: "stopping" });
  start.finish();
  assert.deepEqual(store.getSnapshot(), { b: "stopping" });
  assert.equal(store.startupBoundary("a"), undefined);
  assert.equal(releases, 1);
  assert.equal(renders, 1);
  const replacement = store.begin("a", "start", { startup: { moduleId: "sevendaystodie", boundary } });
  assert.notEqual(replacement.startupAttemptId, start.startupAttemptId, "A new startup must own a new cancellation identity");
  start.finish();
  assert.equal(store.getSnapshot().a, "starting", "A stale completion cannot release a new operation");
  assert.equal(releases, 1);
  replacement.finish();
  stopOther.finish();
  assert.deepEqual(store.getSnapshot(), {});
  assert.equal(releases, 2);
});

for (const submitted of [false, true]) test(`stop takes ownership of ${submitted ? "submitted" : "local"} startup without a late completion clearing it`, () => {
  let released = 0;
  const store = createRuntimeActionStore(() => ({ ready: Promise.resolve(), dispose: () => { released++; } }));
  const boundary = { instanceId: "a", startedAtUnixMs: 100, previousLogPaths: new Set() };
  const start = store.begin("a", "start", { startup: { moduleId: "arksurvivalascended", boundary } });
  if (submitted) assert.equal(start.markStartDispatched(), true);
  const stop = store.begin("a", "stop");
  assert.equal(stop.cancelsStartup, true);
  assert.equal(stop.localCancellation, !submitted);
  assert.equal(stop.startupAttemptId, start.startupAttemptId);
  assert.equal(start.isCurrent(), false);
  assert.equal(start.markStartDispatched(), false, "Cancellation prevents any later IPC submission");
  assert.equal(store.begin("a", "stop"), null);
  assert.equal(store.begin("a", "start"), null);
  assert.deepEqual(store.startupBoundary("a"), { ...boundary, startupAttemptId: start.startupAttemptId });
  assert.equal(released, 0, "The original operation retains its capture until it settles");
  start.finish();
  assert.equal(released, 1);
  assert.equal(stop.isCurrent(), true);
  assert.equal(store.getSnapshot().a, "cancelling");
  start.finish();
  assert.equal(released, 1);
  stop.finish();
  assert.deepEqual(store.getSnapshot(), {});
});

test("a recovered backend startup needs a stop IPC even without a local start owner", () => {
  const store = createRuntimeActionStore();
  const stop = store.begin("a", "stop", { startupPending: true });
  assert.equal(stop.cancelsStartup, true);
  assert.equal(stop.localCancellation, false);
  assert.equal(stop.startupAttemptId, undefined, "A recovered startup remains an explicit action on the current instance");
  assert.equal(store.getSnapshot().a, "cancelling");
  stop.finish();
});

test("ordinary stop after a completed startup does not reuse its attempt identity", () => {
  const store = createRuntimeActionStore();
  const start = store.begin("a", "start");
  start.markStartDispatched();
  start.finish();
  const stop = store.begin("a", "stop");
  assert.equal(stop.startupAttemptId, undefined);
  assert.equal(stop.cancelsStartup, false);
  stop.finish();
});
