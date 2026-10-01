const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const flush = async () => { for (let index = 0; index < 10; index++) await Promise.resolve(); };
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function harness({ enabled = true } = {}) {
  const requests = [], snapshots = [], errors = [];
  const window = new EventTarget();
  const document = new EventTarget();
  document.visibilityState = "visible";
  let instances = [{ id: "a" }, { id: "b" }];
  let selectedId = "a";
  let cleared = 0;
  let optionsRef, cleanup;
  const filename = path.join(__dirname, "../src/hooks/useInstanceInventoryRefresh.ts");
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports, AbortController, window, document,
    require: (id) => {
      if (id === "react") return {
        useRef: (initial) => optionsRef ??= { current: initial },
        useEffect: (effect) => { cleanup = effect(); }
      };
      if (id === "../app-state") return {
        resolveSelectedId: (id, list) => list.some((entry) => entry.id === id) ? id : list[0]?.id ?? null
      };
      if (id === "../instance-panel-loader") return { instancePanelReader: {
        readInstances: (signal) => { const read = { ...deferred(), signal }; requests.push(read); return read.promise; }
      } };
      throw new Error(`Unexpected import ${id}`);
    }
  }, { filename });
  exports.useInstanceInventoryRefresh({ enabled,
    getCurrentInstances: () => instances,
    getCurrentInstanceId: () => selectedId,
    onInstancesSynced: (snapshot) => { snapshots.push(snapshot); instances = snapshot.instances; selectedId = snapshot.selectedInstanceId; },
    onSelectionCleared: () => { cleared++; },
    onError: (error) => errors.push(error)
  });
  return { requests, snapshots, errors, window, document,
    get instances() { return instances; }, get selectedId() { return selectedId; },
    get cleared() { return cleared; },
    select: (id) => { selectedId = id; }, replace: (next) => { instances = next; },
    focus: () => window.dispatchEvent(new Event("focus")),
    visibility: (state) => { document.visibilityState = state; document.dispatchEvent(new Event("visibilitychange")); },
    unmount: () => cleanup?.()
  };
}

test("foreground reconciliation removes an externally deleted selection without needing detail polling", async () => {
  const state = harness();
  state.requests[0].resolve(state.instances);
  await flush();
  state.focus();
  assert.equal(state.requests.length, 2);
  state.requests[1].resolve([{ id: "b" }]);
  await flush();
  assert.equal(state.selectedId, "b");
  assert.deepEqual(state.instances, [{ id: "b" }]);
  state.visibility("hidden");
  state.focus();
  assert.equal(state.requests.length, 2, "a hidden window does not trigger native reads");
  state.visibility("visible");
  state.requests[2].resolve([]);
  await flush();
  assert.equal(state.selectedId, null, "removing the last shell returns to the empty view");
  assert.deepEqual(state.instances, []);
  assert.equal(state.cleared, 1, "clearing the last record also clears stale failure and pause state");
  state.unmount();
});

test("focus and visibility events share one read and preserve newer selection intent", async () => {
  const state = harness();
  state.focus();
  state.visibility("visible");
  assert.equal(state.requests.length, 1);
  state.select("b");
  state.requests[0].resolve([{ id: "a" }, { id: "b" }]);
  await flush();
  assert.equal(state.selectedId, "b", "the response cannot return selection to its request-time value");
  state.unmount();
});

test("inventory replacement during a read discards its stale result", async () => {
  const state = harness();
  const newer = [{ id: "b" }, { id: "created" }];
  state.replace(newer);
  state.select("created");
  state.requests[0].resolve([{ id: "a" }, { id: "b" }]);
  await flush();
  assert.equal(state.snapshots.length, 0);
  assert.equal(state.instances, newer);
  assert.equal(state.selectedId, "created");
  state.unmount();
});

test("inventory failure preserves records and a later foreground event can recover", async () => {
  const state = harness();
  const original = state.instances;
  const error = new Error("volume unavailable");
  state.requests[0].reject(error);
  await flush();
  assert.equal(state.instances, original);
  assert.equal(state.errors[0], error);
  state.focus();
  state.requests[1].resolve([{ id: "b" }]);
  await flush();
  assert.equal(state.selectedId, "b");
  state.unmount();
});

test("unmount aborts observation, detaches events and ignores a late native result", async () => {
  const state = harness();
  state.unmount();
  assert.equal(state.requests[0].signal.aborted, true);
  state.focus();
  state.visibility("visible");
  assert.equal(state.requests.length, 1);
  state.requests[0].resolve([]);
  await flush();
  assert.equal(state.snapshots.length, 0);
  assert.equal(state.selectedId, "a");
});

test("storage not ready never starts inventory reconciliation", () => {
  const state = harness({ enabled: false });
  state.focus();
  state.visibility("visible");
  assert.equal(state.requests.length, 0);
  state.unmount();
});
