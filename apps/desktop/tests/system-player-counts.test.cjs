const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const sourcePath = path.join(__dirname, "../src/system-player-counts.ts");
const moduleScope = { exports: {} };
vm.runInNewContext(transpileTypeScript(fs.readFileSync(sourcePath, "utf8"), sourcePath), {
  module: moduleScope, exports: moduleScope.exports, require
}, { filename: sourcePath });
const { resolveSystemPlayerCounts } = moduleScope.exports;

function resolve(changes = {}, fallback = 2) {
  return resolveSystemPlayerCounts({
    running_instances: 2,
    player_count_queried_instances: 2,
    player_count_queryable_instances: 2,
    total_online_players: 0,
    total_player_capacity: 32,
    ...changes
  }, fallback);
}

test("no running instances reports zero even when a stopped snapshot retains counts", () => {
  const model = resolve({ running_instances: 0, total_online_players: 9 });
  assert.equal(model.state, "idle");
  assert.equal(model.onlinePlayers, 0);
  assert.equal(model.queriedInstances, 0);
  assert.equal(model.capacity, null);
});

test("zero successful queries leaves the count unknown, including a stale positive count", () => {
  for (const total_online_players of [0, 7]) {
    const model = resolve({ player_count_queried_instances: 0, total_online_players });
    assert.equal(model.state, "unknown");
    assert.equal(model.onlinePlayers, null);
    assert.equal(model.capacity, 32);
  }
});

test("partial coverage provides a known lower bound, including a valid zero", () => {
  for (const total_online_players of [0, 7]) {
    const model = resolve({ player_count_queried_instances: 1, total_online_players });
    assert.equal(model.state, "partial");
    assert.equal(model.onlinePlayers, total_online_players);
    assert.equal(model.queriedInstances, 1);
  }
});

test("querying every queryable instance is still partial when another running instance is unsupported", () => {
  const model = resolve({ player_count_queried_instances: 1, player_count_queryable_instances: 1 });
  assert.equal(model.state, "partial");
  assert.equal(model.runningInstances, 2);
});

test("complete coverage preserves a real zero or a positive total", () => {
  for (const total_online_players of [0, 12]) {
    const model = resolve({ total_online_players });
    assert.equal(model.state, "complete");
    assert.equal(model.onlinePlayers, total_online_players);
  }
});

test("missing or invalid player totals never become zero", () => {
  for (const total_online_players of [undefined, null, NaN, Infinity, -1, 0.5, Number.MAX_SAFE_INTEGER + 1]) {
    const model = resolve({ total_online_players });
    assert.equal(model.state, "unknown");
    assert.equal(model.onlinePlayers, null);
  }
});

test("invalid or inconsistent coverage cannot claim complete or partial counts", () => {
  for (const changes of [
    { player_count_queried_instances: NaN },
    { player_count_queried_instances: -1 },
    { player_count_queried_instances: 0.5 },
    { player_count_queried_instances: 3 },
    { player_count_queryable_instances: undefined },
    { player_count_queryable_instances: Infinity },
    { player_count_queryable_instances: -1 },
    { player_count_queryable_instances: 3 },
    { player_count_queryable_instances: 1 }
  ]) {
    const model = resolve(changes);
    assert.equal(model.state, "unknown", JSON.stringify(changes));
    assert.equal(model.onlinePlayers, null);
    assert.equal(model.queriedInstances, null);
  }
});

test("invalid running snapshots use the observed instance count", () => {
  for (const running_instances of [undefined, null, NaN, Infinity, -1, 1.5]) {
    const model = resolve({ running_instances, player_count_queried_instances: 1 });
    assert.equal(model.runningInstances, 2);
    assert.equal(model.state, "partial");
  }
});

test("invalid fallback counts cannot leak non-finite or negative numbers", () => {
  for (const fallback of [NaN, Infinity, -1, 1.5]) {
    const model = resolve({ running_instances: undefined }, fallback);
    assert.equal(model.state, "idle");
    assert.equal(model.runningInstances, 0);
    assert.equal(model.onlinePlayers, 0);
  }
});

test("invalid, zero or underreported capacity does not discard a valid player count", () => {
  for (const total_player_capacity of [undefined, null, NaN, Infinity, -1, 0, 2, 2.5]) {
    const model = resolve({ total_online_players: 3, total_player_capacity });
    assert.equal(model.state, "complete");
    assert.equal(model.onlinePlayers, 3);
    assert.equal(model.capacity, null);
  }
});
