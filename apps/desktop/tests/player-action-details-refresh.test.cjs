const assert = require("node:assert/strict");
const test = require("node:test");
const { deferred, instanceActionFixture } = require("./helpers/instance-action-fixture.cjs");

function harness() {
  const command = deferred();
  const panel = deferred();
  const refreshStarted = deferred();
  const input = { instance_id: "a", snapshot_id: "snapshot-a", player_key: "alice", action_id: "ban_player" };
  const result = { action_id: "ban_player", status: "sent", executed_at_unix_ms: 1, summary: "sent" };
  const refreshed = { details: { summary: { id: "a" }, settings_json: '{"blacklist_entries":[{"platform":"Steam","userid":"76561198000000001"}]}' } };
  const h = instanceActionFixture({ api: {
    executeInstancePlayerAction: (value) => { assert.deepEqual(value, input); return command.promise; },
    readInstanceDetails: () => { refreshStarted.resolve(); return panel.promise.then((value) => value.details); }
  } });
  return { ...h, command, panel, refreshStarted, result, refreshed,
    execute: () => h.actions.handleExecutePlayerAction(input) };
}

test("online action waits for authoritative details and publishes the newly persisted roster", async () => {
  const h = harness();
  let completed = false;
  const pending = h.execute().then((result) => { completed = true; return result; });
  assert.equal(h.state.calls.length, 1);
  assert.equal(h.state.calls[0].name, "executeInstancePlayerAction");
  h.command.resolve(h.result);
  await h.refreshStarted.promise;
  assert.equal(completed, false);
  assert.deepEqual(h.state.calls.find((call) => call.name === "readInstanceDetails").args, ["a"]);
  h.panel.resolve(h.refreshed);
  assert.equal(await pending, h.result);
  assert.equal(h.state.selected.details, h.refreshed.details);
  assert.equal(h.state.cache.a.details, h.refreshed.details);
  assert.equal(h.state.bootstrapRefreshes, 1);
});

test("online action refresh caches its original instance without replacing a later selection", async () => {
  const h = harness();
  const pending = h.execute();
  h.command.resolve(h.result);
  await h.refreshStarted.promise;
  h.state.selectedId = "b";
  const otherPanel = { details: { summary: { id: "b" } } };
  h.state.selected = otherPanel;
  h.panel.resolve(h.refreshed);
  await pending;
  assert.equal(h.state.cache.a.details, h.refreshed.details);
  assert.equal(h.state.selected, otherPanel);
});

test("native readback failure still refreshes authoritative details and preserves the original rejection", async () => {
  const h = harness();
  const failure = new Error("Ban sent but native roster readback failed");
  const pending = h.execute();
  h.command.reject(failure);
  await h.refreshStarted.promise;
  h.panel.resolve(h.refreshed);
  await assert.rejects(pending, (error) => error === failure);
  assert.equal(h.state.selected.details, h.refreshed.details);
});

test("a second refresh failure cannot replace the original player action failure", async () => {
  const h = harness();
  const failure = new Error("Native readback failed");
  const pending = h.execute();
  h.command.reject(failure);
  await h.refreshStarted.promise;
  h.panel.reject(new Error("Details unavailable"));
  await assert.rejects(pending, (error) => error === failure);
  assert.equal(h.state.activity.at(-1).key, "activity.playerAccessRefreshFailed");
  assert.equal(h.state.activity.at(-1).params.message, "Details unavailable");
});

test("successful dispatch keeps its receipt while failed details refresh stays separately visible", async () => {
  const h = harness();
  const pending = h.execute();
  h.command.resolve(h.result);
  await h.refreshStarted.promise;
  h.panel.reject(new Error("Details unavailable"));
  assert.equal(await pending, h.result);
  assert.equal(h.state.activity.at(-1).key, "activity.playerAccessRefreshFailed");
  assert.equal(h.state.activity.at(-1).params.message, "Details unavailable");
});
