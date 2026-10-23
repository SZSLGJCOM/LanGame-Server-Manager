const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { sameDstMapWorld, mergeDstMapPositions } = require("../src/dst-map-session.ts");
const { dstMapErrorCode } = require("../src/dst-map-errors.ts");

const initial = () => ({ instanceId: "world-instance", shard: "master", runId: 7, session: "native-session",
  geometryRevision: "15:0:30:40:1",
  width: 400, height: 400, tileScale: 4, tiles: [1, 2], art: { icons: {} },
  players: [{ id: "KU_USER", mapX: 10 }], capturedAtUnixMs: 1000, day: 3, season: "summer", phase: "day" });
test("position updates preserve terrain references and replace actual players and world clock", () => {
  const first = initial();
  const next = { ...first, tiles: null, art: null, players: [], capturedAtUnixMs: 4000, day: 4, phase: "night" };
  const merged = mergeDstMapPositions(first, next);
  assert.equal(merged.tiles, first.tiles);
  assert.equal(merged.art, first.art);
  assert.deepEqual(merged.players, []);
  assert.equal(merged.day, 4);
  assert.equal(merged.phase, "night");
  assert.equal(merged.capturedAtUnixMs, 4000);
});
test("any native identity or geometry change requires complete terrain before displaying positions", () => {
  const first = initial();
  for (const change of [{ instanceId: "other" }, { shard: "caves" }, { runId: 8 }, { session: "regenerated" },
    { width: 401 }, { height: 401 }, { tileScale: 5 }, { geometryRevision: "15:1:30:40:2" }]) {
    const next = { ...first, ...change, tiles: null, art: null };
    assert.equal(sameDstMapWorld(first, next), false);
    assert.equal(mergeDstMapPositions(first, next), null);
  }
  assert.equal(mergeDstMapPositions(null, { ...first, tiles: null }), null);
  const regenerated = { ...first, session: "regenerated", tiles: [3] };
  assert.equal(mergeDstMapPositions(first, regenerated), regenerated);
});
test("only explicit bounded host error codes classify world readiness", () => {
  assert.equal(dstMapErrorCode("DST_MAP_WORLD_NOT_READY: World is not ready."), "WORLD_NOT_READY");
  assert.equal(dstMapErrorCode("DST_MAP_RESPONSE_TIMEOUT: No complete response."), "RESPONSE_TIMEOUT");
  for (const value of ["world_not_ready", "Mod failed: DST_MAP_WORLD_NOT_READY: false", "DST_MAP_GUESSED_FAILURE: x",
    "native shard connection lost", "DST_MAP_WORLD_NOT_READY:no separator"]) assert.equal(dstMapErrorCode(value), null);
});
