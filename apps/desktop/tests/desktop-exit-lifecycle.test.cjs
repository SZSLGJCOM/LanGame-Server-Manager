const assert = require("node:assert/strict");
const test = require("node:test");
const { loadDesktopExitModule } = require("./helpers/desktop-exit-fixture.cjs");
const { DesktopExitLifecycle, FINAL_EXIT_ERROR } = loadDesktopExitModule({ isTauri: () => false });

function fixture(readStatus) {
  let onExit;
  let reads = 0;
  let unlistened = 0;
  const lifecycle = new DesktopExitLifecycle({
    enabled: true,
    readStatus: () => { reads++; return readStatus(); },
    listen: async (callback) => { onExit = callback; return () => { unlistened++; }; }
  });
  return { lifecycle, emit: (payload) => onExit(payload), get reads() { return reads; }, get unlistened() { return unlistened; } };
}

test("final exit wins an older status read and repeated events cannot replace its receipt", async () => {
  let completeRead;
  const state = fixture(() => new Promise((resolve) => { completeRead = resolve; }));
  const dispose = await state.lifecycle.listen();
  let notifications = 0;
  const unsubscribe = state.lifecycle.subscribe(() => { notifications++; });
  const pending = state.lifecycle.refresh();
  state.emit({ requested: true });
  const receipt = state.lifecycle.getSnapshot();
  completeRead({ requested: false });
  await pending;
  state.emit({ requested: true });
  await state.lifecycle.refresh();
  assert.equal(state.lifecycle.getSnapshot(), receipt);
  assert.equal(notifications, 1);
  assert.equal(state.reads, 1);
  unsubscribe(); dispose();
  assert.equal(state.unlistened, 1);
});

test("mounting after a missed event reads the native final-exit receipt", async () => {
  const state = fixture(async () => ({ requested: true }));
  await state.lifecycle.refresh();
  assert.equal(state.lifecycle.getSnapshot().requested, true);
});

test("concurrent rejected reads share one status query and ordinary errors never request it", async () => {
  let completeRead;
  const state = fixture(() => new Promise((resolve) => { completeRead = resolve; }));
  await state.lifecycle.observeOperationError(new Error("database unavailable"));
  assert.equal(state.reads, 0);
  const errors = [FINAL_EXIT_ERROR,
    "instance detail reconciliation cannot begin while application shutdown is in progress",
    "runtime overview reconciliation cannot begin while application shutdown is in progress"];
  const pending = errors.map((error) => state.lifecycle.observeOperationError(error));
  assert.equal(state.reads, 1);
  completeRead({ requested: true });
  await Promise.all(pending);
  assert.equal(state.lifecycle.getSnapshot().requested, true);
});

test("recoverable update shutdown and unavailable or invalid status do not latch final exit", async () => {
  for (const readStatus of [
    async () => ({ requested: false }),
    async () => { throw new Error("native status unavailable"); },
    async () => ({ requested: "true" })
  ]) {
    const state = fixture(readStatus);
    await state.lifecycle.observeOperationError("instance detail reconciliation cannot begin while application shutdown is in progress");
    assert.equal(state.lifecycle.getSnapshot().requested, false);
  }
});
