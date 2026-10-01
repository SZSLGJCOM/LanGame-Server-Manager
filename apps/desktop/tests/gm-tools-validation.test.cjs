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
    shard: "caves", playerIndex: "2", prefab: "log", amount: "2",
    allPlayers: "false", placeInInventory: "true"
  });
  assert.equal(result.processKey, "caves");
  assert.equal(result.error, null);
  assert.doesNotMatch(result.commands[0], /c_give\(/);
  assert.match(result.commands[0], /AllPlayers\[2\]/);
  assert.match(result.commands[0], /components\.inventory:GiveItem/);
  assert.match(result.commands[0], /components\.inventoryitem/);
});

test("DST inventory protection fits the production command limit for both target modes", () => {
  for (const allPlayers of ["true", "false"]) {
    const result = buildGmToolCommand("dontstarve", "dst_give_item_to_player", {
      shard: "master", playerIndex: "2", prefab: "twigs", amount: "3", allPlayers, placeInInventory: "true"
    });
    assert.equal(result.error, null);
    assert.ok(result.commands[0].length <= 512);
  }
});

test("built-in DST selections stay within the native command length budget", () => {
  const { getDstPrefabOptions } = require("../src/views/servers/gm-tools.ts");
  for (const option of getDstPrefabOptions()) {
    const result = buildGmToolCommand("dontstarve", "dst_give_item_to_player", {
      shard: "caves", playerIndex: "999", prefab: option.value, amount: "999", allPlayers: "true", placeInInventory: "true"
    });
    assert.equal(result.error, null, option.value);
    assert.ok(result.commands[0].length <= 512, option.value);
  }
});

test("DST revive can select caves and rejects an unknown shard", () => {
  const tool = getGmToolCatalog("dontstarve").tools.find((entry) => entry.id === "dst_revive_player");
  assert.deepEqual(tool.fields.find((field) => field.key === "shard")?.options.map((option) => option.value), ["master", "caves"]);
  const result = buildGmToolCommand("dontstarve", tool.id, { playerIndex: "2", shard: "caves" });
  assert.equal(result.processKey, "caves");
  assert.match(result.commands[0], /playerghost/);
  assert.match(result.commands[0], /assert\(p/);
  const invalid = buildGmToolCommand("dontstarve", tool.id, { playerIndex: "2", shard: "unknown" });
  assert.deepEqual(invalid.commands, []);
  assert.ok(invalid.error);
});
