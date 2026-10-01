const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const repo = path.resolve(__dirname, "../../..");

test("Necesse keeps cfg overrides optional and uses the current authority flag", () => {
  const native = require(path.join(repo, "modules/necesse/schema.json"));
  for (const key of ["max_client_latency_seconds", "unload_levels_cooldown", "dropped_items_life_minutes", "unload_settlements", "max_settlements_per_player", "max_settlers_per_settlement", "world_border_size"]) {
    assert.equal(Object.hasOwn(native.properties[key], "default"), false, key);
  }
  const moduleText = fs.readFileSync(path.join(repo, "modules/necesse/module.toml"), "utf8");
  assert.ok(moduleText.includes('"-strictserverauthority"'));
  assert.ok(!moduleText.includes('"-giveclientspower"'));
  assert.ok(moduleText.includes('"{{paths.data_dir}}/cfg/server.cfg"'));
});
