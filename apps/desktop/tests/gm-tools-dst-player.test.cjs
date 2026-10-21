const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");
require.extensions[".ts"] = function compileTypeScript(module, filename) {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const { buildGmToolCommand, getGmToolCatalog } = require(path.join(desktopRoot, "src", "views", "servers", "gm-tools.ts"));
const { mapPointToWorld } = require(path.join(desktopRoot, "src", "dst-world-map.ts"));

const build = (id, values) => buildGmToolCommand("dontstarve", id, values);
const player = { shard: "caves", playerId: "KU_ab12" };
const PLAYER_TOOLS = ["dst_restore_player", "dst_toggle_godmode", "dst_unlock_recipes"];

test("DST player tools target the selected player's shard with one validated stdin command", () => {
  for (const id of [...PLAYER_TOOLS, "dst_teleport_to_player", "dst_teleport_to_point", "dst_clear_ground_items"]) {
    assert.ok(getGmToolCatalog("dontstarve")?.tools.some(tool => tool.id === id), id);
  }
  for (const id of PLAYER_TOOLS) {
    const result = build(id, player);
    assert.equal(result.error, null, id);
    assert.equal(result.processKey, "caves");
    assert.equal(result.dispatchOptions.transport, "stdin");
    assert.equal(result.commands.length, 1);
    assert.match(result.commands[0], /^local p;for _,c in ipairs\(AllPlayers\) do if c\.userid=="KU_ab12" then p=c end end;assert\(p,"Player is not on this shard"\);assert\(not p:HasTag\("playerghost"\)/);
  }
});

test("DST player tools emit the scripts.zip component calls", () => {
  const restore = build("dst_restore_player", player).commands[0];
  for (const call of ["c.health:SetPercent(1)", "c.hunger:SetPercent(1)", "c.sanity:SetPercent(1)", "c.moisture:SetPercent(0)", "c.temperature:SetTemperature(TUNING.STARTING_TEMP)"]) {
    assert.ok(restore.includes(call), call);
  }
  assert.match(build("dst_toggle_godmode", player).commands[0], /h:SetInvincible\(not h:IsInvincible\(\)\)/);
  // GiveAllRecipes toggles free build mode, so a second click must not switch it off.
  assert.match(build("dst_unlock_recipes", player).commands[0], /if not b\.freebuildmode then b:GiveAllRecipes\(\) end;p:PushEvent\("techlevelchange"\)/);
});

test("DST teleports stay on one shard and only accept walkable numeric points", () => {
  const toPlayer = build("dst_teleport_to_player", { ...player, destinationId: "KU_zz9" });
  assert.equal(toPlayer.error, null);
  assert.match(toPlayer.commands[0], /local q;for _,c in ipairs\(AllPlayers\) do if c\.userid=="KU_zz9" then q=c end end;assert\(q,"Destination player is not on this shard"\)/);
  assert.match(toPlayer.commands[0], /p\.Physics:Teleport\(q\.Transform:GetWorldPosition\(\)\) else p\.Transform:SetPosition/);
  assert.match(build("dst_teleport_to_player", { ...player, destinationId: "KU_ab12" }).error, /different destination/);

  const toPoint = build("dst_teleport_to_point", { ...player, x: "-12.5", z: "40" });
  assert.equal(toPoint.error, null);
  assert.match(toPoint.commands[0], /assert\(TheWorld\.Map:IsPassableAtPoint\(-12\.5,0,40\),"Destination is not walkable"\);if p\.Physics then p\.Physics:Teleport\(-12\.5,0,40\)/);
  for (const bad of ["", "1e3", "NaN", "10001", "1);os.exit()", "0x10"]) {
    assert.match(build("dst_teleport_to_point", { ...player, x: bad, z: "0" }).error ?? "", /World X/, bad);
  }
});

test("DST ground cleanup removes only loose items on one shard and reports the count", () => {
  const result = build("dst_clear_ground_items", { shard: "master" });
  assert.equal(result.error, null);
  assert.equal(result.processKey, "master");
  const lua = result.commands[0];
  assert.match(lua, /i\.owner==nil and v:IsValid\(\) and not v:IsInLimbo\(\) and v\.entity:GetParent\(\)==nil/);
  assert.match(lua, /HasOneOfTags\(\{"irreplaceable","nonpotatable","heavy","trap","mine","player"\}\)/);
  assert.match(lua, /components\.equippable==nil and v\.components\.container==nil and v\.components\.health==nil then v:Remove\(\);n=n\+1/);
  assert.match(lua, /print\("\[LGSM\] Removed "\.\.n\.\." dropped items"\)/);
  assert.match(build("dst_clear_ground_items", { shard: "forest" }).error ?? "", /Shard/);
});

test("DST player tools reject injected identifiers and unknown shards", () => {
  for (const id of [...PLAYER_TOOLS, "dst_teleport_to_point"]) {
    for (const playerId of ["", "KU_a\"..os.exit()..\"", "KU a", "x".repeat(33)]) {
      const result = build(id, { shard: "master", playerId, x: "0", z: "0" });
      assert.ok(result.error, `${id} ${playerId}`);
      assert.deepEqual(result.commands, []);
    }
    assert.ok(build(id, { ...player, shard: "caves;os.exit()", x: "0", z: "0" }).error, id);
  }
  assert.ok(build("dst_teleport_to_player", { ...player, destinationId: "KU\")" }).error);
});

test("map points convert to world positions through a known player's anchor", () => {
  const snapshot = { height: 400, tileScale: 4 };
  const anchor = { x: 100, z: -40, mapX: 200, mapZ: 150 };
  assert.deepEqual(mapPointToWorld(snapshot, anchor, { x: 200, y: 250 }), { x: 100, z: -40 });
  assert.deepEqual(mapPointToWorld(snapshot, anchor, { x: 210.5, y: 245 }), { x: 142, z: -20 });
});

test("DST spawns validate the prefab id and guard it with PrefabExists", () => {
  const { DST_PREFAB_ID_PATTERN, readDstPrefabId } = require(path.join(desktopRoot, "src", "views", "servers", "gm-tool-values.ts"));
  const { getDstPrefabOptions } = require(path.join(desktopRoot, "src", "views", "servers", "gm-tools.ts"));
  const buildDstGive = (prefab) => build("dst_give_item_to_player", { ...player, prefab, amount: "2" });
  assert.ok(getDstPrefabOptions().every(option => DST_PREFAB_ID_PATTERN.test(option.value)), "Every catalog prefab fits the typed-code pattern");
  assert.deepEqual(readDstPrefabId({ prefab: "my_mod_item" }, "prefab", "Item"), { value: "my_mod_item", error: null });
  for (const bad of ["", "Log", 'log")', "log;c_godmode()", "log name", "a".repeat(65), "../log", "log\\n"]) {
    assert.ok(readDstPrefabId({ prefab: bad }, "prefab", "Item").error, `${JSON.stringify(bad)} must be rejected`);
    const built = buildDstGive(bad);
    assert.ok(built.error && !built.commands?.length, `${JSON.stringify(bad)} must never reach Lua`);
  }
  const { commands: [command], error } = buildDstGive("my_mod_item");
  assert.equal(error, null);
  assert.ok(command.startsWith('local f="my_mod_item";assert(PrefabExists(f),"Unknown prefab");'), command);
  assert.ok(command.indexOf("PrefabExists") < command.indexOf("SpawnPrefab"), "The guard runs before anything spawns");
});
