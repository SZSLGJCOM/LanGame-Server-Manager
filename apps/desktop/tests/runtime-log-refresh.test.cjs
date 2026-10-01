const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
const file = path.join(__dirname, "../src/runtime-log-refresh.ts");
const exported = { exports: {} };
vm.runInNewContext(transpileTypeScript(fs.readFileSync(file, "utf8"), file),
  { module: exported, exports: exported.exports }, { filename: file });
const { createRuntimeLogRefresh } = exported.exports;
const turn = () => new Promise(resolve => setImmediate(resolve));
function deferred() { let resolve, reject; const promise = new Promise((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; }

test("a burst has one reader and one follow-up, preserving native repeats and the shutdown tail", async () => {
  const pending = [], published = [], errors = [], loading = [];
  const queue = createRuntimeLogRefresh({
    read: () => { const read = deferred(); pending.push(read); return read.promise; },
    publish: value => published.push(value), failed: error => errors.push(error), loading: value => loading.push(value)
  });
  queue.request();
  for (let i = 0; i < 1000; i++) queue.request();
  assert.equal(pending.length, 1);
  pending[0].resolve(["native pair", "native pair"]);
  await turn();
  assert.equal(pending.length, 2);
  pending[1].resolve(["native pair", "native pair", "final shutdown tail"]);
  await turn();
  assert.equal(pending.length, 2);
  assert.deepEqual(published, [["native pair", "native pair"], ["native pair", "native pair", "final shutdown tail"]]);
  assert.deepEqual(errors, []);
  assert.deepEqual(loading, [true, false]);
  queue.dispose();
});

test("disposing a replaced source blocks late success, error and queued reads", async () => {
  for (const reject of [false, true]) {
    const first = deferred(), visible = [], failures = [];
    let reads = 0;
    const old = createRuntimeLogRefresh({ read: () => { reads++; return first.promise; },
      publish: value => visible.push(value), failed: error => failures.push(error), loading: () => {} });
    old.request(); old.request(); old.dispose();
    const current = createRuntimeLogRefresh({ read: async () => "new source", publish: value => visible.push(value),
      failed: error => failures.push(error), loading: () => {} });
    current.request();
    await turn();
    if (reject) first.reject(new Error("old source failed")); else first.resolve("old source");
    await turn();
    old.request();
    assert.equal(reads, 1);
    assert.deepEqual(visible, ["new source"]);
    assert.deepEqual(failures, []);
    current.dispose();
  }
});

test("a failed read is observable and does not disable a later bounded retry", async () => {
  const failure = new Error("file unavailable"), errors = [], values = [];
  let fails = true;
  const queue = createRuntimeLogRefresh({ read: async () => { if (fails) throw failure; return "recovered"; },
    publish: value => values.push(value), failed: error => errors.push(error), loading: () => {} });
  queue.request(); await turn();
  assert.deepEqual(errors, [failure]);
  fails = false;
  queue.request(); await turn();
  assert.deepEqual(values, ["recovered"]);
  queue.dispose();
});
