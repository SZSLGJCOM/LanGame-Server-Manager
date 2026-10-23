const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".ts"] = function compileTypeScript(module, filename) {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const {
  buildGmToolCommand, getDstPrefabOptions, searchDstPrefabOptions, localizeDstPrefabName
} = require("../src/views/servers/gm-tools.ts");

// Native 759671: prefab registrations and STRINGS.NAMES from strings.lua/chinese_s.po.
// Keep the expected names independent of the production catalog and search index.
const CONTENT = [
  ["armor_rocky", "嶙峋甲壳", "Craggy Carapace"],
  ["bat_bosscorpsehat", "蝙蝠帽", "Bat Hat"],
  ["queen_torch", "女王火炬", "Queen's Torch"],
  ["king_cane", "国王魔杖", "King's Staff"],
  ["mask_charlie_bosshat", "女王面容", "Queen's Visage"],
  ["atrium_ritual_organ_rocky", "枯萎钳爪", "Blighted Claw"],
  ["atrium_ritual_organ_bat", "枯萎喉头", "Blighted Larynx"],
  ["atrium_ritual_organ_worm", "枯萎内脏", "Blighted Guts"],
  ["bat_boss", "猎犬蝙蝠", "Hound Batilisk"],
  ["bat_boss_shadow", "枯萎猎犬蝙蝠", "Blighted Hound Batilisk"],
  ["rocky_boss", "岩石大师", "Rock Master"],
  ["rocky_boss_shadow", "枯萎岩石大师", "Blighted Rock Master"],
  ["worm_boss_shadow", "枯萎洞穴巨虫", "Blighted Great Depths Worm"],
  ["charlie_boss", "藏影女王", "Shrouded Queen"],
  ["shrouden", "暗影憎恶", "Umbral Abomination"],
  ["charlie_boss_runner", "恐怖猎手", "Grim Harrier"],
  ["batbosscave", "巨蝠洞", "Big Bat Cave"]
];
const FIGURES = [
  ["batboss_shadow", "枯萎猎犬蝙蝠", "Blighted Hound Batilisk"],
  ["rockyboss_shadow", "枯萎岩石大师", "Blighted Rock Master"],
  ["wormboss_shadow", "枯萎洞穴巨虫", "Blighted Great Depths Worm"],
  ["charlie_boss", "藏影女王", "Shrouded Queen"],
  ["shrouden", "暗影憎恶", "Umbral Abomination"]
];
for (const [piece, zh, en] of FIGURES) {
  CONTENT.push([`chesspiece_${piece}_sketch`, `${zh}雕塑草图`, `${en} Figure Sketch`]);
  for (const [material, zhMaterial, enMaterial] of [["marble", "大理石", "Marble"], ["stone", "石砖", "Cut Stone"], ["moonglass", "月亮碎片", "Moon Shard"]]) {
    CONTENT.push([`chesspiece_${piece}_${material}`, `${zh}雕塑（${zhMaterial}）`, `${en} Figure (${enMaterial})`]);
  }
}

test("Cursed Confrontation Part 2 content is searchable by its official Chinese and English names", () => {
  const catalog = getDstPrefabOptions();
  assert.equal(CONTENT.length, 37);
  assert.equal(new Set(catalog.map(option => option.value)).size, catalog.length, "Prefab identities must stay unique");
  for (const [prefab, zh, en] of CONTENT) {
    const option = catalog.find(candidate => candidate.value === prefab);
    assert.ok(option, `${prefab} must be a catalog selection`);
    assert.equal(localizeDstPrefabName(option, "zh-CN"), zh, prefab);
    assert.equal(localizeDstPrefabName(option, "en-US"), en, prefab);
    for (const query of [prefab, zh, en]) {
      assert.ok(searchDstPrefabOptions(query, catalog.length).includes(option), `${query} must find ${prefab}`);
    }
  }
});

test("new selections use the existing validated spawn path on every managed shard", () => {
  for (const [prefab] of CONTENT) {
    for (const shard of ["master", "caves", "islands", "volcano"]) {
      for (const placeInInventory of ["true", "false"]) {
        const result = buildGmToolCommand("dontstarve", "dst_give_item_to_player", {
          shard, prefab, playerId: "KU_catalog_player", amount: "1", placeInInventory
        });
        assert.equal(result.error, null, `${prefab}:${shard}:${placeInInventory}`);
        assert.equal(result.processKey, shard);
        assert.equal(result.dispatchOptions.transport, "stdin");
        assert.equal(result.commands.length, 1);
        assert.ok(Buffer.byteLength(result.commands[0], "utf8") <= 512);
      }
    }
  }
});

test("internal encounter controllers, projectiles, effects and body segments are not item suggestions", () => {
  const catalog = new Set(getDstPrefabOptions().map(option => option.value));
  for (const prefab of ["charlie_boss_trial", "charlie_boss_projectile", "charlie_boss_aoe_flame_fx",
    "atriummarker_gate_center", "atrium_ritual_marking", "worm_boss_shadow_head", "worm_boss_shadow_segment",
    "queen_torchlight", "king_cane_fx", "chesspiece_shrouden_builder"]) {
    assert.equal(catalog.has(prefab), false, prefab);
  }
});
