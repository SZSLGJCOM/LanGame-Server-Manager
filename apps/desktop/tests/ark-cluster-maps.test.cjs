const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
for (const extension of [".ts", ".tsx"]) require.extensions[extension] = (loaded, filename) => {
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const { ARK_ADDITIONAL_MAP_LIMIT, readAdditionalArkMaps, validateAdditionalArkMaps, readArkMapSuggestions,
  newArkMapId, getArkMapSettingsValidationIssues, buildArkMapPortGroups } = require("../src/views/settings/ark-cluster-maps.ts");
const map = (overrides = {}) => ({ id: "scorched", map_name: "ScorchedEarth_WP", name: "焦土", enabled: true, ...overrides });

test("maps preserve distinct save identities and paused configurations across JSON readback", () => {
  const settings = JSON.parse(JSON.stringify({ additional_maps: [map(), map({ id: "center", map_name: "TheCenter_WP", enabled: false })] }));
  assert.equal(validateAdditionalArkMaps(settings), null);
  assert.deepEqual(readAdditionalArkMaps(settings), settings.additional_maps);
  assert.deepEqual(readAdditionalArkMaps({}), []);
});

test("the map boundary rejects unsafe packages, duplicate identities and malformed topology", () => {
  const cases = [
    [{ additional_maps: null }, "shape"], [{ additional_maps: [{}] }, "shape"],
    [{ additional_maps: [map({ enabled: "true" })] }, "shape"],
    [{ additional_maps: [map({ unexpected: "preserve rather than silently discard" })] }, "shape"],
    [{ additional_maps: [map({ id: "../outside" })] }, "id"],
    [{ additional_maps: [map(), map()] }, "duplicate"],
    [{ additional_maps: [map({ map_name: "TheIsland?Port=1" })] }, "map"],
    [{ additional_maps: [map({ map_name: "../TheIsland" })] }, "map"],
    [{ additional_maps: [map({ map_name: "a".repeat(129) })] }, "map"],
    [{ additional_maps: [map({ name: "\u0085" })] }, "name"],
    [{ additional_maps: [map({ name: 'Bad"Name' })] }, "name"],
    [{ additional_maps: [map({ name: "Bad?Name" })] }, "name"],
    [{ additional_maps: [map({ name: "Leading space " })] }, "name"],
    [{ additional_maps: [map({ name: "Bad\u2028Name" })] }, "name"],
    [{ additional_maps: [map({ name: " " })] }, "name"],
    [{ additional_maps: [map({ name: "界".repeat(81) })] }, "name"],
    [{ additional_maps: Array.from({ length: ARK_ADDITIONAL_MAP_LIMIT + 1 }, (_, index) => map({ id: `world-${index}` })) }, "limit"]
  ];
  for (const [settings, reason] of cases) {
    assert.equal(validateAdditionalArkMaps(settings), reason);
    assert.equal(getArkMapSettingsValidationIssues(settings, (key) => key)[0].fieldKey, "additional_maps");
  }
  assert.equal(validateAdditionalArkMaps({ additional_maps: [map({ name: "🦖".repeat(80) })] }), null);
  assert.equal(validateAdditionalArkMaps({ additional_maps: [map({ name: "Bob's map" })] }), null);
});

test("the map picker reads each edition's native suggestions and keeps custom packages available", () => {
  for (const [edition, primary] of [["arksurvivalascended", "TheIsland_WP"], ["arksurvivalevolved", "TheIsland"]]) {
    const source = fs.readFileSync(path.resolve(__dirname, `../../../modules/${edition}/schema.json`), "utf8");
    const chinese = readArkMapSuggestions(source, "zh-CN");
    assert.ok(chinese.some((entry) => entry.value === primary && entry.label.includes("孤岛")));
    assert.ok(chinese.some((entry) => /Ragnarok/.test(entry.value)));
    assert.equal(new Set(chinese.map((entry) => entry.value)).size, chinese.length);
    assert.equal(readArkMapSuggestions(source, "en-US").find((entry) => entry.value === primary).label, "The Island");
  }
});

test("new maps receive fresh safe identities with bounded collision handling", () => {
  const existing = newArkMapId([], () => "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
  let calls = 0;
  const next = newArkMapId([existing], () => ++calls === 1 ? "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa" : "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb");
  assert.notEqual(next, existing);
  assert.match(next, /^[a-z0-9][a-z0-9-]{0,31}$/);
  assert.equal(calls, 2);
  let collisions = 0;
  assert.throws(() => newArkMapId([existing], () => { collisions++; return "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"; }), /unique ARK map identity/);
  assert.equal(collisions, 10);
});

test("ASE map port groups keep game and peer together including paused maps", () => {
  const base = [{ id: "game_peer", members: ["game", "peer"], member_offsets: { game: 0, peer: 1 } }];
  const settingsJson = JSON.stringify({ additional_maps: [map(), map({ id: "center", enabled: false })] });
  const groups = buildArkMapPortGroups("arksurvivalevolved", settingsJson, base);
  assert.equal(groups.length, 3);
  assert.deepEqual(groups[1], { id: "map-scorched-game_peer", members: ["map-scorched-game", "map-scorched-peer"], member_offsets: { "map-scorched-game": 0, "map-scorched-peer": 1 } });
  assert.deepEqual(groups[2].members, ["map-center-game", "map-center-peer"]);
  assert.equal(buildArkMapPortGroups("terraria", settingsJson, base), base);
  assert.equal(buildArkMapPortGroups("arksurvivalevolved", "bad json", base), base);
  assert.deepEqual(buildArkMapPortGroups("arksurvivalascended", settingsJson, []), []);
});
