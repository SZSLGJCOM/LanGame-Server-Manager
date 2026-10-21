const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".ts"] = function compileTypeScript(module, filename) {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const { buildGmToolCommand, getGmToolCatalog } = require("../src/views/servers/gm-tools.ts");

test("every remote tool enable switch exists in its game's current schema", () => {
  for (const moduleId of ["arksurvivalevolved", "arksurvivalascended"]) {
    const schema = JSON.parse(fs.readFileSync(require("node:path").join(__dirname, "../../../modules", moduleId, "schema.json"), "utf8"));
    const { getInitialGmToolValues } = require("../src/views/servers/gm-tools.ts");
    for (const tool of getGmToolCatalog(moduleId).tools) {
      const result = buildGmToolCommand(moduleId, tool.id, { ...getInitialGmToolValues(tool), playerId: "123" });
      if (tool.id === "ark_spawn_creature") {
        assert.deepEqual(result.commands, []);
        continue;
      }
      const key = result.dispatchOptions.enabledSettingKey;
      if (key) assert.ok(Object.hasOwn(schema.properties, key), `${moduleId}:${tool.id} references unknown ${key}`);
    }
  }
});

test("ARK time rejects impossible clock values before dispatch", () => {
  for (const time of ["24:00", "23:60", "99:99", "-1:30"]) {
    const result = buildGmToolCommand("arksurvivalevolved", "ark_set_time", { time });
    assert.deepEqual(result.commands, [], time);
    assert.ok(result.error, time);
  }
  for (const time of ["00:00", "9:05", "23:59"]) {
    assert.equal(buildGmToolCommand("arksurvivalevolved", "ark_set_time", { time }).error, null);
  }
});

test("Unicode blueprint commands respect the native 512-byte dispatch boundary", () => {
  const build = (name) => buildGmToolCommand("arksurvivalascended", "ark_give_item_to_player", {
    itemMode: "blueprint", playerId: "123", blueprintPath: `Blueprint'/Game/Mods/${name}/Item.Item'`,
    quantity: "1", quality: "0", blueprint: "0"
  });
  const available = 512 - Buffer.byteLength(build("a").commands[0], "utf8") + 1;
  const boundary = "中".repeat(Math.floor(available / 3)) + "a".repeat(available % 3);
  const accepted = build(boundary);
  assert.equal(accepted.error, null);
  assert.equal(Buffer.byteLength(accepted.commands[0], "utf8"), 512);
  const rejected = build(`${boundary}a`);
  assert.deepEqual(rejected.commands, []);
  assert.ok(rejected.error);
});

test("ARK reward rejects extra columns and bounds one submission", () => {
  for (const lines of ["9,1,0,0,ignored", Array(65).fill("9,1,0,0").join("\n")]) {
    const result = buildGmToolCommand("arksurvivalascended", "ark_give_item_to_player", {
      itemMode: "batch", playerId: "123456789", lines
    });
    assert.deepEqual(result.commands, []);
    assert.ok(result.error);
  }
});

test("ARK structured player target cannot insert more command arguments", () => {
  for (const playerId of ["123 9", "123; SaveWorld", "123\u0000", "123|SaveWorld"]) {
    const result = buildGmToolCommand("arksurvivalascended", "ark_give_item_to_player", {
      itemMode: "number", playerId, itemId: "9", quantity: "1", quality: "0", blueprint: "0"
    });
    assert.deepEqual(result.commands, []);
    assert.ok(result.error);
  }
});

test("DST item action addresses the selected inventory, not ConsoleCommandPlayer", () => {
  const result = buildGmToolCommand("dontstarve", "dst_give_item_to_player", {
    shard: "caves", playerId: "KU_ab12", prefab: "log", amount: "2",
    allPlayers: "false", placeInInventory: "true"
  });
  assert.equal(result.processKey, "caves");
  assert.equal(result.error, null);
  assert.doesNotMatch(result.commands[0], /c_give\(/);
  assert.match(result.commands[0], /LookupPlayerInstByUserID\("KU_ab12"\)/);
  assert.match(result.commands[0], /components\.inventory:GiveItem/);
  assert.match(result.commands[0], /components\.inventoryitem/);
});

test("DST inventory protection fits the production command limit for both target modes", () => {
  for (const allPlayers of ["true", "false"]) {
    const result = buildGmToolCommand("dontstarve", "dst_give_item_to_player", {
      shard: "master", playerId: "KU_ab12", prefab: "twigs", amount: "3", allPlayers, placeInInventory: "true"
    });
    assert.equal(result.error, null);
    assert.ok(result.commands[0].length <= 512);
  }
});

test("built-in DST selections stay within the native command length budget for the longest player id", () => {
  const { getDstPrefabOptions } = require("../src/views/servers/gm-tools.ts");
  for (const option of getDstPrefabOptions()) {
    for (const allPlayers of ["true", "false"]) {
      const result = buildGmToolCommand("dontstarve", "dst_give_item_to_player", {
        shard: "caves", playerId: "x".repeat(32), prefab: option.value, amount: "999", allPlayers, placeInInventory: "true"
      });
      assert.equal(result.error, null, `${option.value}:${allPlayers}`);
      assert.ok(result.commands[0].length <= 512, option.value);
    }
  }
});

test("DST accepts maximum-length custom prefabs for every target and placement mode", () => {
  for (const allPlayers of ["true", "false"]) {
    for (const placeInInventory of ["true", "false"]) {
      const result = buildGmToolCommand("dontstarve", "dst_give_item_to_player", {
        shard: "caves", playerId: "x".repeat(32), prefab: "a".repeat(64), amount: "999", allPlayers, placeInInventory
      });
      assert.equal(result.error, null, `${allPlayers}:${placeInInventory}`);
      assert.ok(Buffer.byteLength(result.commands[0], "utf8") <= 512);
    }
  }
});

test("DST revive can select every managed shard and rejects an unknown shard", () => {
  const tool = getGmToolCatalog("dontstarve").tools.find((entry) => entry.id === "dst_revive_player");
  for (const shard of ["master", "caves", "islands", "volcano"]) {
    const result = buildGmToolCommand("dontstarve", tool.id, { playerId: "KU_ab12", shard });
    assert.equal(result.error, null, shard);
    assert.equal(result.processKey, shard);
    assert.match(result.commands[0], /\.userid=="KU_ab12"/);
    assert.match(result.commands[0], /playerghost/);
    assert.match(result.commands[0], /assert\(p/);
  }
  const invalid = buildGmToolCommand("dontstarve", tool.id, { playerId: "KU_ab12", shard: "unknown" });
  assert.deepEqual(invalid.commands, []);
  assert.ok(invalid.error);
});

test("DST time of day always drives the Master clock from a fixed phase list", () => {
  const expected = { day: 'TheWorld:PushEvent("ms_setphase", "day")', dusk: 'TheWorld:PushEvent("ms_setphase", "dusk")',
    night: 'TheWorld:PushEvent("ms_setphase", "night")', next: 'TheWorld:PushEvent("ms_nextcycle")' };
  for (const [phase, lua] of Object.entries(expected)) {
    const result = buildGmToolCommand("dontstarve", "dst_set_phase", { phase, shard: "caves" });
    assert.equal(result.error, null, phase);
    assert.equal(result.processKey, "master", phase);
    assert.equal(result.dispatchOptions.transport, "stdin");
    assert.deepEqual(result.commands, [lua]);
  }
  for (const phase of ["", "Day", "evening", 'day") c_shutdown() --']) {
    const result = buildGmToolCommand("dontstarve", "dst_set_phase", { phase });
    assert.deepEqual(result.commands, [], phase);
    assert.ok(result.error, phase);
  }
});

test("DST item commands keep the selected managed shard for inventory and nearby spawning", () => {
  for (const shard of ["master", "caves", "islands", "volcano"]) {
    for (const placeInInventory of ["true", "false"]) {
      const result = buildGmToolCommand("dontstarve", "dst_give_item_to_player", {
        shard, playerId: "KU_ab12", prefab: "log", amount: "2", allPlayers: "false", placeInInventory
      });
      assert.equal(result.error, null, `${shard}:${placeInInventory}`);
      assert.equal(result.processKey, shard);
      assert.equal(result.dispatchOptions.transport, "stdin");
      assert.match(result.commands[0], /LookupPlayerInstByUserID\("KU_ab12"\)/);
    }
  }
  for (const shard of ["unknown", "../Master", 'islands;print("injected")']) {
    const result = buildGmToolCommand("dontstarve", "dst_give_item_to_player", {
      shard, playerId: "KU_ab12", prefab: "log", amount: "2"
    });
    assert.ok(result.error);
    assert.deepEqual(result.commands, []);
  }
});

test("DST player actions target a Klei user id selected on the map and reject anything else", () => {
  for (const id of ["dst_give_item_to_player", "dst_revive_player"]) {
    const base = { shard: "master", prefab: "log", amount: "1", allPlayers: "false", placeInInventory: "true" };
    assert.equal(buildGmToolCommand("dontstarve", id, base).error, "Select a player first.", id);
    for (const playerId of ['KU_1" then os.exit() end --', "KU 1", "x".repeat(33)]) {
      const result = buildGmToolCommand("dontstarve", id, { ...base, playerId });
      assert.deepEqual(result.commands, [], `${id}:${playerId}`);
      assert.ok(result.error);
    }
  }
  const longest = buildGmToolCommand("dontstarve", "dst_give_item_to_player", {
    shard: "volcano", playerId: "x".repeat(32), prefab: "log", amount: "999", allPlayers: "false", placeInInventory: "true"
  });
  assert.equal(longest.error, null);
  assert.ok(longest.commands[0].length <= 512);
});
