const assert = require("node:assert/strict");
const test = require("node:test");
const { deferred, flush, instanceActionFixture } = require("./helpers/instance-action-fixture.cjs");

test("a sent command keeps its receipt when an auxiliary panel read fails", async () => {
  const h = instanceActionFixture({ api: {
    readInstanceRuntimeWindowSnapshot: async () => { throw new Error("Window inspection unavailable"); }
  } });
  const receipt = await h.actions.handleSendRuntimeCommand("a", "save", null, { throwOnError: true });
  assert.equal(receipt, h.receipt);
  assert.equal(h.state.cache.a.details, h.details);
  assert.equal(h.state.selected.runtime.log_tail.lines[0], "current log");
  assert.equal(h.state.settled.at(-1).errors.runtimeWindows, "Window inspection unavailable");
  assert.equal(h.state.settled.at(-1).pending.length, 0);
  assert.equal(h.state.activity.at(-1).key, "activity.refreshServersFailed");
  assert.equal(h.state.activity.some((value) => value.key === "activity.runtimeCommandFailed"), false);
});

test("a sent command keeps its receipt when bootstrap refresh fails", async () => {
  const h = instanceActionFixture({ reloadBootstrap: async () => { throw new Error("Inventory unavailable"); } });
  assert.equal(await h.actions.handleSendRuntimeCommand("a", "save"), h.receipt);
  assert.equal(h.state.selected.details, h.details);
  assert.equal(h.state.activity.at(-1).key, "activity.refreshServersFailed");
  assert.match(h.state.activity.at(-1).params.message, /Inventory unavailable/);
});

test("a hanging auxiliary read reaches the shared deadline without losing the sent command", async () => {
  const backups = deferred();
  const h = instanceActionFixture({ api: { listInstanceBackups: () => backups.promise } });
  const pending = h.actions.handleSendRuntimeCommand("a", "save");
  await flush();
  assert.equal(h.state.selected.details, h.details, "available parts publish before the slow auxiliary read");
  h.expireReads();
  assert.equal(await pending, h.receipt);
  assert.match(h.state.settled.at(-1).errors.backups, /timed out/);
  backups.resolve([]);
  await flush();
});

test("dispatch failure preserves the original error and does not pretend a refresh is a retry", async () => {
  const failure = new Error("Command destination unavailable");
  const h = instanceActionFixture({ api: { sendInstanceRuntimeCommand: async () => { throw failure; } } });
  await assert.rejects(h.actions.handleSendRuntimeCommand("a", "save", null, { throwOnError: true }),
    (error) => error === failure);
  assert.deepEqual(h.state.calls.map((call) => call.name), ["sendInstanceRuntimeCommand"]);
  assert.equal(h.state.activity.at(-1).key, "activity.runtimeCommandFailed");
});

test("mutation readback invalidates a pending poll and serializes its fresh observation", async () => {
  const oldDetails = deferred();
  const freshDetails = { summary: { id: "a", name: "Saved Server A" }, settings_json: '{"saved":true}' };
  let detailReads = 0;
  const h = instanceActionFixture({ api: {
    readInstanceDetails: () => ++detailReads === 1 ? oldDetails.promise : Promise.resolve(freshDetails)
  } });
  const polling = h.reader().load("a", {
    signal: new AbortController().signal, onUpdate() {}, onProgress() {}
  });
  await flush();
  const command = h.actions.handleSendRuntimeCommand("a", "save");
  await flush();
  assert.equal(detailReads, 1, "uncancellable native reads must not overlap");
  oldDetails.resolve({ summary: { id: "a", name: "Before mutation" }, settings_json: "{}" });
  await polling;
  assert.equal(await command, h.receipt);
  assert.equal(detailReads, 2, "the mutation must not adopt the older poll's observation");
  assert.equal(h.state.selected.details, freshDetails);
});

test("mutation readback caches its instance without replacing a later selection", async () => {
  const details = deferred();
  const h = instanceActionFixture({ api: { readInstanceDetails: () => details.promise } });
  const pending = h.actions.handleSendRuntimeCommand("a", "save", null, { silent: true });
  await flush();
  h.state.selectedId = "b";
  const selected = { details: { summary: { id: "b" } } };
  h.state.selected = selected;
  details.resolve(h.details);
  assert.equal(await pending, h.receipt);
  assert.equal(h.state.cache.a.details, h.details);
  assert.equal(h.state.selected, selected);
  assert.equal(h.state.bootstrapRefreshes, 0);
});

test("a selection changed during bootstrap refresh keeps its own panel", async () => {
  const bootstrap = deferred();
  const h = instanceActionFixture({ reloadBootstrap: () => bootstrap.promise });
  const pending = h.actions.handleSendRuntimeCommand("a", "save");
  await flush();
  assert.equal(h.state.bootstrapRefreshes, 1);
  h.state.selectedId = "b";
  const selected = { details: { summary: { id: "b" } } };
  h.state.selected = selected;
  bootstrap.resolve();
  assert.equal(await pending, h.receipt);
  assert.equal(h.state.selected, selected);
  assert.equal(h.state.cache.a.details, h.details);
});

for (const retirement of ["pending", "completed"]) {
  test(`late mutation readback cannot restore an instance whose retirement is ${retirement}`, async () => {
    const details = deferred();
    const h = instanceActionFixture({ api: { readInstanceDetails: () => details.promise } });
    const pending = h.actions.handleSendRuntimeCommand("a", "save", null, { silent: true });
    await flush();
    h.state.cache = {};
    h.state.selectedId = "b";
    const selected = { details: { summary: { id: "b" } } };
    h.state.selected = selected;
    if (retirement === "pending") h.state.retiring.add("a");
    else h.state.instances = [{ id: "b" }];
    details.resolve(h.details);
    assert.equal(await pending, h.receipt);
    assert.equal(Object.hasOwn(h.state.cache, "a"), false);
    assert.equal(h.state.selected, selected);
  });
}
